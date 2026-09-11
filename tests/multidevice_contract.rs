//! Exercise archive and session isolation using synthetic local media only.
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
use std::{collections::BTreeMap, sync::Arc};
use tower::ServiceExt;

const TOKEN: &str = "01234567890123456789012345678901";

async fn request(
    app: &axum::Router,
    method: &str,
    path: &str,
    body: Vec<u8>,
) -> (StatusCode, Vec<u8>) {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::CONTENT_TYPE, "audio/webm")
        .body(Body::from(body))
        .unwrap_or_else(|error| panic!("{error}"));
    let response = app
        .clone()
        .oneshot(request)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .to_bytes()
        .to_vec();
    (status, bytes)
}

#[tokio::test]
async fn recordings_cannot_be_read_or_deleted_from_another_intercom() {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let devices: BTreeMap<_, _> = [("front", 42), ("back", 43)]
        .into_iter()
        .map(|(alias, id)| {
            (
                alias.into(),
                DeviceConfig {
                    kind: DeviceKind::RingIntercomAudio,
                    device_id: Some(id),
                },
            )
        })
        .collect();
    let config = BridgeConfig::new("127.0.0.1".into(), 8775, TOKEN.into(), devices)
        .unwrap_or_else(|error| panic!("{error}"))
        .with_recording_dir(directory.path().join("recordings"));
    let app = router(Arc::new(
        Runtime::new(config).unwrap_or_else(|error| panic!("{error}")),
    ));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut media = vec![0_u8; 128];
    media[..4].copy_from_slice(&[0x1a, 0x45, 0xdf, 0xa3]);
    let (status, body) = request(
        &app,
        "POST",
        &format!(
            "/v1/devices/front/recordings?started_at={}&ended_at={now}",
            now - 1
        ),
        media,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let saved: serde_json::Value =
        serde_json::from_slice(&body).unwrap_or_else(|error| panic!("{error}"));
    let id = saved["recording_id"].as_str().unwrap_or_default();
    let (status, _) = request(
        &app,
        "GET",
        &format!("/v1/devices/back/recordings/{id}"),
        vec![],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = request(
        &app,
        "DELETE",
        &format!("/v1/devices/back/recordings/{id}"),
        vec![],
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = request(
        &app,
        "GET",
        &format!("/v1/devices/front/recordings/{id}"),
        vec![],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = request(&app, "GET", "/v1/devices/back/recordings", vec![]).await;
    let inventory: serde_json::Value =
        serde_json::from_slice(&body).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(inventory["recordings"], serde_json::json!([]));
}
