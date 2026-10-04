//! Registration state I/O off the async runtime; failures carry bounded text.

use std::sync::Arc;

use super::{PushError, RingPushService, dispatch};
use crate::ring_push_store::RingPushState;

impl RingPushService {
    pub(super) async fn load(&self) -> Result<Option<RingPushState>, PushError> {
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || store.load())
            .await
            .map_err(|error| PushError::Fcm("state load", dispatch::bounded(&error)))?
            .map_err(Into::into)
    }

    pub(super) async fn persist(&self, state: &RingPushState) -> Result<(), PushError> {
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
