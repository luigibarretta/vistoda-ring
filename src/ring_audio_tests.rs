use super::{AudioMode, AudioSessionRequest, validate_offer};

const OFFER: &str = "v=0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 0 111\r\na=sendrecv\r\n";

#[test]
fn accepts_one_sendrecv_pcmu_audio_section() {
    assert!(validate_offer(OFFER).is_ok());
}

#[test]
fn rejects_video_data_channels_and_non_pcmu_audio() {
    for offer in [
        "v=0\r\nm=audio 9 RTP/AVP 111\r\na=sendrecv\r\n",
        "v=0\r\nm=audio 9 RTP/AVP 0\r\na=sendrecv\r\nm=video 9 RTP/AVP 96\r\n",
        "v=0\r\nm=audio 9 RTP/AVP 0\r\na=recvonly\r\n",
        "v=0\r\nm=audio 9 RTP/AVP 0\r\na=sendrecv\r\nm=application 9 DTLS/SCTP 5000",
    ] {
        assert!(validate_offer(offer).is_err());
    }
}

#[test]
fn request_does_not_accept_unknown_fields() {
    let value = serde_json::json!({
        "offer_sdp": OFFER,
        "mode": "listen",
        "token": "must-not-be-accepted"
    });
    assert!(serde_json::from_value::<AudioSessionRequest>(value).is_err());
    assert_eq!(AudioMode::Listen, AudioMode::Listen);
}
