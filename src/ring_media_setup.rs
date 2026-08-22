use std::sync::Arc;

use rtc::{
    media_stream::MediaStreamTrack,
    peer_connection::configuration::media_engine::{MIME_TYPE_PCMU, MediaEngine},
    rtp_transceiver::rtp_sender::{
        RTCRtpCodec, RTCRtpCodecParameters, RTCRtpCodingParameters, RTCRtpEncodingParameters,
        RtpCodecKind,
    },
};
use uuid::Uuid;
use webrtc::media_stream::track_local::static_sample::TrackLocalStaticSample;

use crate::BridgeError;

pub fn media_engine() -> Result<(RTCRtpCodecParameters, MediaEngine), BridgeError> {
    let codec = RTCRtpCodecParameters {
        rtp_codec: RTCRtpCodec {
            mime_type: MIME_TYPE_PCMU.to_owned(),
            clock_rate: 8_000,
            channels: 1,
            sdp_fmtp_line: String::new(),
            rtcp_feedback: vec![],
        },
        payload_type: 0,
    };
    let mut engine = MediaEngine::default();
    engine
        .register_codec(codec.clone(), RtpCodecKind::Audio)
        .map_err(|_| protocol("PCMU codec setup failed"))?;
    Ok((codec, engine))
}

pub fn local_track(
    codec: &RTCRtpCodecParameters,
) -> Result<Arc<TrackLocalStaticSample>, BridgeError> {
    let id = Uuid::new_v4();
    let ssrc = u32::from_le_bytes(id.as_bytes()[..4].try_into().unwrap_or([0, 0, 0, 1])).max(1);
    TrackLocalStaticSample::new(MediaStreamTrack::new(
        "ring-intercom-relay".into(),
        "audio".into(),
        "ring-intercom-relay".into(),
        RtpCodecKind::Audio,
        vec![RTCRtpEncodingParameters {
            rtp_coding_parameters: RTCRtpCodingParameters {
                ssrc: Some(ssrc),
                ..Default::default()
            },
            codec: codec.rtp_codec.clone(),
            ..Default::default()
        }],
    ))
    .map(Arc::new)
    .map_err(|_| protocol("local audio track setup failed"))
}

fn protocol(message: &str) -> BridgeError {
    BridgeError::Protocol(message.into())
}
