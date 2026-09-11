// Keep the synthetic HTTP server alive until the wakeup worker has published routes.
#![allow(clippy::significant_drop_tightening)]
use crate::{
    BridgeConfig, Runtime,
    model::{DeviceConfig, DeviceKind},
    ring_client::tests::support::{MockState, test_client},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

#[tokio::test]
async fn successful_worker_is_woken_again_after_a_transient_reenrollment_failure() {
    let state = Arc::new(MockState::default());
    let harness = test_client(Arc::clone(&state)).await;
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let devices = BTreeMap::from([(
        "entrance".into(),
        DeviceConfig {
            kind: DeviceKind::RingIntercomAudio,
            device_id: None,
        },
    )]);
    let config = BridgeConfig::new(
        "127.0.0.1".into(),
        8775,
        "01234567890123456789012345678901".into(),
        devices,
    )
    .unwrap_or_else(|error| panic!("{error}"))
    .with_session_file(directory.path().join("session"))
    .with_recording_dir(directory.path().join("recordings"));
    let runtime = Arc::new(Runtime::new(config).unwrap_or_else(|error| panic!("{error}")));
    runtime.provider.seed_client(harness.client.clone()).await;
    runtime.start_discovery();
    wait_route(&runtime).await;
    assert_eq!(state.discovery_calls.load(Ordering::SeqCst), 1);
    // Mirror enrollment's cache invalidation and immediate unsuccessful refresh.
    runtime.provider.reset().await;
    runtime.provider.seed_client(harness.client.clone()).await;
    runtime
        .device_runtimes
        .write()
        .unwrap_or_else(|error| panic!("{error}"))
        .remove("intercom-42");
    state.unavailable_discoveries.store(1, Ordering::SeqCst);
    assert!(runtime.refresh_intercoms().await.is_err());
    assert!(runtime.device("intercom-42").is_err());
    runtime.start_discovery();
    wait_route(&runtime).await;
    assert_eq!(state.discovery_calls.load(Ordering::SeqCst), 3);
    assert_eq!(state.control_calls.load(Ordering::SeqCst), 0);
}

async fn wait_route(runtime: &Runtime) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while runtime.device("intercom-42").is_err() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap_or_else(|error| panic!("{error}"));
}
