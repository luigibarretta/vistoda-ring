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
use tokio::sync::OnceCell;

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
    events: Arc<RingPushEvents>,
    metrics: Arc<RingPushMetrics>,
    provider_device_id: OnceCell<String>,
    started: AtomicBool,
}

impl RingPushService {
    pub fn new(path: PathBuf) -> Self {
        Self {
            store: Arc::new(RingPushStore::new(path)),
            events: Arc::new(RingPushEvents::default()),
            metrics: Arc::new(RingPushMetrics::default()),
            provider_device_id: OnceCell::new(),
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

    pub fn metrics(&self) -> String {
        self.metrics.render()
    }

    pub fn events(&self) -> Arc<RingPushEvents> {
        Arc::clone(&self.events)
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
        let device_id = self
            .provider_device_id
            .get_or_try_init(|| async {
                let client = provider.client().await?;
                client
                    .register_push_token(&state.registration.fcm_token)
                    .await?;
                let device_id = client.subscribe_push_events().await?;
                self.metrics.registered();
                Ok::<String, PushError>(device_id)
            })
            .await?
            .clone();
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
        while let Some(message) = stream.next().await {
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

    async fn handle_message(&self, device_id: &str, body: &[u8]) {
        let Some(event) = parse_push_event(body) else {
            self.metrics.ignored();
            return;
        };
        if event.device_id != device_id {
            self.metrics.ignored();
            return;
        }
        let occurred_at = event.occurred_at.unwrap_or_else(unix_timestamp);
        self.events.publish(event.event_type, occurred_at).await;
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

fn unix_timestamp() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}
