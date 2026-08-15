use crate::ring_media_canary::{AudioCanaryEvidence, CanaryStage, Teardown, audio_is_sendrecv};

#[test]
fn bidirectional_audio_is_scoped_to_audio_media() {
    assert!(audio_is_sendrecv(
        "m=audio 9 RTP/AVP 0\r\na=sendrecv\r\nm=video 0 RTP/AVP 96"
    ));
    assert!(!audio_is_sendrecv(
        "m=audio 9 RTP/AVP 0\r\na=sendonly\r\nm=video 9 RTP/AVP 96\r\na=sendrecv"
    ));
}

#[test]
fn release_gate_rejects_negotiation_without_inbound_audio() {
    let mut evidence = valid_evidence();
    evidence.received_packets = 0;
    evidence.received_bytes = 0;
    assert!(!evidence.passes_release_gate());
}

#[test]
fn release_gate_accepts_complete_bidirectional_evidence() {
    assert!(valid_evidence().passes_release_gate());
}

fn valid_evidence() -> AudioCanaryEvidence {
    AudioCanaryEvidence {
        protocol: "ring_signalsocket_webrtc_audio_v1",
        requested_seconds: 5,
        observed: vec![
            CanaryStage::SessionCreated,
            CanaryStage::AnswerReceived,
            CanaryStage::CameraConnected,
            CanaryStage::BidirectionalNegotiated,
            CanaryStage::PeerConnected,
            CanaryStage::Activated,
        ],
        codec: Some("audio/PCMU".into()),
        received_packets: 130,
        received_bytes: 62_400,
        silent_packets_sent: 230,
        remote_close_code: None,
        teardown: Teardown::Complete,
    }
}
