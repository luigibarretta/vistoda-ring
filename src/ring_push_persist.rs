//! Registration state I/O off the async runtime; failures carry bounded text.

use std::sync::Arc;

use super::{PushError, dispatch};
use crate::ring_push_store::{RingPushState, RingPushStore, StoredPush};

pub(super) async fn load(store: &Arc<RingPushStore>) -> Result<StoredPush, PushError> {
    let store = Arc::clone(store);
    tokio::task::spawn_blocking(move || store.load())
        .await
        .map_err(|error| PushError::Fcm("state load", dispatch::bounded(&error)))?
        .map_err(Into::into)
}

pub(super) async fn persist(
    store: &Arc<RingPushStore>,
    state: &RingPushState,
) -> Result<(), PushError> {
    let store = Arc::clone(store);
    let state = RingPushState {
        registration: state.registration.clone(),
        persistent_ids: state.persistent_ids.clone(),
    };
    tokio::task::spawn_blocking(move || store.persist(&state))
        .await
        .map_err(|error| PushError::Fcm("state persistence", dispatch::bounded(&error)))?
        .map_err(Into::into)
}
