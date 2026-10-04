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
    ring_push_store::{RingPushState, RingPushStore},
    ring_push_support::fcm_client,
};

const FIREBASE_APP_ID: &str = "1:876313859327:android:e10ec6ddb3c81f39";
const FIREBASE_PROJECT_ID: &str = "ring-17770";
const FIREBASE_API_KEY: &str = "AIzaSyCv-hdFBmmdBBJadNy-TFwB-xN_H5m3Bk8";
const MIN_RETRY: Duration = Duration::from_secs(5);
const MAX_RETRY: Duration = Duration::from_mins(5);

#[derive(Debug, Error)]
pub enum PushError {
    #[error("Ring push provider operation failed")]
    Provider(#[from] BridgeError),
    #[error("Ring FCM {0} failed: {1}")]
    Fcm(&'static str, String),
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
        let unlocks = (Arc::clone(self), Arc::clone(&provider));
        tokio::spawn(async move { service.run(provider).await });
        tokio::spawn(async move { unlocks.0.watch_unlocks(unlocks.1).await });
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
                Ok(true) => {
                    tracing::warn!("Ring push connection ended; rotating registration");
                    let store = Arc::clone(&self.store);
                    match tokio::task::spawn_blocking(move || store.clear()).await {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => {
                            tracing::warn!(error_class = %error, "Ring push registration rotation failed");
                        }
                        Err(error) => {
                            tracing::warn!(error_class = %error, "Ring push registration rotation task failed");
                        }
                    }
                }
                Ok(false) => {
                    tracing::info!("Ring push device subscription changed; reconnecting");
                }
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

    async fn run_once(&self, provider: &RingProvider) -> Result<bool, PushError> {
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
            .map_err(|error| PushError::Fcm("registration", dispatch::bounded(&error)))?;
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
        let device_id = self.subscribe_devices(&client).await?;
        drop(client);
        self.metrics.registered();
        let checked = state
            .registration
            .gcm
            .checkin(&http)
            .await
            .map_err(|error| PushError::Fcm("check-in", dispatch::bounded(&error)))?;
        if checked.changed(&state.registration.gcm) {
            state.registration.gcm = checked.session();
            self.persist(&state).await?;
        }
        let connection = checked
            .new_connection(state.persistent_ids.clone())
            .await
            .map_err(|error| PushError::Fcm("connection", dispatch::bounded(&error)))?;
        let mut stream = MessageStream::wrap(connection, &state.registration.keys);
        self.metrics.set_connected(true);
        loop {
            let message = tokio::select! {
                message = stream.next() => message,
                _ = reload.changed() => return Ok(false),
            };
            let Some(message) = message else {
                break;
            };
            // Only transport failures reach here; per-message problems arrive as
            // `Undecryptable` and must not drop the connection.
            match message
                .map_err(|error| PushError::Fcm("connection", dispatch::bounded(&error)))?
            {
                Message::HeartbeatPing => {
                    stream
                        .write_all(&new_heartbeat_ack())
                        .await
                        .map_err(|error| {
                            PushError::Fcm("heartbeat acknowledgement", dispatch::bounded(&error))
                        })?;
                }
                Message::Data(message) => {
                    self.handle_message(&device_id, &message.body).await;
                    if let Some(id) = message.persistent_id {
                        state.remember(id);
                        self.persist(&state).await?;
                    }
                }
                Message::Undecryptable {
                    persistent_id,
                    error,
                } => {
                    self.handle_undecryptable(&error);
                    // Acknowledge it so FCM stops redelivering the same stanza.
                    if let Some(id) = persistent_id {
                        state.remember(id);
                        self.persist(&state).await?;
                    }
                }
                Message::Other(_, _) => self.metrics.ignored(),
            }
        }
        Ok(true)
    }

    async fn load(&self) -> Result<Option<RingPushState>, PushError> {
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || store.load())
            .await
            .map_err(|error| PushError::Fcm("state load", dispatch::bounded(&error)))?
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
            .map_err(|error| PushError::Fcm("state persistence", dispatch::bounded(&error)))?
            .map_err(Into::into)
    }
}

#[path = "ring_push_dispatch.rs"]
mod dispatch;
#[path = "ring_unlock_watch.rs"]
mod unlock_watch;

#[cfg(test)]
#[path = "ring_push_multidevice_tests.rs"]
mod multi_tests;
