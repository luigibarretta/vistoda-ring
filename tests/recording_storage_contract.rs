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

#[tokio::test]
async fn inventory_exposes_exact_display_path_without_credentials() {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let app = app(directory.path().join("recordings"));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let now = i64::try_from(now).unwrap_or(i64::MAX);
    let mut media = vec![0_u8; 128];
    media[..4].copy_from_slice(&[0x1a, 0x45, 0xdf, 0xa3]);
    let upload = Request::post(format!(
        "/v1/devices/entrance/recordings?started_at={}&ended_at={}",
        now - 1,
        now
    ))
    .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
    .header(header::CONTENT_TYPE, "audio/webm")
    .body(Body::from(media))
    .unwrap_or_else(|error| panic!("request failed: {error}"));
    let response = app
        .clone()
        .oneshot(upload)
        .await
        .unwrap_or_else(|error| panic!("router failed: {error}"));
    assert_eq!(response.status(), StatusCode::CREATED);
    let response = app
        .oneshot(authenticated_get())
        .await
        .unwrap_or_else(|error| panic!("router failed: {error}"));
    let bytes = response
        .into_body()
        .collect()
        .await
        .unwrap_or_else(|error| panic!("body failed: {error}"))
        .to_bytes();
    let payload: serde_json::Value =
        serde_json::from_slice(&bytes).unwrap_or_else(|error| panic!("JSON failed: {error}"));
    assert_eq!(payload["storage"]["kind"], "private");
    assert_eq!(payload["storage"]["directory"], "/data/recordings");
    assert_eq!(payload["storage"]["user_visible"], false);
    let path = payload["recordings"][0]["storage_path"]
        .as_str()
        .unwrap_or_default();
    assert!(path.starts_with("/data/recordings/"));
    assert_eq!(
        std::path::Path::new(path)
            .extension()
            .and_then(|value| value.to_str()),
        Some("webm")
    );
    assert!(!path.contains(TOKEN));
}

fn app(recording_dir: std::path::PathBuf) -> axum::Router {
    let devices = BTreeMap::from([(
        "entrance".into(),
        DeviceConfig {
            kind: DeviceKind::RingIntercomAudio,
        },
    )]);
    let config = BridgeConfig::new("127.0.0.1".into(), 8775, TOKEN.into(), devices)
        .unwrap_or_else(|error| panic!("test config failed: {error}"))
        .with_recording_dir(recording_dir);
    router(Arc::new(Runtime::new(config).unwrap_or_else(|error| {
        panic!("runtime setup failed: {error}")
    })))
}

fn authenticated_get() -> Request<Body> {
    Request::get("/v1/devices/entrance/recordings")
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap_or_else(|error| panic!("request failed: {error}"))
}
