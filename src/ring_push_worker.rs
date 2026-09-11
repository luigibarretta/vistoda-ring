use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use fcm_push_listener::{Message, MessageStream, new_heartbeat_ack, register};
use futures_util::StreamExt;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;

use crate::{
    error::BridgeError,
    ring_provider::RingProvider,
    ring_push_event::RingPushEvents,
    ring_push_metrics::RingPushMetrics,
    ring_push_payload::parse_push_event,
    ring_push_store::{RingPushState, RingPushStore},
};

const FIREBASE_APP_ID: &str = "1:876313859327:android:e10ec6ddb3c81f39";
const FIREBASE_PROJECT_ID: &str = "ring-17770";
const FIREBASE_API_KEY: &str = "AIzaSyCv-hdFBmmdBBJadNy-TFwB-xN_H5m3Bk8";
const MIN_RETRY: Duration = Duration::from_secs(5);
const MAX_RETRY: Duration = Duration::from_mins(5);

#[derive(Debug, Error)]
enum PushError {
    #[error("Ring push provider operation failed")]
    Provider(#[from] BridgeError),
    #[error("Ring FCM {0} failed")]
    Fcm(&'static str),
}

pub struct RingPushService {
    store: Arc<RingPushStore>,
    events: crate::ring_push_queues::RingPushQueues,
    metrics: Arc<RingPushMetrics>,
    reload: watch::Sender<u64>,
    started: AtomicBool,
}

impl RingPushService {
    pub fn new(path: PathBuf, devices: impl Iterator<Item = Option<u64>>) -> Self {
        Self {
            store: Arc::new(RingPushStore::new(path)),
            events: crate::ring_push_queues::RingPushQueues::new(devices),
            metrics: Arc::new(RingPushMetrics::default()),
            reload: watch::channel(0).0,
            started: AtomicBool::new(false),
        }
    }

    pub fn start(self: &Arc<Self>, provider: Arc<RingProvider>) {
        if self.started.swap(true, Ordering::AcqRel) {
            return;
        }
        let service = Arc::clone(self);
        tokio::spawn(async move { service.run(provider).await });
    }

    pub fn connected(&self) -> bool {
        self.metrics.connected()
    }

    pub fn reload(&self) {
        self.reload
            .send_modify(|value| *value = value.wrapping_add(1));
    }

    pub fn metrics(&self) -> String {
        self.metrics.render()
    }

    pub fn events(&self, device_id: Option<u64>) -> Result<Arc<RingPushEvents>, BridgeError> {
        self.events.get(device_id)
    }

    pub fn include_devices(
        &self,
        devices: impl Iterator<Item = Option<u64>>,
    ) -> Result<(), BridgeError> {
        if self.events.include(devices)? {
            self.reload();
        }
        Ok(())
    }

    async fn run(self: Arc<Self>, provider: Arc<RingProvider>) {
        let mut delay = MIN_RETRY;
        loop {
            match self.run_once(&provider).await {
                Ok(()) => tracing::warn!("Ring push connection ended"),
                Err(error) => tracing::warn!(error_class = %error, "Ring push listener failed"),
            }
            let was_connected = self.metrics.connected();
            self.metrics.set_connected(false);
            self.metrics.failed();
            if was_connected {
                delay = MIN_RETRY;
            }
            tokio::time::sleep(delay).await;
            if !was_connected {
                delay = delay.saturating_mul(2).min(MAX_RETRY);
            }
            self.metrics.reconnected();
        }
    }

    async fn run_once(&self, provider: &RingProvider) -> Result<(), PushError> {
        let http = fcm_client()?;
        let mut state = if let Some(state) = self.load().await? {
            state
        } else {
            let registration = register(
                &http,
                FIREBASE_APP_ID,
                FIREBASE_PROJECT_ID,
                FIREBASE_API_KEY,
                None,
            )
            .await
            .map_err(|_| PushError::Fcm("registration"))?;
            let state = RingPushState {
                registration,
                persistent_ids: Vec::new(),
            };
            self.persist(&state).await?;
            state
        };
        let mut reload = self.reload.subscribe();
        let client = provider.client().await?;
        client
            .register_push_token(&state.registration.fcm_token)
            .await?;
        let mut device_id = Vec::new();
        for id in self.events.ids()? {
            device_id.push(client.scoped(id).subscribe_push_events().await?);
        }
        drop(client);
        self.metrics.registered();
        let checked = state
            .registration
            .gcm
            .checkin(&http)
            .await
            .map_err(|_| PushError::Fcm("check-in"))?;
        if checked.changed(&state.registration.gcm) {
            state.registration.gcm = checked.session();
            self.persist(&state).await?;
        }
        let connection = checked
            .new_connection(state.persistent_ids.clone())
            .await
            .map_err(|_| PushError::Fcm("connection"))?;
        let mut stream = MessageStream::wrap(connection, &state.registration.keys);
        self.metrics.set_connected(true);
        loop {
            let message = tokio::select! {
                message = stream.next() => message,
                _ = reload.changed() => return Ok(()),
            };
            let Some(message) = message else {
                break;
            };
            match message.map_err(|_| PushError::Fcm("message decoding"))? {
                Message::HeartbeatPing => stream
                    .write_all(&new_heartbeat_ack())
                    .await
                    .map_err(|_| PushError::Fcm("heartbeat acknowledgement"))?,
                Message::Data(message) => {
                    self.handle_message(&device_id, &message.body).await;
                    if let Some(id) = message.persistent_id {
                        state.remember(id);
                        self.persist(&state).await?;
                    }
                }
                Message::Other(_, _) => self.metrics.ignored(),
            }
        }
        Ok(())
    }

    async fn handle_message(&self, device_ids: &[String], body: &[u8]) {
        let Some(event) = parse_push_event(body) else {
            self.metrics.ignored();
            return;
        };
        if !device_ids.contains(&event.device_id) {
            self.metrics.ignored();
            return;
        }
        let occurred_at = event.occurred_at.unwrap_or_else(unix_timestamp);
        let id = event.device_id.parse::<u64>().ok();
        if let Ok(queue) = self.events.get(id) {
            queue.publish(event.event_type, occurred_at).await;
        }
        if device_ids.len() == 1
            && let Ok(queue) = self.events.get(None)
        {
            queue.publish(event.event_type, occurred_at).await;
        }
        self.metrics.received(event.event_type, occurred_at);
        tracing::info!(event_type = ?event.event_type, "Ring push event received");
    }

    async fn load(&self) -> Result<Option<RingPushState>, PushError> {
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || store.load())
            .await
            .map_err(|_| PushError::Fcm("state load"))?
            .map_err(Into::into)
    }

    async fn persist(&self, state: &RingPushState) -> Result<(), PushError> {
        let store = Arc::clone(&self.store);
        let state = RingPushState {
            registration: state.registration.clone(),
            persistent_ids: state.persistent_ids.clone(),
        };
        tokio::task::spawn_blocking(move || store.persist(&state))
            .await
            .map_err(|_| PushError::Fcm("state persistence"))?
            .map_err(Into::into)
    }
}

fn fcm_client() -> Result<reqwest::Client, PushError> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| PushError::Fcm("HTTP client setup"))
}

#[cfg(test)]
#[path = "ring_push_multidevice_tests.rs"]
mod multi_tests;

fn unix_timestamp() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}
