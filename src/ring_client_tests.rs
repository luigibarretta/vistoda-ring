use std::sync::{Arc, atomic::Ordering};

#[path = "ring_client_test_support.rs"]
mod support;

use support::{MockState, assert_session_token, test_client};

#[tokio::test]
async fn discovery_rotates_session_and_reuses_cached_auth() {
    let state = Arc::new(MockState::default());
    let harness = test_client(Arc::clone(&state)).await;
    let first = harness
        .client
        .discover_intercoms()
        .await
        .unwrap_or_else(|error| panic!("first discovery failed: {error}"));
    let second = harness
        .client
        .discover_intercoms()
        .await
        .unwrap_or_else(|error| panic!("second discovery failed: {error}"));
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].id(), 42);
    assert_eq!(first[0].description(), "Synthetic Entrance Intercom");
    assert_eq!(second.len(), 1);
    assert_eq!(state.oauth_calls.load(Ordering::SeqCst), 1);
    assert_eq!(state.session_calls.load(Ordering::SeqCst), 1);
    assert_eq!(state.discovery_calls.load(Ordering::SeqCst), 2);
    assert_session_token(&harness.session_path, support::REFRESH_B);
}

#[tokio::test]
async fn discovery_reauthenticates_only_once_after_unauthorized() {
    let state = Arc::new(MockState {
        first_discovery_unauthorized: true,
        ..MockState::default()
    });
    let harness = test_client(Arc::clone(&state)).await;
    let devices = harness
        .client
        .discover_intercoms()
        .await
        .unwrap_or_else(|error| panic!("discovery failed: {error}"));
    assert_eq!(devices.len(), 1);
    assert_eq!(state.oauth_calls.load(Ordering::SeqCst), 2);
    assert_eq!(state.session_calls.load(Ordering::SeqCst), 2);
    assert_eq!(state.discovery_calls.load(Ordering::SeqCst), 2);
    assert_session_token(&harness.session_path, support::REFRESH_C);
}

#[tokio::test]
async fn rejected_refresh_token_is_not_retried() {
    let state = Arc::new(MockState {
        reject_oauth: true,
        ..MockState::default()
    });
    let harness = test_client(Arc::clone(&state)).await;
    assert!(harness.client.discover_intercoms().await.is_err());
    assert_eq!(state.oauth_calls.load(Ordering::SeqCst), 1);
    assert_eq!(state.session_calls.load(Ordering::SeqCst), 0);
    assert_eq!(state.discovery_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn rate_limited_discovery_is_not_retried() {
    let state = Arc::new(MockState {
        rate_limit_discovery: true,
        ..MockState::default()
    });
    let harness = test_client(Arc::clone(&state)).await;
    assert!(harness.client.discover_intercoms().await.is_err());
    assert_eq!(state.oauth_calls.load(Ordering::SeqCst), 1);
    assert_eq!(state.discovery_calls.load(Ordering::SeqCst), 1);
}
