// Runtime and scratch directory intentionally survive all synthetic session requests.
#![allow(clippy::significant_drop_tightening)]
use crate::{
    BridgeConfig, BridgeError, Runtime,
    model::{DeviceConfig, DeviceKind},
    ring_audio_manager::{
        RingAudioSessions,
        tests::{FakeRunner, request as offer},
    },
    ring_inventory::{IntercomInventory, IntercomSummary},
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use std::{collections::BTreeMap, sync::Arc};
use tower::ServiceExt;

const TOKEN: &str = "01234567890123456789012345678901";

async fn call(
    runtime: &Arc<Runtime>,
    method: &str,
    path: &str,
    body: String,
) -> axum::response::Response {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("Authorization", format!("Bearer {TOKEN}"))
        .header("Content-Type", "application/json")
        .body(Body::from(body))
        .unwrap_or_else(|error| panic!("{error}"));
    crate::router(Arc::clone(runtime))
        .oneshot(request)
        .await
        .unwrap_or_else(|error| panic!("{error}"))
}

fn runtime(directory: &std::path::Path) -> Arc<Runtime> {
    let config = BridgeConfig::new(
        "127.0.0.1".into(),
        8775,
        TOKEN.into(),
        BTreeMap::from([(
            "legacy".into(),
            DeviceConfig {
                kind: DeviceKind::RingIntercomAudio,
                device_id: None,
            },
        )]),
    )
    .unwrap_or_else(|error| panic!("{error}"))
    .with_session_file(directory.join("session"))
    .with_recording_dir(directory.join("recordings"));
    let runtime = Arc::new(Runtime::new(config).unwrap_or_else(|error| panic!("{error}")));
    runtime
        .provision_intercoms(&mut IntercomInventory {
            intercoms: [42, 43]
                .map(|id| IntercomSummary {
                    alias: None,
                    device_id: id.to_string(),
                    name: "Intercom".into(),
                    location_name: None,
                })
                .into(),
        })
        .unwrap_or_else(|error| panic!("{error}"));
    {
        let mut devices = runtime
            .device_runtimes
            .write()
            .unwrap_or_else(|error| panic!("{error}"));
        for alias in ["intercom-42", "intercom-43"] {
            let target = devices
                .get_mut(alias)
                .and_then(Arc::get_mut)
                .unwrap_or_else(|| panic!("exclusive fixture target"));
            target.audio = RingAudioSessions::new(Arc::new(FakeRunner));
        }
    }
    runtime
}

#[tokio::test]
async fn legacy_and_auto_aliases_share_physical_audio_gate_and_delete_target() {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let runtime = runtime(directory.path());
    let (legacy, _) = runtime
        .audio_target("legacy", Some("42"))
        .unwrap_or_else(|error| panic!("{error}"));
    let (physical, _) = runtime
        .audio_target("intercom-42", None)
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(Arc::ptr_eq(&legacy, &physical));
    let mut input = offer();
    input.expected_device_id = Some("42".into());
    let body = serde_json::to_string(&input).unwrap_or_default();
    let first = call(
        &runtime,
        "POST",
        "/v1/devices/legacy/audio/sessions",
        body.clone(),
    )
    .await;
    assert_eq!(first.status(), StatusCode::CREATED);
    let bytes = first
        .into_body()
        .collect()
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .to_bytes();
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).unwrap_or_else(|error| panic!("{error}"));
    let session = value["session_id"].as_str().unwrap_or_default();
    assert_eq!(
        call(
            &runtime,
            "POST",
            "/v1/devices/intercom-42/audio/sessions",
            body
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    assert!(matches!(
        physical.audio.reserve_relay("42".into()).await,
        Err(BridgeError::SessionBusy)
    ));
    input.expected_device_id = Some("43".into());
    assert_eq!(
        call(
            &runtime,
            "POST",
            "/v1/devices/intercom-43/audio/sessions",
            serde_json::to_string(&input).unwrap_or_default()
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        call(
            &runtime,
            "DELETE",
            &format!("/v1/devices/legacy/audio/sessions/{session}"),
            String::new()
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &runtime,
            "DELETE",
            &format!("/v1/devices/legacy/audio/sessions/{session}?expected_device_id=42"),
            String::new()
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert!(matches!(
        physical.audio.reserve_relay("42".into()).await,
        Err(BridgeError::RateLimited)
    ));
}
