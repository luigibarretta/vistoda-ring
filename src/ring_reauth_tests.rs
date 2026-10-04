//! A revoked refresh token is a distinct, non-hammering re-enrollment signal;
//! transient OAuth failures keep their previous classification.
// Harnesses own the mock server and credential directory for the whole test.
#![allow(clippy::significant_drop_tightening)]
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::Ordering},
};

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

use crate::{
    BridgeConfig, BridgeError, Runtime,
    model::{DeviceConfig, DeviceKind},
    ring_client::tests::support::{MockState, test_client},
    router,
};

const TOKEN: &str = "01234567890123456789012345678901";

async fn status_response(rejection: u16) -> (StatusCode, serde_json::Value, Arc<MockState>) {
    let state = Arc::new(MockState {
        oauth_rejection: rejection,
        ..MockState::default()
    });
    let harness = test_client(Arc::clone(&state)).await;
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let devices = BTreeMap::from([(
        "entrance".into(),
        DeviceConfig {
            kind: DeviceKind::RingIntercomAudio,
            device_id: Some(42),
        },
    )]);
    let config = BridgeConfig::new("127.0.0.1".into(), 8775, TOKEN.into(), devices)
        .unwrap_or_else(|error| panic!("{error}"))
        .with_session_file(directory.path().join("session"))
        .with_recording_dir(directory.path().join("recordings"));
    let runtime = Arc::new(Runtime::new(config).unwrap_or_else(|error| panic!("{error}")));
    runtime.provider.seed_client(harness.client.clone()).await;
    let response = router(runtime)
        .oneshot(
            Request::get("/v1/devices/entrance/status")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .to_bytes();
    let json = serde_json::from_slice(&body).unwrap_or_else(|error| panic!("{error}"));
    (status, json, state)
}

#[tokio::test]
async fn invalid_grant_is_reported_as_reauth_required_over_http() {
    for rejection in [400, 401] {
        let (status, body, state) = status_response(rejection).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body, serde_json::json!({"error": "reauth_required"}));
        assert_eq!(state.oauth_calls.load(Ordering::SeqCst), 1);
        assert_eq!(state.discovery_calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn transient_oauth_failure_keeps_the_internal_error_contract() {
    let (status, body, _) = status_response(500).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body, serde_json::json!({"error": "internal"}));
}

#[tokio::test]
async fn revoked_token_is_not_presented_again_but_transient_failures_retry() {
    let state = Arc::new(MockState {
        oauth_rejection: 400,
        ..MockState::default()
    });
    let harness = test_client(Arc::clone(&state)).await;
    for _ in 0..3 {
        assert!(matches!(
            harness.client.discover_intercoms().await,
            Err(BridgeError::ReauthRequired)
        ));
    }
    assert_eq!(state.oauth_calls.load(Ordering::SeqCst), 1);
    let state = Arc::new(MockState {
        oauth_rejection: 503,
        ..MockState::default()
    });
    let harness = test_client(Arc::clone(&state)).await;
    for _ in 0..2 {
        assert!(matches!(
            harness.client.discover_intercoms().await,
            Err(BridgeError::VendorRejected { status: 503, .. })
        ));
    }
    assert_eq!(state.oauth_calls.load(Ordering::SeqCst), 2);
}
