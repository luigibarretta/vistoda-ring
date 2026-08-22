use std::{collections::BTreeMap, sync::Arc};

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use ring_intercom_bridge::{
    BridgeConfig, Runtime,
    model::{DeviceConfig, DeviceKind},
    router,
};
use tower::ServiceExt;

const TOKEN: &str = "01234567890123456789012345678901";

fn app() -> axum::Router {
    let devices = BTreeMap::from([(
        "entrance".into(),
        DeviceConfig {
            kind: DeviceKind::RingIntercomAudio,
        },
    )]);
    let config = BridgeConfig::new("127.0.0.1".into(), 8775, TOKEN.into(), devices)
        .unwrap_or_else(|error| panic!("test config failed: {error}"));
    router(Arc::new(
        Runtime::new(config).unwrap_or_else(|error| panic!("runtime failed: {error}")),
    ))
}

#[tokio::test]
async fn native_status_and_controls_require_auth_before_provider_access() {
    for (method, path, body) in [
        ("GET", "/v1/devices/entrance/status", ""),
        ("POST", "/v1/devices/entrance/unlock", ""),
        (
            "PATCH",
            "/v1/devices/entrance/settings",
            r#"{"mic_volume":5}"#,
        ),
    ] {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap_or_else(|error| panic!("request failed: {error}"));
        let response = app()
            .oneshot(request)
            .await
            .unwrap_or_else(|error| panic!("router failed: {error}"));
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn native_status_does_not_disclose_unknown_aliases() {
    let request = Request::get("/v1/devices/unknown/status")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap_or_else(|error| panic!("request failed: {error}"));
    let response = app()
        .oneshot(request)
        .await
        .unwrap_or_else(|error| panic!("router failed: {error}"));
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn prometheus_metrics_are_aggregate_and_have_the_right_content_type() {
    let request = Request::get("/metrics")
        .body(Body::empty())
        .unwrap_or_else(|error| panic!("request failed: {error}"));
    let response = app()
        .oneshot(request)
        .await
        .unwrap_or_else(|error| panic!("router failed: {error}"));
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap_or_default()
            .starts_with("text/plain")
    );
    let body = response
        .into_body()
        .collect()
        .await
        .unwrap_or_else(|error| panic!("metrics body failed: {error}"));
    let text = String::from_utf8_lossy(&body.to_bytes()).into_owned();
    assert!(text.contains("ring_audio_sessions_active 0"));
    assert!(!text.contains("entrance"));
}
