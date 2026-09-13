use super::*;
use crate::{
    BridgeConfig,
    ring_client::tests::support::{MockState, test_client},
};
use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use std::{collections::BTreeMap, sync::atomic::Ordering};
use tower::ServiceExt;

const TOKEN: &str = "01234567890123456789012345678901";
const OFFER: &str = "v=0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 0\r\na=sendrecv\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\na=rtpmap:96 H264/90000\r\na=recvonly\r\n";

fn request(sdp: &str) -> AudioSessionRequest {
    AudioSessionRequest {
        expected_device_id: Some("51".into()),
        offer_sdp: sdp.into(),
        mode: crate::ring_audio::AudioMode::Listen,
        ice_gathering_ms: None,
    }
}

#[test]
fn video_sdp_is_bounded_h264_receive_only_and_cannot_replace_intercom_audio() {
    assert!(crate::ring_video::validate_request(&request(OFFER)).is_ok());
    assert!(crate::ring_audio::validate_offer(OFFER).is_err());
    for invalid in [
        OFFER.replace("a=recvonly", "a=sendrecv"),
        OFFER.replace("H264", "VP8"),
        OFFER.replace("m=video 9", "m=video 0"),
        OFFER.replace("m=audio 9", "m=audio 0"),
        OFFER.replace("a=rtpmap:96", "a=rtpmap:97"),
        OFFER.replace("SAVPF 0", "SAVPF 8"),
        format!("{OFFER}m=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\n"),
        format!("{OFFER}a=inactive\r\n"),
        format!("{OFFER}\0"),
        "x".repeat(65_537),
    ] {
        assert!(crate::ring_video::validate_request(&request(&invalid)).is_err());
    }
    let answer = OFFER.replace("a=recvonly", "a=sendonly");
    assert!(crate::ring_video::validate_answer(&answer).is_ok());
    assert!(crate::ring_video::validate_answer(OFFER).is_err());
}

#[test]
fn discovery_is_lossless_minimal_and_separate_from_intercom_controls() {
    let body = br#"{"doorbots":[{"id":18446744073709551615,"kind":"doorbell_v4","description":"Front","token":"private"}],"stickup_cams":[{"id":52,"kind":"stickup_cam","description":"Garden"}],"authorized_doorbots":[{"id":53,"kind":"doorbell","description":"Shared"}],"other":[{"id":42,"kind":"intercom_handset_audio","description":"Entrance"}]}"#;
    let inventory = crate::ring_camera::parse(body).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(inventory.cameras.len(), 3);
    assert_eq!(inventory.cameras[2].device_id, "18446744073709551615");
    let encoded = serde_json::to_string(&inventory).unwrap_or_default();
    assert!(!encoded.contains("private"));
    assert!(!encoded.contains("Entrance"));
    assert!(encoded.contains("protocol_research"));
    assert_eq!(
        crate::ring_wire::parse_devices(body)
            .unwrap_or_else(|e| panic!("{e}"))
            .len(),
        1
    );
    for body in [
        r#"{"doorbots":[{"id":0,"kind":"doorbell","description":"Front"}]}"#,
        r#"{"doorbots":[{"id":51,"kind":"doorbell","description":"Front"}],"stickup_cams":[{"id":51,"kind":"stickup_cam","description":"Garden"}]}"#,
        r#"{"doorbots":[{"id":42,"kind":"intercom_handset_audio","description":"Entrance"}]}"#,
        r#"{"doorbots":[{"id":42,"kind":"doorbell","description":"Front"}],"other":[{"id":42,"kind":"intercom_handset_audio","description":"Entrance"}]}"#,
    ] {
        assert!(crate::ring_camera::parse(body.as_bytes()).is_err());
    }
}

