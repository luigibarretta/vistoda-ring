use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use thiserror::Error;
use tokio::{sync::watch, time::Instant};

use crate::{
    error::BridgeError, ring_provider::RingProvider, ring_push_event::RingPushEvents,
    ring_push_metrics::RingPushMetrics, ring_push_store::RingPushStore,
    ring_push_support::fcm_client,
};
use policy::{Attempt, Ending, ReconnectPolicy};

#[derive(Debug, Error)]
pub enum PushError {
    #[error("Ring push provider operation failed")]
    Provider(#[from] BridgeError),
    #[error("Ring FCM {0} failed: {1}")]
    Fcm(&'static str, String),
    /// Google answered but refused the stored device credentials.
    #[error("Ring FCM {0} was rejected: {1}")]
    Rejected(&'static str, String),
}

impl PushError {
    /// Bounded metric label for the failing stage.
    const fn stage(&self) -> &'static str {
        match self {
            Self::Provider(_) => "provider",
            Self::Fcm(stage, _) | Self::Rejected(stage, _) => stage,
        }
    }
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

    /// The stored registration was unreadable and replaced since start.
    pub fn state_reset(&self) -> bool {
        self.metrics.state_was_reset()
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
        let mut policy = ReconnectPolicy::default();
        let mut rotate = false;
        loop {
            let attempt = self.run_once(&provider, &mut rotate).await;
            self.metrics.set_connected(false);
            let plan = policy.next(attempt, policy::jitter());
            if plan.rotate {
                tracing::warn!(
                    "Ring push connections keep ending quickly; rotating the registration"
                );
                rotate = true;
            }
            tokio::time::sleep(plan.delay).await;
            self.metrics.reconnected();
        }
    }

    async fn run_once(&self, provider: &RingProvider, rotate: &mut bool) -> Attempt {
        let mut connected_at = None;
        let result = self.connect(provider, rotate, &mut connected_at).await;
        let uptime = connected_at.map(|at: Instant| at.elapsed());
        let ending = match result {
            Ok(Ending::Reload) => {
                tracing::info!("Ring push device subscription changed; reconnecting");
                Ending::Reload
            }
            Ok(ending) => {
                tracing::warn!(
                    ?ending,
                    uptime_seconds = uptime.unwrap_or_default().as_secs(),
                    "Ring push connection ended; reconnecting with the stored registration"
                );
                ending
            }
            Err(PushError::Provider(BridgeError::ReauthRequired)) => {
                // Only a re-enrollment recovers; retry rarely and log once.
                self.reauth_state(true);
                Ending::Reauth
            }
            Err(error) => {
                self.metrics.failed(error.stage());
                tracing::warn!(error_class = %error, "Ring push listener failed");
                match error {
                    PushError::Rejected(..) => Ending::Rejected,
                    // A transport error after login is the server dropping us.
                    PushError::Fcm("connection", _) if uptime.is_some() => Ending::Dropped,
                    _ => Ending::Failed,
                }
            }
        };
        Attempt { ending, uptime }
    }

    async fn connect(
        &self,
        provider: &RingProvider,
        rotate: &mut bool,
        connected_at: &mut Option<Instant>,
    ) -> Result<Ending, PushError> {
        let http = fcm_client()?;
        let mut reload = self.reload.subscribe();
        // The client holds the provider lifecycle lock; release it before listening.
        let prepared = {
            let client = provider.client().await?;
            let registrar = session::LiveRegistrar {
                http: &http,
                client: &client,
                service: self,
            };
            rotation::prepare(&self.store, &registrar, &self.metrics, rotate).await?
        };
        let mut state = prepared.state;
        let checked = state
            .registration
            .gcm
            .checkin(&http)
            .await
            .map_err(|error| {
                let text = dispatch::bounded(&error);
                match error {
                    fcm_push_listener::Error::DependencyRejection(..)
                    | fcm_push_listener::Error::DependencyFailure(..) => {
                        PushError::Rejected("check-in", text)
                    }
                    _ => PushError::Fcm("check-in", text),
                }
            })?;
        if checked.changed(&state.registration.gcm) {
            state.registration.gcm = checked.session();
            persist::persist(&self.store, &state).await?;
        }
        self.listen(
            checked,
            &mut state,
            &prepared.devices,
            &mut reload,
            connected_at,
        )
        .await
    }
}

#[path = "ring_ding_watch.rs"]
mod ding_watch;
#[path = "ring_push_dispatch.rs"]
mod dispatch;
#[path = "ring_push_persist.rs"]
mod persist;
#[path = "ring_push_policy.rs"]
mod policy;
#[path = "ring_push_rotation.rs"]
mod rotation;
#[path = "ring_push_session.rs"]
mod session;
#[path = "ring_unlock_watch.rs"]
mod unlock_watch;

#[cfg(test)]
#[path = "ring_push_multidevice_tests.rs"]
mod multi_tests;
#[cfg(test)]
#[path = "ring_push_rotation_tests.rs"]
mod rotation_tests;
