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
use serde_json::Value;
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
        .unwrap_or_else(|error| panic!("test configuration failed: {error}"));
    router(Arc::new(Runtime::new(config).unwrap_or_else(|error| {
        panic!("runtime setup failed: {error}")
    })))
}

async fn json(response: axum::response::Response) -> Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .unwrap_or_else(|error| panic!("body collection failed: {error}"))
        .to_bytes();
    serde_json::from_slice(&bytes).unwrap_or_else(|error| panic!("response JSON failed: {error}"))
}

#[tokio::test]
async fn health_is_public_and_honest_about_research_state() {
    let response = app()
        .oneshot(
            Request::get("/healthz")
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("request failed: {error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("router failed: {error}"));
    assert_eq!(response.status(), StatusCode::OK);
    let body = json(response).await;
    assert_eq!(body["status"], "ok");
    assert_eq!(body["phase"], "protocol_research");
}

#[tokio::test]
async fn device_inventory_requires_authentication() {
    let response = app()
        .oneshot(
            Request::get("/v1/devices")
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("request failed: {error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("router failed: {error}"));
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn unverified_media_capabilities_fail_closed() {
    let request = Request::get("/v1/devices/entrance/capabilities")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap_or_else(|error| panic!("request failed: {error}"));
    let response = app()
        .oneshot(request)
        .await
        .unwrap_or_else(|error| panic!("router failed: {error}"));
    assert_eq!(response.status(), StatusCode::OK);
    let body = json(response).await;
    assert_eq!(body["phase"], "protocol_research");
    assert_eq!(body["available"], serde_json::json!([]));
}

#[tokio::test]
async fn unknown_device_is_not_disclosed() {
    let request = Request::get("/v1/devices/unknown/capabilities")
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
async fn enrollment_requires_bridge_authentication() {
    let request = Request::post("/v1/enrollments")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"email":"owner@example.com","password":"secret"}"#,
        ))
        .unwrap_or_else(|error| panic!("request failed: {error}"));
    let response = app()
        .oneshot(request)
        .await
        .unwrap_or_else(|error| panic!("router failed: {error}"));
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn enrollment_cancellation_is_idempotent() {
    let request = Request::delete("/v1/enrollments/not-a-valid-id")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap_or_else(|error| panic!("request failed: {error}"));
    let response = app()
        .oneshot(request)
        .await
        .unwrap_or_else(|error| panic!("router failed: {error}"));
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}
