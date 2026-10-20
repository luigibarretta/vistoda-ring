//! Make-before-break push registration. The stored registration is reused
//! across reconnects. A rotation registers new credentials and hands them to
//! Ring before the file is atomically replaced; if any step fails the old
//! registration stays on disk and is used for this attempt.

use std::sync::Arc;

use async_trait::async_trait;
use fcm_push_listener::Registration;

use super::{PushError, persist};
use crate::{
    ring_push_metrics::RingPushMetrics,
    ring_push_store::{RingPushState, RingPushStore, StoredPush},
};

/// The network side of a registration; faked in tests.
#[async_trait]
pub(super) trait PushRegistrar: Send + Sync {
    /// Create a new FCM registration (GCM check-in, token, FCM register).
    async fn register(&self) -> Result<Registration, PushError>;
    /// Hand the token to Ring and subscribe the Intercoms; returns their IDs.
    async fn hand_off(&self, token: &str) -> Result<Vec<String>, PushError>;
}

pub(super) struct Prepared {
    pub state: RingPushState,
    pub devices: Vec<String>,
}

/// Returns the registration to connect with. `rotate` is cleared once a
/// rotation was attempted, whether or not it succeeded.
pub(super) async fn prepare(
    store: &Arc<RingPushStore>,
    registrar: &dyn PushRegistrar,
    metrics: &RingPushMetrics,
    rotate: &mut bool,
) -> Result<Prepared, PushError> {
    let current = match persist::load(store).await? {
        StoredPush::Present(state) => state,
        stored => {
            if matches!(stored, StoredPush::Quarantined) {
                metrics.state_reset();
            }
            // Nothing to break: persist first so a failed hand-off reuses it.
            let state = RingPushState::new(registrar.register().await?);
            persist::persist(store, &state).await?;
            let devices = registrar.hand_off(&state.registration.fcm_token).await?;
            *rotate = false;
            return Ok(Prepared { state, devices });
        }
    };
    if std::mem::take(rotate) {
        match rotated(store, registrar).await {
            Ok(prepared) => {
                metrics.rotated();
                tracing::info!("Ring push registration rotated");
                return Ok(prepared);
            }
            Err(error) => {
                metrics.failed("rotation");
                tracing::warn!(
                    error_class = %error,
                    "Ring push registration rotation failed; keeping the current registration"
                );
            }
        }
    }
    let devices = registrar.hand_off(&current.registration.fcm_token).await?;
    Ok(Prepared {
        state: current,
        devices,
    })
}

async fn rotated(
    store: &Arc<RingPushStore>,
    registrar: &dyn PushRegistrar,
) -> Result<Prepared, PushError> {
    let state = RingPushState::new(registrar.register().await?);
    let devices = registrar.hand_off(&state.registration.fcm_token).await?;
    // The atomic rename is the only point at which the old file goes away.
    persist::persist(store, &state).await?;
    Ok(Prepared { state, devices })
}
