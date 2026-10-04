// Harnesses own the mock server and credential directory for the whole test.
#![allow(clippy::significant_drop_tightening)]
use std::sync::{Arc, atomic::Ordering};

#[path = "ring_client_test_support.rs"]
pub mod support;

use crate::ring_control::VolumeUpdate;
use crate::ring_history::RingHistoryEventType;
use support::{MockState, assert_session_token, test_client};

#[tokio::test]
async fn enrollment_waits_for_old_client_requests_and_resets_every_scoped_cache() {
    let harness = test_client(Arc::new(MockState::default())).await;
    let provider = crate::ring_provider::RingProvider::new(harness.session_path.clone());
    let first = provider.scoped(Some(42));
    let old = first
        .client()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let old_state = Arc::clone(&old.state);
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(5),
            provider.enrollment_guard()
        )
        .await
        .is_err()
    );
    drop(old);
    let guard = provider.enrollment_guard().await;
    provider.reset().await;
    drop(guard);
    let refreshed = first
        .client()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(!Arc::ptr_eq(&old_state, &refreshed.state));
}

#[tokio::test]
async fn two_intercoms_keep_status_history_and_unlock_on_selected_device() {
    let state = Arc::new(MockState {
        additional_intercoms: 1,
        ..MockState::default()
    });
    let harness = test_client(Arc::clone(&state)).await;
    assert!(harness.client.device_status().await.is_err());
    assert!(harness.client.unlock().await.is_err());
    let first = harness.client.scoped(Some(42));
    let second = harness.client.scoped(Some(43));
    assert_eq!(
        first
            .device_status()
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .device_id,
        "42"
    );
    assert_eq!(
        second
            .device_status()
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .device_id,
        "43"
    );
    let history = second
        .history(20, None)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(history.identity.device_id, "43");
    assert_eq!(history.events[0].event_id, "99");
    assert!(second.unlock_expected(Some("42")).await.is_err());
    assert_eq!(state.control_calls.load(Ordering::SeqCst), 0);
    second
        .unlock_expected(Some("43"))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(state.last_control_device.load(Ordering::SeqCst), 43);
    assert!(harness.client.scoped(Some(999)).unlock().await.is_err());
    assert_eq!(state.control_calls.load(Ordering::SeqCst), 1);
    assert_eq!(state.oauth_calls.load(Ordering::SeqCst), 1);
}

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

#[tokio::test]
async fn native_status_includes_battery_volumes_and_activity() {
    let harness = test_client(Arc::new(MockState::default())).await;
    let status = harness
        .client
        .device_status()
        .await
        .unwrap_or_else(|error| panic!("status failed: {error}"));
    assert_eq!(status.battery, Some(73));
    assert!(status.online);
    assert_eq!(status.doorbell_volume, Some(6));
    assert_eq!(status.mic_volume, Some(10));
    assert_eq!(status.voice_volume, Some(9));
    assert_eq!(status.last_activity, Some(1_786_795_500));
}

#[tokio::test]
async fn history_exposes_provider_identity_and_cursor_without_credentials() {
    let harness = test_client(Arc::new(MockState::default())).await;
    let page = harness
        .client
        .history(2, None)
        .await
        .unwrap_or_else(|error| panic!("history failed: {error}"));
    assert_eq!(page.identity.device_name, "Synthetic Entrance Intercom");
    assert_eq!(page.identity.location_name, "Home");
    assert_eq!(page.identity.city.as_deref(), Some("Casoria"));
    assert_eq!(page.events[0].event_type, RingHistoryEventType::Unlock);
    assert_eq!(page.events[1].event_type, RingHistoryEventType::LiveView);
    assert_eq!(page.next_cursor.as_deref(), Some("7323267080901445808"));
}

#[tokio::test]
async fn native_unlock_and_each_volume_use_bounded_vendor_contracts() {
    let state = Arc::new(MockState::default());
    let harness = test_client(Arc::clone(&state)).await;
    harness
        .client
        .unlock()
        .await
        .unwrap_or_else(|error| panic!("unlock failed: {error}"));
    for update in [
        VolumeUpdate {
            expected_device_id: None,
            doorbell_volume: Some(7),
            mic_volume: None,
            voice_volume: None,
        },
        VolumeUpdate {
            expected_device_id: None,
            doorbell_volume: None,
            mic_volume: Some(8),
            voice_volume: None,
        },
        VolumeUpdate {
            expected_device_id: None,
            doorbell_volume: None,
            mic_volume: None,
            voice_volume: Some(7),
        },
    ] {
        harness
            .client
            .update_volume(&update)
            .await
            .unwrap_or_else(|error| panic!("volume failed: {error}"));
    }
    assert_eq!(state.control_calls.load(Ordering::SeqCst), 4);
}
