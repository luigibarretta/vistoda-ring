//! Push token registration against the mock Ring client API.
// Harnesses own the mock server and credential directory for the whole test.
#![allow(clippy::significant_drop_tightening)]

use std::sync::{Arc, atomic::Ordering};

use super::support::{MockState, test_client};

#[tokio::test]
async fn push_token_is_restored_after_every_session_registration() {
    let state = Arc::new(MockState::default());
    let harness = test_client(Arc::clone(&state)).await;
    harness
        .client
        .register_push_token(&"t".repeat(40))
        .await
        .unwrap_or_else(|error| panic!("push registration failed: {error}"));
    assert_eq!(state.session_calls.load(Ordering::SeqCst), 1);
    assert_eq!(state.control_calls.load(Ordering::SeqCst), 1);
    // A forced re-registration (as after the 12-hour lifetime) re-sends it.
    harness.client.state.lock().await.registered_until = None;
    harness
        .client
        .discover_intercoms()
        .await
        .unwrap_or_else(|error| panic!("discovery failed: {error}"));
    assert_eq!(state.session_calls.load(Ordering::SeqCst), 2);
    assert_eq!(state.control_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_refused_app_record_falls_back_to_the_legacy_record() {
    let state = Arc::new(MockState::default());
    let harness = test_client(Arc::clone(&state)).await;
    harness
        .client
        .register_push_token(&"l".repeat(40))
        .await
        .unwrap_or_else(|error| panic!("legacy fallback failed: {error}"));
    assert_eq!(state.control_calls.load(Ordering::SeqCst), 1);
}
