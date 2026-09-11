// The local HTTP harness and scratch storage must outlive their asynchronous requests.
#![allow(clippy::significant_drop_tightening)]
use super::*;
use crate::{
    BridgeConfig,
    model::DeviceConfig,
    ring_client::tests::support::{MockState, test_client},
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use std::sync::{Arc, atomic::Ordering};
use tower::ServiceExt;

const TOKEN: &str = "01234567890123456789012345678901";

fn runtime(directory: &std::path::Path, devices: BTreeMap<String, DeviceConfig>) -> Runtime {
    let config = BridgeConfig::new("127.0.0.1".into(), 8775, TOKEN.into(), devices)
        .unwrap_or_else(|error| panic!("{error}"))
        .with_session_file(directory.join("session"))
        .with_recording_dir(directory.join("recordings"));
    Runtime::new(config).unwrap_or_else(|error| panic!("{error}"))
}

fn bootstrap() -> BTreeMap<String, DeviceConfig> {
    BTreeMap::from([(
        "entrance".into(),
        DeviceConfig {
            kind: DeviceKind::RingIntercomAudio,
            device_id: None,
        },
    )])
}

fn inventory(ids: &[u64]) -> IntercomInventory {
    IntercomInventory {
        intercoms: ids
            .iter()
            .map(|id| crate::ring_inventory::IntercomSummary {
                alias: None,
                device_id: id.to_string(),
                name: "User-supplied name".into(),
                location_name: None,
            })
            .collect(),
    }
}

#[test]
fn bootstrap_discovery_is_stable_scoped_and_reuses_active_runtime() {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let runtime = runtime(directory.path(), bootstrap());
    let mut first = inventory(&[42, 43]);
    runtime
        .provision_intercoms(&mut first)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(first.intercoms[0].alias.as_deref(), Some("intercom-42"));
    assert_eq!(first.intercoms[1].alias.as_deref(), Some("intercom-43"));
    let front = runtime
        .device("intercom-42")
        .unwrap_or_else(|error| panic!("{error}"));
    let back = runtime
        .device("intercom-43")
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(front.device_id, Some(42));
    assert_eq!(back.device_id, Some(43));
    assert_ne!(front.recording_display_dir, back.recording_display_dir);
    let mut renamed = inventory(&[43, 42]);
    renamed.intercoms[1].name = "A completely different user name".into();
    runtime
        .provision_intercoms(&mut renamed)
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(Arc::ptr_eq(
        &front,
        &runtime
            .device("intercom-42")
            .unwrap_or_else(|error| panic!("{error}"))
    ));
    runtime
        .provision_intercoms(&mut inventory(&[42]))
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(runtime.device("intercom-43").is_err());
    assert!(runtime.push.events(Some(43)).is_err());
}

#[test]
fn collision_and_oversized_inventory_do_not_publish_partial_routes() {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let config = BTreeMap::from([(
        "intercom-43".into(),
        DeviceConfig {
            kind: DeviceKind::RingIntercomAudio,
            device_id: Some(42),
        },
    )]);
    let runtime = runtime(directory.path(), config);
    assert!(
        runtime
            .provision_intercoms(&mut inventory(&[42, 43]))
            .is_err()
    );
    assert_eq!(
        runtime
            .device("intercom-43")
            .unwrap_or_else(|error| panic!("{error}"))
            .device_id,
        Some(42)
    );
    assert!(runtime.device("intercom-42").is_err());
    assert!(
        runtime
            .provision_intercoms(&mut inventory(&(1..=513).collect::<Vec<_>>()))
            .is_err()
    );
    assert!(
        runtime
            .provision_intercoms(&mut inventory(&[42, 42]))
            .is_err()
    );
}

#[tokio::test]
async fn transient_startup_discovery_recovers_without_enrollment_or_request_retries() {
    let state = Arc::new(MockState {
        unavailable_discoveries: 1.into(),
        additional_intercoms: 1,
        ..MockState::default()
    });
    let harness = test_client(Arc::clone(&state)).await;
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let runtime = runtime(directory.path(), bootstrap());
    runtime.provider.seed_client(harness.client.clone()).await;
    assert!(runtime.device("intercom-43").is_err());
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        runtime.restore_intercoms(std::time::Duration::from_millis(1)),
    )
    .await
    .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(state.discovery_calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        runtime
            .device("intercom-43")
            .unwrap_or_else(|error| panic!("{error}"))
            .device_id,
        Some(43)
    );
    for _ in 0..20 {
        assert!(runtime.device("intercom-42").is_ok());
    }
    assert_eq!(state.discovery_calls.load(Ordering::SeqCst), 2);
    assert_eq!(state.control_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn discovered_alias_routes_enforce_expected_device_without_wrong_door_rpc() {
    let state = Arc::new(MockState {
        additional_intercoms: 1,
        ..MockState::default()
    });
    let harness = test_client(Arc::clone(&state)).await;
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let runtime = Arc::new(runtime(directory.path(), bootstrap()));
    runtime.provider.seed_client(harness.client.clone()).await;
    let inventory = runtime
        .refresh_intercoms()
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(inventory.intercoms.len(), 2);
    let app = crate::router(runtime);
    for alias in ["intercom-42", "intercom-43"] {
        for suffix in ["status", "history", "recordings", "events", "capabilities"] {
            let request = Request::builder()
                .uri(format!("/v1/devices/{alias}/{suffix}"))
                .header("Authorization", format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}"));
            assert_eq!(
                app.clone()
                    .oneshot(request)
                    .await
                    .unwrap_or_else(|error| panic!("{error}"))
                    .status(),
                StatusCode::OK,
                "{alias}/{suffix}"
            );
        }
    }
    let request = Request::builder()
        .method("POST")
        .uri("/v1/devices/intercom-43/unlock")
        .header("Authorization", format!("Bearer {TOKEN}"))
        .body(Body::from(r#"{"expected_device_id":"42"}"#))
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(
        !app.oneshot(request)
            .await
            .unwrap_or_else(|error| panic!("{error}"))
            .status()
            .is_success()
    );
    assert_eq!(state.control_calls.load(Ordering::SeqCst), 0);
}
