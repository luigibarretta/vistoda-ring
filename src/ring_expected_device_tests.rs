// Async fixture and request targets deliberately outlive all HTTP/media assertions.
#![allow(clippy::significant_drop_tightening)]
use crate::{
    BridgeConfig, BridgeError, Runtime,
    model::{DeviceConfig, DeviceKind},
    ring_client::tests::support::{MockState, test_client},
    ring_inventory::{IntercomInventory, IntercomSummary},
    ring_push_event::RingPushEventKind,
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::Ordering},
};
use tower::ServiceExt;

const TOKEN: &str = "01234567890123456789012345678901";

fn runtime(directory: &std::path::Path) -> Arc<Runtime> {
    let devices = BTreeMap::from([(
        "legacy".into(),
        DeviceConfig {
            kind: DeviceKind::RingIntercomAudio,
            device_id: None,
        },
    )]);
    let config = BridgeConfig::new("127.0.0.1".into(), 8775, TOKEN.into(), devices)
        .unwrap_or_else(|error| panic!("{error}"))
        .with_session_file(directory.join("session"))
        .with_recording_dir(directory.join("recordings"));
    Arc::new(Runtime::new(config).unwrap_or_else(|error| panic!("{error}")))
}

async fn request(
    runtime: &Arc<Runtime>,
    method: &str,
    uri: &str,
    body: &str,
) -> axum::response::Response {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("Authorization", format!("Bearer {TOKEN}"))
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_owned()))
        .unwrap_or_else(|error| panic!("{error}"));
    crate::router(Arc::clone(runtime))
        .oneshot(request)
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
async fn legacy_alias_missing_or_wrong_pins_never_grant_audio_or_mutate_another_intercom() {
    let state = Arc::new(MockState::default());
    let harness = test_client(Arc::clone(&state)).await;
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let runtime = runtime(directory.path());
    runtime.provider.seed_client(harness.client.clone()).await;
    let offer = "v=0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 0\r\na=sendrecv\r\n";
    for expected in [None, Some("43")] {
        let audio =
            serde_json::json!({"offer_sdp":offer,"mode":"listen","expected_device_id":expected})
                .to_string();
        let volume = serde_json::json!({"mic_volume":8,"expected_device_id":expected}).to_string();
        let unlock = serde_json::json!({"expected_device_id":expected}).to_string();
        for (method, suffix, body) in [
            ("POST", "audio/sessions", audio),
            ("PATCH", "settings", volume),
            ("POST", "unlock", unlock),
        ] {
            let status = request(
                &runtime,
                method,
                &format!("/v1/devices/legacy/{suffix}"),
                &body,
            )
            .await
            .status();
            assert!(
                matches!(status, StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND),
                "{suffix}"
            );
        }
        let query = expected.map_or_else(String::new, |id| format!("?expected_device_id={id}"));
        assert_eq!(
            request(
                &runtime,
                "GET",
                &format!("/v1/devices/legacy/history{query}"),
                ""
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(state.control_calls.load(Ordering::SeqCst), 0);
    assert_eq!(state.ticket_calls.load(Ordering::SeqCst), 0);
    // The same check is used by the relay worker before creating a media peer.
    assert!(matches!(
        harness.client.prepare_audio_call_expected(Some("43")).await,
        Err(BridgeError::InvalidRequest(_))
    ));
    let grant = harness
        .client
        .prepare_audio_call_expected(Some("42"))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(grant.device_id, 42);
    assert_eq!(state.ticket_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn physical_event_queues_and_restarted_generations_cannot_cross_or_lose_first_events() {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
    let runtime = runtime(directory.path());
    let mut inventory = IntercomInventory {
        intercoms: [42, 43]
            .map(|id| IntercomSummary {
                alias: None,
                device_id: id.to_string(),
                name: "Intercom".into(),
                location_name: None,
            })
            .into(),
    };
    runtime
        .provision_intercoms(&mut inventory)
        .unwrap_or_else(|error| panic!("{error}"));
    for id in [42, 43] {
        runtime
            .push
            .events(Some(id))
            .unwrap_or_else(|error| panic!("{error}"))
            .publish(
                RingPushEventKind::Ding,
                i64::try_from(id).unwrap_or_default(),
            )
            .await;
    }
    let old_generation = uuid::Uuid::new_v4();
    let path = format!(
        "/v1/devices/legacy/events?expected_device_id=43&after=900&generation={old_generation}"
    );
    let response = request(&runtime, "GET", &path, "").await;
    assert_eq!(response.status(), StatusCode::OK);
    let first = json(response).await;
    assert_eq!(first["device_id"], "43");
    assert_eq!(first["cursor_reset"], true);
    assert_eq!(first["next_sequence"], 1);
    assert_eq!(first["events"][0]["occurred_at"], 43);
    assert_eq!(first["events"].as_array().map(Vec::len), Some(1));
    runtime
        .push
        .events(Some(43))
        .unwrap_or_else(|error| panic!("{error}"))
        .publish(RingPushEventKind::IntercomUnlock, 44)
        .await;
    let generation = first["generation"].as_str().unwrap_or_default();
    let next = json(request(&runtime, "GET", &format!("/v1/devices/intercom-43/events?expected_device_id=43&after=1&generation={generation}"), "").await).await;
    assert_eq!(next["cursor_reset"], false);
    assert_eq!(next["next_sequence"], 2);
    assert_eq!(next["events"][0]["occurred_at"], 44);
    for path in [
        "/v1/devices/legacy/events",
        "/v1/devices/intercom-42/events?expected_device_id=43",
        "/v1/devices/intercom-42/audio/relay?expected_device_id=43",
    ] {
        assert_eq!(
            request(&runtime, "GET", path, "").await.status(),
            StatusCode::BAD_REQUEST
        );
    }
}

#[test]
fn lossless_physical_pins_reject_zero_leading_zero_signs_and_overflow() {
    for id in [
        "",
        "0",
        "01",
        "+1",
        "-1",
        " 1",
        "18446744073709551616",
        "<script>",
    ] {
        assert!(super::parse_id(id).is_err(), "{id}");
    }
    assert_eq!(super::parse_id("18446744073709551615").ok(), Some(u64::MAX));
}
