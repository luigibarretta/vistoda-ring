use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use rtc::peer_connection::configuration::media_engine::MIME_TYPE_PCMU;
use tokio::sync::{Mutex, Notify, mpsc};
use webrtc::{
    media_stream::track_remote::{TrackRemote, TrackRemoteEvent},
    peer_connection::{PeerConnectionEventHandler, RTCIceGatheringState, RTCPeerConnectionState},
    runtime::{Runtime, Sender},
};

use crate::{ring_relay_metrics::RelayMetrics, ring_relay_protocol::MAX_MESSAGE_BYTES};

#[derive(Default)]
pub struct PeerStats {
    connected: AtomicBool,
    received_packets: AtomicU64,
    received_bytes: AtomicU64,
    silent_packets: AtomicU64,
    codec: Mutex<Option<String>>,
}

pub struct PeerSnapshot {
    pub connected: bool,
    pub received_packets: u64,
    pub received_bytes: u64,
    pub silent_packets: u64,
    pub codec: Option<String>,
}

impl PeerStats {
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub fn sent_silence(&self) {
        self.silent_packets.fetch_add(1, Ordering::Relaxed);
    }

    pub async fn snapshot(&self) -> PeerSnapshot {
        let received_packets = self.received_packets.load(Ordering::Relaxed);
        let codec = self
            .codec
            .lock()
            .await
            .clone()
            .filter(|value| !value.is_empty())
            .or_else(|| (received_packets > 0).then(|| MIME_TYPE_PCMU.to_owned()));
        PeerSnapshot {
            connected: self.connected.load(Ordering::Relaxed),
            received_packets,
            received_bytes: self.received_bytes.load(Ordering::Relaxed),
            silent_packets: self.silent_packets.load(Ordering::Relaxed),
            codec,
        }
    }
}

#[derive(Clone)]
pub struct Handler {
    pub stats: Arc<PeerStats>,
    pub connected: Arc<Notify>,
    pub gather_complete: Sender<()>,
    pub runtime: Arc<dyn Runtime>,
    pub outbound: Option<mpsc::Sender<Vec<u8>>>,
    pub relay_metrics: Option<Arc<RelayMetrics>>,
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for Handler {
    async fn on_ice_gathering_state_change(&self, state: RTCIceGatheringState) {
        if state == RTCIceGatheringState::Complete {
            let _ = self.gather_complete.try_send(());
        }
    }

    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        if state == RTCPeerConnectionState::Connected {
            self.stats.connected.store(true, Ordering::Relaxed);
            self.connected.notify_waiters();
        }
    }

    async fn on_track(&self, track: Arc<dyn TrackRemote>) {
        let Some(ssrc) = track.ssrcs().await.first().copied() else {
            return;
        };
        if let Some(codec) = track.codec(ssrc).await {
            *self.stats.codec.lock().await = Some(codec.mime_type);
        }
        let stats = Arc::clone(&self.stats);
        let outbound = self.outbound.clone();
        let relay_metrics = self.relay_metrics.clone();
        self.runtime.spawn(Box::pin(async move {
            while let Some(event) = track.poll().await {
                if let TrackRemoteEvent::OnRtpPacket(packet) = event {
                    stats.received_packets.fetch_add(1, Ordering::Relaxed);
                    let length = u64::try_from(packet.payload.len()).unwrap_or(u64::MAX);
                    stats.received_bytes.fetch_add(length, Ordering::Relaxed);
                    if let Some(sender) = &outbound {
                        if packet.payload.is_empty() || packet.payload.len() > MAX_MESSAGE_BYTES {
                            if let Some(metrics) = &relay_metrics {
                                metrics.ring_frame_dropped();
                            }
                            continue;
                        }
                        match sender.try_send(packet.payload.to_vec()) {
                            Ok(()) => {
                                if let Some(metrics) = &relay_metrics {
                                    metrics.ring_frame_forwarded(length);
                                }
                            }
                            Err(_) => {
                                if let Some(metrics) = &relay_metrics {
                                    metrics.ring_frame_dropped();
                                }
                            }
                        }
                    }
                }
            }
        }));
    }
}
