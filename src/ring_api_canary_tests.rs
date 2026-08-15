use super::{ApiCanaryEvidence, local_base_url};

#[test]
fn canary_target_is_strictly_loopback() {
    assert!(local_base_url("http://127.0.0.1:8775/").is_ok());
    for target in [
        "https://127.0.0.1:8775/",
        "http://192.168.50.130:8775/",
        "http://user@127.0.0.1:8775/",
        "http://127.0.0.1:8775/path",
    ] {
        assert!(local_base_url(target).is_err());
    }
}

#[test]
fn release_gate_requires_media_delete_and_local_teardown() {
    let mut evidence = ApiCanaryEvidence {
        protocol: "ring_consumer_webrtc_audio_v1",
        connected: true,
        codec: Some("audio/PCMU".into()),
        received_packets: 10,
        received_bytes: 1600,
        silent_packets_sent: 10,
        delete_status: 204,
        teardown_complete: true,
    };
    assert!(evidence.passes_release_gate());
    evidence.delete_status = 500;
    assert!(!evidence.passes_release_gate());
}
