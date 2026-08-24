use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use rtc::{
    interceptor::Registry,
    peer_connection::{
        configuration::{
            RTCConfigurationBuilder, interceptor_registry::register_default_interceptors,
        },
        sdp::RTCSessionDescription,
        transport::RTCIceServer,
    },
};
use tokio::sync::{Notify, mpsc};
use webrtc::{
    media_stream::{track_local::TrackLocal, track_local::static_sample::TrackLocalStaticSample},
    peer_connection::{PeerConnection, PeerConnectionBuilder, RTCIceCandidateInit},
    rtp_transceiver::RtpSender,
    runtime::{Runtime, TokioRuntime, channel},
};

use crate::{
    BridgeError,
    error::BridgeError::Protocol,
    ring_media_handler::{Handler, PeerSnapshot, PeerStats},
    ring_media_network::routed_local_ip,
    ring_media_sender,
    ring_media_setup::{local_track, media_engine},
    ring_relay_metrics::RelayMetrics,
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
        Self::build(None, None).await
    }

    pub async fn new_relay(
        outbound: mpsc::Sender<Vec<u8>>,
        metrics: Arc<RelayMetrics>,
    ) -> Result<Self, BridgeError> {
        Self::build(Some(outbound), Some(metrics)).await
    }

    async fn build(
        outbound: Option<mpsc::Sender<Vec<u8>>>,
        relay_metrics: Option<Arc<RelayMetrics>>,
    ) -> Result<Self, BridgeError> {
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
            outbound,
            relay_metrics,
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
        let (sender, receiver) = mpsc::channel(1);
        drop(sender);
        self.start_audio(receiver, deadline, None);
    }

    pub fn start_audio(
        &self,
        receiver: mpsc::Receiver<Vec<u8>>,
        deadline: Duration,
        metrics: Option<Arc<RelayMetrics>>,
    ) {
        ring_media_sender::spawn(ring_media_sender::SenderTask {
            track: Arc::clone(&self.local_audio),
            sender: Arc::clone(&self.sender),
            connected: Arc::clone(&self.connected),
            stopped: Arc::clone(&self.stopped),
            stats: Arc::clone(&self.stats),
            receiver,
            deadline,
            metrics,
        });
    }

    pub async fn wait_connected(&self, deadline: Duration) -> Result<(), BridgeError> {
        let notified = self.connected.notified();
        tokio::pin!(notified);
        if self.stats.is_connected() {
            return Ok(());
        }
        tokio::time::timeout(deadline, &mut notified)
            .await
            .map_err(|_| protocol("peer connection timed out"))?;
        Ok(())
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

fn protocol(message: &str) -> BridgeError {
    Protocol(message.into())
}
