//! One MCS session: the live registrar and the read loop. A client heartbeat
//! and an idle timeout bound how long a silent socket can look connected.

use std::time::Duration;

use async_trait::async_trait;
use fcm_push_listener::{
    CheckedSession, Message, MessageStream, Registration, new_heartbeat_ack, new_heartbeat_ping,
    register,
};
use futures_util::StreamExt;
use tokio::{io::AsyncWriteExt, sync::watch, time::Instant};

use super::{PushError, RingPushService, dispatch, persist, policy::Ending, rotation};
use crate::{
    ring_client::RingClient, ring_push_store::RingPushState, ring_push_support::unix_timestamp,
};

const FIREBASE_APP_ID: &str = "1:876313859327:android:e10ec6ddb3c81f39";
const FIREBASE_PROJECT_ID: &str = "ring-17770";
const FIREBASE_API_KEY: &str = "AIzaSyCv-hdFBmmdBBJadNy-TFwB-xN_H5m3Bk8";
/// Client heartbeat; the server acknowledges each ping.
pub(super) const PING_INTERVAL: Duration = Duration::from_mins(5);
/// Two missed heartbeats: the socket is dead even if TCP has not noticed.
pub(super) const IDLE_TIMEOUT: Duration = Duration::from_mins(10);

pub(super) struct LiveRegistrar<'a> {
    pub http: &'a reqwest::Client,
    pub client: &'a RingClient,
    pub service: &'a RingPushService,
}

#[async_trait]
impl rotation::PushRegistrar for LiveRegistrar<'_> {
    async fn register(&self) -> Result<Registration, PushError> {
        register(
            self.http,
            FIREBASE_APP_ID,
            FIREBASE_PROJECT_ID,
            FIREBASE_API_KEY,
            None,
        )
        .await
        .map_err(|error| PushError::Fcm("registration", dispatch::bounded(&error)))
    }

    async fn hand_off(&self, token: &str) -> Result<Vec<String>, PushError> {
        self.client.register_push_token(token).await?;
        self.service.reauth_state(false);
        let devices = self.service.subscribe_devices(self.client).await?;
        let metrics = &self.service.metrics;
        metrics.registered();
        metrics.set_legacy_record(self.client.push_record_legacy().await);
        Ok(devices)
    }
}

enum Event {
    Inbound(Option<Result<Message, fcm_push_listener::Error>>),
    Ping,
    Idle,
    Reload,
}

impl RingPushService {
    /// Reads until the connection ends; `connected_at` marks the login.
    pub(super) async fn listen(
        &self,
        checked: CheckedSession,
        state: &mut RingPushState,
        devices: &[String],
        reload: &mut watch::Receiver<u64>,
        connected_at: &mut Option<Instant>,
    ) -> Result<Ending, PushError> {
        let connection = checked
            .new_connection(state.persistent_ids.clone())
            .await
            .map_err(|error| PushError::Fcm("connection", dispatch::bounded(&error)))?;
        let mut stream = MessageStream::wrap(connection, &state.registration.keys);
        let now = Instant::now();
        *connected_at = Some(now);
        self.metrics.set_connected(true);
        let mut ping = tokio::time::interval_at(now + PING_INTERVAL, PING_INTERVAL);
        ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last_inbound = now;
        loop {
            let event = tokio::select! {
                message = stream.next() => Event::Inbound(message),
                _ = ping.tick() => Event::Ping,
                () = tokio::time::sleep_until(last_inbound + IDLE_TIMEOUT) => Event::Idle,
                _ = reload.changed() => Event::Reload,
            };
            let message = match event {
                Event::Inbound(Some(message)) => message,
                Event::Inbound(None) => return Ok(Ending::Dropped),
                Event::Idle => return Ok(Ending::Idle),
                Event::Reload => return Ok(Ending::Reload),
                Event::Ping => {
                    write(&mut stream, &new_heartbeat_ping(), "heartbeat").await?;
                    continue;
                }
            };
            last_inbound = Instant::now();
            // Only transport failures reach here; per-message problems arrive as
            // `Undecryptable` and must not drop the connection.
            match message
                .map_err(|error| PushError::Fcm("connection", dispatch::bounded(&error)))?
            {
                Message::HeartbeatPing => {
                    self.metrics.heartbeat(unix_timestamp());
                    write(&mut stream, &new_heartbeat_ack(), "heartbeat").await?;
                }
                Message::HeartbeatAck => self.metrics.heartbeat(unix_timestamp()),
                Message::Data(message) => {
                    self.handle_message(devices, &message.body).await;
                    self.acknowledge(state, message.persistent_id).await?;
                }
                Message::Undecryptable {
                    persistent_id,
                    error,
                } => {
                    self.handle_undecryptable(&error);
                    // Acknowledge it so FCM stops redelivering the same stanza.
                    self.acknowledge(state, persistent_id).await?;
                }
                Message::Other(_, _) => self.metrics.ignored(),
            }
        }
    }

    async fn acknowledge(
        &self,
        state: &mut RingPushState,
        persistent_id: Option<String>,
    ) -> Result<(), PushError> {
        if let Some(id) = persistent_id {
            state.remember(id);
            persist::persist(&self.store, state).await?;
        }
        Ok(())
    }
}

async fn write<T: tokio::io::AsyncWrite + Unpin>(
    stream: &mut MessageStream<T>,
    bytes: &[u8],
    stage: &'static str,
) -> Result<(), PushError> {
    stream
        .write_all(bytes)
        .await
        .map_err(|error| PushError::Fcm(stage, dispatch::bounded(&error)))
}
