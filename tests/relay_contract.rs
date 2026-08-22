use std::{collections::BTreeMap, sync::Arc};

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
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
        .unwrap_or_else(|error| panic!("test configuration failed: {error}"));
    router(Arc::new(Runtime::new(config).unwrap_or_else(|error| {
        panic!("runtime setup failed: {error}")
    })))
}

fn upgrade(path: &str, authenticated: bool) -> Request<Body> {
    let mut request = Request::get(path)
        .header(header::CONNECTION, "upgrade")
        .header(header::UPGRADE, "websocket")
        .header("sec-websocket-version", "13")
        .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==");
    if authenticated {
        request = request.header(header::AUTHORIZATION, format!("Bearer {TOKEN}"));
    }
    request
        .body(Body::empty())
        .unwrap_or_else(|error| panic!("request failed: {error}"))
}

#[tokio::test]
async fn relay_authentication_precedes_device_disclosure() {
    let response = app()
        .oneshot(upgrade("/v1/devices/entrance/audio/relay", false))
        .await
        .unwrap_or_else(|error| panic!("router failed: {error}"));
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn relay_rejects_unknown_alias_before_upgrade() {
    let response = app()
        .oneshot(upgrade("/v1/devices/unknown/audio/relay", true))
        .await
        .unwrap_or_else(|error| panic!("router failed: {error}"));
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