#[test]
fn camera_inventory_rejects_overflow_and_preserves_empty_accounts() {
    for count in [0, 32, 33] {
        let cameras: Vec<_> = (1..=count)
            .map(|id| serde_json::json!({"id":id,"kind":"stickup_cam","description":"Camera"}))
            .collect();
        let body =
            serde_json::to_vec(&serde_json::json!({"stickup_cams":cameras})).unwrap_or_default();
        assert_eq!(crate::ring_camera::parse(&body).is_ok(), count <= 32);
    }
}

#[test]
fn camera_openapi_documents_physical_ids_and_native_unverified_media() {
    let paths = include_str!("../docs/openapi/camera-paths.yaml");
    let spec = include_str!("../openapi.yaml");
    assert!(spec.contains("/v1/cameras/{device}/video/sessions:"));
    assert!(paths.contains("required: [expected_device_id]"));
    assert!(paths.contains("const: protocol_research"));
    assert!(paths.contains("maxItems: 32"));
    assert!(paths.contains("security: [{bearerAuth: []}]"));
}

#[tokio::test]
async fn camera_grants_require_exact_camera_id_and_never_use_intercom_control() {
    let state = Arc::new(MockState {
        include_cameras: true,
        ..MockState::default()
    });
    let harness = test_client(Arc::clone(&state)).await;
    for id in ["42", "50", "051", "0"] {
        assert!(harness.client.prepare_camera_call(id).await.is_err());
    }
    assert_eq!(state.ticket_calls.load(Ordering::SeqCst), 0);
    let grant = harness
        .client
        .scoped(Some(51))
        .prepare_camera_call("51")
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(grant.device_id, 51);
    assert!(
        harness
            .client
            .scoped(Some(51))
            .prepare_camera_call("52")
            .await
            .is_err()
    );
    assert!(
        harness
            .client
            .scoped(Some(51))
            .prepare_audio_call()
            .await
            .is_err()
    );
    assert_eq!(state.ticket_calls.load(Ordering::SeqCst), 1);
    assert_eq!(state.control_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn camera_only_boot_inventory_auth_and_bad_bindings_never_start_media() {
    let state = Arc::new(MockState {
        include_cameras: true,
        ..MockState::default()
    });
    let harness = test_client(Arc::clone(&state)).await;
    let directory = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let config = BridgeConfig::new("127.0.0.1".into(), 8775, TOKEN.into(), BTreeMap::new())
        .unwrap_or_else(|e| panic!("{e}"))
        .with_recording_dir(directory.path().join("recordings"));
    let runtime = Arc::new(Runtime::new(config).unwrap_or_else(|e| panic!("{e}")));
    runtime.provider.seed_client(harness.client.clone()).await;
    let app = crate::router(Arc::clone(&runtime));
    for (path, method, token, body, status) in [
        (
            "/v1/cameras",
            "GET",
            "wrong",
            String::new(),
            StatusCode::UNAUTHORIZED,
        ),
        (
            "/v1/cameras/52/video/sessions",
            "POST",
            TOKEN,
            serde_json::to_string(&request(OFFER)).unwrap_or_default(),
            StatusCode::BAD_REQUEST,
        ),
        ("/v1/cameras", "GET", TOKEN, String::new(), StatusCode::OK),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .method(method)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap_or_else(|e| panic!("{e}")),
            )
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(response.status(), status);
        if status == StatusCode::OK {
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            let body = response
                .into_body()
                .collect()
                .await
                .unwrap_or_else(|e| panic!("{e}"))
                .to_bytes();
            let body: serde_json::Value =
                serde_json::from_slice(&body).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(body["cameras"][0]["device_id"], "51");
            assert_eq!(body["cameras"][0]["location_name"], "Home");
        }
    }
    assert_eq!(state.ticket_calls.load(Ordering::SeqCst), 0);
    assert_eq!(state.control_calls.load(Ordering::SeqCst), 0);
    assert!(
        runtime
            .device_summaries()
            .unwrap_or_else(|e| panic!("{e}"))
            .is_empty()
    );
    drop(runtime);
}
