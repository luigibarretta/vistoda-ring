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

#[tokio::test]
async fn a_refused_app_record_is_remembered_instead_of_reprobed() {
    let state = Arc::new(MockState::default());
    let harness = test_client(Arc::clone(&state)).await;
    let token = "l".repeat(40);
    harness
        .client
        .register_push_token(&token)
        .await
        .unwrap_or_else(|error| panic!("first registration failed: {error}"));
    assert_eq!(state.push_attempts.load(Ordering::SeqCst), 2);
    assert!(harness.client.push_record_legacy().await);
    harness
        .client
        .register_push_token(&token)
        .await
        .unwrap_or_else(|error| panic!("second registration failed: {error}"));
    // Only the legacy record is sent while the refusal is cached.
    assert_eq!(state.push_attempts.load(Ordering::SeqCst), 3);
    assert_eq!(state.control_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn rate_limiting_is_not_mistaken_for_a_refused_record() {
    let state = Arc::new(MockState::default());
    let harness = test_client(Arc::clone(&state)).await;
    let result = harness.client.register_push_token(&"r".repeat(40)).await;
    assert!(result.is_err());
    assert_eq!(state.push_attempts.load(Ordering::SeqCst), 1);
    assert!(!harness.client.push_record_legacy().await);
}

#[tokio::test]
async fn a_new_token_is_never_preceded_by_the_stale_one() {
    let state = Arc::new(MockState::default());
    let harness = test_client(Arc::clone(&state)).await;
    harness
        .client
        .register_push_token(&"t".repeat(40))
        .await
        .unwrap_or_else(|error| panic!("first registration failed: {error}"));
    assert_eq!(state.push_attempts.load(Ordering::SeqCst), 1);
    // The session expires; the rotation must re-register it without first
    // restoring the outgoing token.
    harness.client.state.lock().await.registered_until = None;
    harness
        .client
        .register_push_token(&"l".repeat(40))
        .await
        .unwrap_or_else(|error| panic!("rotation failed: {error}"));
    assert_eq!(state.session_calls.load(Ordering::SeqCst), 2);
    // App-format refused, then legacy accepted: no PATCH for the old token.
    assert_eq!(state.push_attempts.load(Ordering::SeqCst), 3);
    // A refused token leaves the previous one in place for later restores.
    let refused = harness.client.register_push_token(&"r".repeat(40)).await;
    assert!(refused.is_err());
    let kept = harness.client.state.lock().await.push_token.clone();
    assert_eq!(
        kept.as_deref().map(String::as_str),
        Some("l".repeat(40).as_str())
    );
}
