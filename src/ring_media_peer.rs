use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use rtc::{
    interceptor::Registry,
    media::Sample,
    media_stream::MediaStreamTrack,
    peer_connection::{
        configuration::{
            RTCConfigurationBuilder,
            interceptor_registry::register_default_interceptors,
            media_engine::{MIME_TYPE_PCMU, MediaEngine},
        },
        sdp::RTCSessionDescription,
        transport::RTCIceServer,
    },
    rtp_transceiver::rtp_sender::{
        RTCRtpCodec, RTCRtpCodecParameters, RTCRtpCodingParameters, RTCRtpEncodingParameters,
        RtpCodecKind,
    },
};
use tokio::sync::Notify;
use uuid::Uuid;
use webrtc::{
    media_stream::{
        Track, track_local::TrackLocal, track_local::static_sample::TrackLocalStaticSample,
    },
    peer_connection::{PeerConnection, PeerConnectionBuilder, RTCIceCandidateInit},
    rtp_transceiver::RtpSender,
    runtime::{Runtime, TokioRuntime, channel},
};

use crate::{
    BridgeError,
    error::BridgeError::Protocol,
    ring_media_handler::{Handler, PeerSnapshot, PeerStats},
    ring_media_network::routed_local_ip,
};

pub struct MediaPeer {
    connection: Arc<dyn PeerConnection>,
    local_audio: Arc<TrackLocalStaticSample>,
    sender: Arc<dyn RtpSender>,
    stats: Arc<PeerStats>,
    connected: Arc<Notify>,
    stopped: Arc<AtomicBool>,
    gather_complete: tokio::sync::Mutex<webrtc::runtime::Receiver<()>>,
}
impl MediaPeer {
    pub async fn new() -> Result<Self, BridgeError> {
        let (codec, mut engine) = media_engine()?;
        let registry = register_default_interceptors(Registry::new(), &mut engine)
            .map_err(|_| protocol("WebRTC interceptor setup failed"))?;
        let runtime: Arc<dyn Runtime> = Arc::new(TokioRuntime);
        let stats = Arc::new(PeerStats::default());
        let connected = Arc::new(Notify::new());
        let (gather_tx, gather_rx) = channel(1);
        let handler = Arc::new(Handler {
            stats: Arc::clone(&stats),
            connected: Arc::clone(&connected),
            gather_complete: gather_tx,
            runtime: Arc::clone(&runtime),
        });
        let config = RTCConfigurationBuilder::new()
            .with_ice_servers(vec![RTCIceServer {
                urls: vec![
                    "stun:stun.kinesisvideo.us-east-1.amazonaws.com:443".into(),
                    "stun:stun.l.google.com:19302".into(),
                ],
                ..Default::default()
            }])
            .build();
        let bind_address = format!("{}:0", routed_local_ip()?);
        let connection = Box::pin(
            PeerConnectionBuilder::new()
                .with_configuration(config)
                .with_media_engine(engine)
                .with_interceptor_registry(registry)
                .with_handler(handler)
                .with_runtime(runtime)
                .with_udp_addrs(vec![bind_address])
                .build(),
        )
        .await
        .map_err(|_| protocol("peer connection setup failed"))?;
        let connection: Arc<dyn PeerConnection> = Arc::new(connection);
        let local_audio = local_track(&codec)?;
        let sender = connection
            .add_track(Arc::clone(&local_audio) as Arc<dyn TrackLocal>)
            .await
            .map_err(|_| protocol("audio transceiver setup failed"))?;
        Ok(Self {
            connection,
            local_audio,
            sender,
            stats,
            connected,
            stopped: Arc::new(AtomicBool::new(false)),
            gather_complete: tokio::sync::Mutex::new(gather_rx),
        })
    }
    pub async fn offer(&self) -> Result<String, BridgeError> {
        let offer = self
            .connection
            .create_offer(None)
            .await
            .map_err(|_| protocol("SDP offer creation failed"))?;
        self.connection
            .set_local_description(offer)
            .await
            .map_err(|_| protocol("local SDP setup failed"))?;
        let mut complete = self.gather_complete.lock().await;
        let _gathered = tokio::time::timeout(Duration::from_secs(3), complete.recv()).await;
        drop(complete);
        self.connection
            .local_description()
            .await
            .map(|value| value.sdp)
            .ok_or_else(|| protocol("local SDP is unavailable"))
    }
    pub async fn accept_answer(&self, sdp: String) -> Result<(), BridgeError> {
        let answer = RTCSessionDescription::answer(sdp)
            .map_err(|_| protocol("Ring SDP answer is invalid"))?;
        self.connection
            .set_remote_description(answer)
            .await
            .map_err(|_| protocol("remote SDP setup failed"))
    }
    pub async fn add_ice(&self, candidate: String, line: u16) -> Result<(), BridgeError> {
        self.connection
            .add_ice_candidate(RTCIceCandidateInit {
                candidate,
                sdp_mline_index: Some(line),
                ..Default::default()
            })
            .await
            .map_err(|_| protocol("remote ICE candidate was rejected"))
    }
    pub fn start_silence(&self, deadline: Duration) {
        let track = Arc::clone(&self.local_audio);
        let sender = Arc::clone(&self.sender);
        let connected = Arc::clone(&self.connected);
        let stopped = Arc::clone(&self.stopped);
        let stats = Arc::clone(&self.stats);
        tokio::spawn(async move {
            if tokio::time::timeout(deadline, connected.notified())
                .await
                .is_err()
            {
                return;
            }
            let Ok(payload_type) = negotiated_payload_type(&sender).await else {
                return;
            };
            let Some(ssrc) = track.ssrcs().await.first().copied() else {
                return;
            };
            let end = tokio::time::Instant::now() + deadline;
            let mut interval = tokio::time::interval(Duration::from_millis(20));
            while tokio::time::Instant::now() < end && !stopped.load(Ordering::Relaxed) {
                interval.tick().await;
                let result = track
                    .sample_writer(ssrc, payload_type)
                    .write_sample(&Sample {
                        data: vec![0xff; 160].into(),
                        duration: Duration::from_millis(20),
                        ..Default::default()
                    })
                    .await;
                if result.is_ok() {
                    stats.sent_silence();
                }
            }
        });
    }
    pub async fn snapshot(&self) -> PeerSnapshot {
        self.stats.snapshot().await
    }

    pub async fn close(&self) -> Result<(), BridgeError> {
        self.stopped.store(true, Ordering::Relaxed);
        self.connection
            .close()
            .await
            .map_err(|_| protocol("peer close failed"))
    }
}

fn media_engine() -> Result<(RTCRtpCodecParameters, MediaEngine), BridgeError> {
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

fn local_track(codec: &RTCRtpCodecParameters) -> Result<Arc<TrackLocalStaticSample>, BridgeError> {
    let id = Uuid::new_v4();
    let ssrc = u32::from_le_bytes(id.as_bytes()[..4].try_into().unwrap_or([0, 0, 0, 1])).max(1);
    TrackLocalStaticSample::new(MediaStreamTrack::new(
        "ring-intercom-canary".into(),
        "audio".into(),
        "ring-intercom-canary".into(),
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

async fn negotiated_payload_type(sender: &Arc<dyn RtpSender>) -> Result<u8, BridgeError> {
    sender
        .get_parameters()
        .await
        .map_err(|_| protocol("audio negotiation unavailable"))?
        .rtp_parameters
        .codecs
        .first()
        .map(|codec| codec.payload_type)
        .ok_or_else(|| protocol("audio codec was not negotiated"))
}

fn protocol(message: &str) -> BridgeError {
    Protocol(message.into())
}
