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
    app_with_recordings(std::path::PathBuf::from("/data/recordings"))
}

fn app_with_recordings(recording_dir: std::path::PathBuf) -> axum::Router {
    let devices = BTreeMap::from([(
        "entrance".into(),
        DeviceConfig {
            kind: DeviceKind::RingIntercomAudio,
        },
    )]);
    let config = BridgeConfig::new("127.0.0.1".into(), 8775, TOKEN.into(), devices)
        .unwrap_or_else(|error| panic!("test configuration failed: {error}"))
        .with_recording_dir(recording_dir);
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
async fn health_is_public_and_reports_verified_delivery() {
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
    assert_eq!(body["phase"], "verified");
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
async fn verified_audio_capabilities_are_explicit() {
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
    assert_eq!(body["phase"], "verified");
    assert_eq!(
        body["available"],
        serde_json::json!(["live_audio_receive", "live_audio_transmit", "recordings"])
    );
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

#[tokio::test]
async fn audio_session_requires_auth_before_offer_validation() {
    let request = Request::post("/v1/devices/entrance/audio/sessions")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"offer_sdp":"invalid","mode":"listen"}"#))
        .unwrap_or_else(|error| panic!("request failed: {error}"));
    let response = app()
        .oneshot(request)
        .await
        .unwrap_or_else(|error| panic!("router failed: {error}"));
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn audio_session_rejects_unbounded_media_before_vendor_access() {
    let request = Request::post("/v1/devices/entrance/audio/sessions")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"offer_sdp":"v=0\r\nm=video 9 RTP/AVP 96","mode":"talk"}"#,
        ))
        .unwrap_or_else(|error| panic!("request failed: {error}"));
    let response = app()
        .oneshot(request)
        .await
        .unwrap_or_else(|error| panic!("router failed: {error}"));
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn audio_session_delete_is_idempotent() {
    let request =
        Request::delete("/v1/devices/entrance/audio/sessions/00000000-0000-0000-0000-000000000000")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap_or_else(|error| panic!("request failed: {error}"));
    let response = app()
        .oneshot(request)
        .await
        .unwrap_or_else(|error| panic!("router failed: {error}"));
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn recording_inventory_is_private_and_empty_by_default() {
    let unauthenticated = app()
        .oneshot(
            Request::get("/v1/devices/entrance/recordings")
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("request failed: {error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("router failed: {error}"));
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
    let authenticated = app()
        .oneshot(
            Request::get("/v1/devices/entrance/recordings")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("request failed: {error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("router failed: {error}"));
    assert_eq!(authenticated.status(), StatusCode::OK);
    assert_eq!(
        json(authenticated).await["recordings"],
        serde_json::json!([])
    );
}

#[tokio::test]
async fn recording_delete_is_idempotent() {
    let request =
        Request::delete("/v1/devices/entrance/recordings/00000000-0000-0000-0000-000000000000")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap_or_else(|error| panic!("request failed: {error}"));
    let response = app()
        .oneshot(request)
        .await
        .unwrap_or_else(|error| panic!("router failed: {error}"));
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn local_recording_upload_validates_and_archives_webm() {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let now = i64::try_from(now).unwrap_or(i64::MAX);
    let mut media = vec![0_u8; 2048];
    media[..4].copy_from_slice(&[0x1a, 0x45, 0xdf, 0xa3]);
    let request = Request::post(format!(
        "/v1/devices/entrance/recordings?started_at={}&ended_at={}",
        now - 10,
        now
    ))
    .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
    .header(header::CONTENT_TYPE, "audio/webm;codecs=opus")
    .body(Body::from(media))
    .unwrap_or_else(|error| panic!("request failed: {error}"));
    let response = app_with_recordings(directory.path().join("recordings"))
        .oneshot(request)
        .await
        .unwrap_or_else(|error| panic!("router failed: {error}"));
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = json(response).await;
    assert_eq!(body["media_type"], "audio/webm");
    assert_eq!(body["started_at"], now - 10);
    assert_eq!(body["ended_at"], now);
}
