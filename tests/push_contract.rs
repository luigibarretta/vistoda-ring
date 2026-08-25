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
        .unwrap_or_else(|error| panic!("config: {error}"));
    let runtime = Runtime::new(config).unwrap_or_else(|error| panic!("runtime: {error}"));
    router(Arc::new(runtime))
}

#[tokio::test]
async fn event_cursor_is_private_and_empty_before_new_pushes() {
    let unauthenticated = app()
        .oneshot(
            Request::get("/v1/devices/entrance/events")
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("request: {error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("router: {error}"));
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    let authenticated = app()
        .oneshot(
            Request::get("/v1/devices/entrance/events?wait=0")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("request: {error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("router: {error}"));
    assert_eq!(authenticated.status(), StatusCode::OK);
    let body = authenticated
        .into_body()
        .collect()
        .await
        .unwrap_or_else(|error| panic!("body: {error}"))
        .to_bytes();
    let payload: serde_json::Value =
        serde_json::from_slice(&body).unwrap_or_else(|error| panic!("json: {error}"));
    assert_eq!(payload["events"], serde_json::json!([]));
    assert_eq!(payload["next_sequence"], 0);
    assert_eq!(payload["generation"].as_str().map(str::len), Some(36));
    assert_eq!(payload["connected"], false);
}
