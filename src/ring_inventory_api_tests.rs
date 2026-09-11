// The harness keeps its server and credential directory alive for all HTTP assertions.
#![allow(clippy::significant_drop_tightening)]
use crate::{
    BridgeConfig, Runtime,
    model::{DeviceConfig, DeviceKind},
    ring_client::tests::support::{MockState, test_client},
    router,
};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::Ordering},
};
use tower::ServiceExt;

const TOKEN: &str = "01234567890123456789012345678901";

fn config(directory: &std::path::Path) -> BridgeConfig {
    let devices = BTreeMap::from([(
        "entrance".into(),
        DeviceConfig {
            kind: DeviceKind::RingIntercomAudio,
            device_id: Some(42),
        },
    )]);
    BridgeConfig::new("127.0.0.1".into(), 8775, TOKEN.into(), devices)
        .unwrap_or_else(|error| panic!("{error}"))
        .with_session_file(directory.join("missing-private-session"))
        .with_recording_dir(directory.join("recordings"))
}

async fn request(app: &Router, method: &str, token: Option<&str>) -> axum::response::Response {
    let mut request = Request::builder().method(method).uri("/v1/intercoms");
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    app.clone()
        .oneshot(
            request
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"))
}

async fn json(response: axum::response::Response) -> serde_json::Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .to_bytes();
    serde_json::from_slice(&bytes).unwrap_or_else(|error| panic!("{error}"))
}

#[tokio::test]
async fn inventory_is_authenticated_read_only_and_lists_all_account_intercoms() {
    let state = Arc::new(MockState {
        additional_intercoms: 1,
        ..MockState::default()
    });
    let harness = test_client(Arc::clone(&state)).await;
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let runtime =
        Arc::new(Runtime::new(config(directory.path())).unwrap_or_else(|error| panic!("{error}")));
    runtime.provider.seed_client(harness.client.clone()).await;
    let app = router(runtime);
    for token in [None, Some("wrong-token")] {
        assert_eq!(
            request(&app, "GET", token).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(state.discovery_calls.load(Ordering::SeqCst), 0);
    let response = request(&app, "GET", Some(TOKEN)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        json(response).await,
        serde_json::json!({"intercoms":[
            {"alias":"entrance","device_id":"42","name":"Synthetic Entrance Intercom","location_name":"Home"},
            {"alias":"intercom-43","device_id":"43","name":"Synthetic Other","location_name":"Home"}
        ]})
    );
    assert_eq!(
        request(&app, "POST", Some(TOKEN)).await.status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    assert_eq!(state.discovery_calls.load(Ordering::SeqCst), 1);
    assert_eq!(state.control_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn missing_enrollment_returns_only_a_stable_unavailable_error() {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let runtime = Runtime::new(config(directory.path())).unwrap_or_else(|error| panic!("{error}"));
    let response = request(&router(Arc::new(runtime)), "GET", Some(TOKEN)).await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(
        json(response).await,
        serde_json::json!({"error":"upstream_unavailable"})
    );
}

#[tokio::test]
async fn missing_location_metadata_keeps_intercom_selection_available() {
    let state = Arc::new(MockState {
        location_status: 502,
        ..MockState::default()
    });
    let harness = test_client(state).await;
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let runtime =
        Arc::new(Runtime::new(config(directory.path())).unwrap_or_else(|error| panic!("{error}")));
    runtime.provider.seed_client(harness.client.clone()).await;
    let response = request(&router(runtime), "GET", Some(TOKEN)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json(response).await,
        serde_json::json!({"intercoms":[
            {"alias":"entrance","device_id":"42","name":"Synthetic Entrance Intercom","location_name":null}
        ]})
    );
}

#[test]
fn openapi_documents_inventory_authorization_and_response_bounds() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let spec = std::fs::read_to_string(root.join("openapi.yaml")).unwrap_or_default();
    let path =
        std::fs::read_to_string(root.join("docs/openapi/inventory-paths.yaml")).unwrap_or_default();
    assert!(spec.contains("/v1/intercoms:"));
    assert!(spec.contains("./docs/openapi/inventory-paths.yaml#/Intercoms"));
    assert!(path.contains("security: [{bearerAuth: []}]"));
    assert!(path.contains("maxItems: 32"));
    assert!(path.contains("required: [alias, device_id, name, location_name]"));
    assert!(path.contains("additionalProperties: false"));
}
