use std::{collections::BTreeSet, time::Duration};

use serde::Serialize;
use tokio::time::{Instant, interval_at};

use crate::{
    BridgeError,
    ring_client::AudioCallGrant,
    ring_media_handler::PeerSnapshot,
    ring_media_peer::MediaPeer,
    ring_signaling::{Incoming, Signaling},
};

#[derive(Debug, Serialize)]
pub struct AudioCanaryEvidence {
    pub protocol: &'static str,
    pub requested_seconds: u64,
    pub observed: Vec<CanaryStage>,
    pub codec: Option<String>,
    pub received_packets: u64,
    pub received_bytes: u64,
    pub silent_packets_sent: u64,
    pub remote_close_code: Option<i64>,
    pub teardown: Teardown,
}

impl AudioCanaryEvidence {
    #[must_use]
    pub fn passes_release_gate(&self) -> bool {
        let required = [
            CanaryStage::SessionCreated,
            CanaryStage::AnswerReceived,
            CanaryStage::CameraConnected,
            CanaryStage::BidirectionalNegotiated,
            CanaryStage::PeerConnected,
            CanaryStage::Activated,
        ];
        required.iter().all(|stage| self.observed.contains(stage))
            && self.codec.as_deref() == Some("audio/PCMU")
            && self.received_packets > 0
            && self.received_bytes > 0
            && self.silent_packets_sent > 0
            && self.remote_close_code.is_none()
            && matches!(self.teardown, Teardown::Complete)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CanaryStage {
    SessionCreated,
    AnswerReceived,
    CameraConnected,
    BidirectionalNegotiated,
    PeerConnected,
    Activated,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Teardown {
    Complete,
    Incomplete,
}

#[derive(Default)]
struct SignalStats {
    seen: BTreeSet<CanaryStage>,
    remote_close_code: Option<i64>,
}

pub async fn run_audio_canary(
    grant: AudioCallGrant,
    duration: Duration,
) -> Result<AudioCanaryEvidence, BridgeError> {
    if !(Duration::from_secs(5)..=Duration::from_secs(30)).contains(&duration) {
        return Err(protocol("audio canary duration must be 5-30 seconds"));
    }
    let peer = Box::pin(MediaPeer::new()).await?;
    let offer = match peer.offer().await {
        Ok(offer) => offer,
        Err(error) => {
            let _ = peer.close().await;
            return Err(error);
        }
    };
    let mut signaling = match Signaling::connect(&grant.ticket, grant.device_id).await {
        Ok(signaling) => signaling,
        Err(error) => {
            let _ = peer.close().await;
            return Err(error);
        }
    };
    if let Err(error) = signaling.offer(&offer).await {
        let _ = signaling.close().await;
        let _ = peer.close().await;
        return Err(error);
    }
    peer.start_silence(duration);
    let mut stats = SignalStats::default();
    let result = signal_loop(&mut signaling, &peer, duration, &mut stats).await;
    let signal_closed = signaling.close().await.is_ok();
    let peer_closed = peer.close().await.is_ok();
    result?;
    let media = peer.snapshot().await;
    Ok(evidence(
        duration,
        &stats,
        media,
        signal_closed && peer_closed,
    ))
}

async fn signal_loop(
    signaling: &mut Signaling,
    peer: &MediaPeer,
    duration: Duration,
    stats: &mut SignalStats,
) -> Result<(), BridgeError> {
    let deadline = Instant::now() + duration;
    let mut pings = interval_at(
        Instant::now() + Duration::from_secs(5),
        Duration::from_secs(5),
    );
    loop {
        tokio::select! {
            () = tokio::time::sleep_until(deadline) => return Ok(()),
            _instant = pings.tick() => signaling.ping().await?,
            message = signaling.next() => {
                let Some(message) = message? else { return Ok(()) };
                handle(message, signaling, peer, stats).await?;
                activate_when_ready(signaling, stats).await?;
                if stats.remote_close_code.is_some() { return Ok(()) }
            }
        }
    }
}

async fn handle(
    message: Incoming,
    signaling: &mut Signaling,
    peer: &MediaPeer,
    stats: &mut SignalStats,
) -> Result<(), BridgeError> {
    match message {
        Incoming::Answer(sdp) => {
            if audio_is_sendrecv(&sdp) {
                stats.seen.insert(CanaryStage::BidirectionalNegotiated);
            }
            peer.accept_answer(sdp).await?;
            stats.seen.insert(CanaryStage::AnswerReceived);
        }
        Incoming::Ice { candidate, line } => peer.add_ice(candidate, line).await?,
        Incoming::SessionCreated => {
            stats.seen.insert(CanaryStage::SessionCreated);
        }
        Incoming::CameraConnected => {
            stats.seen.insert(CanaryStage::CameraConnected);
            if stats.seen.contains(&CanaryStage::SessionCreated) {
                signaling.camera_options().await?;
            }
        }
        Incoming::Close { code } => stats.remote_close_code = Some(code),
        Incoming::Other => {}
    }
    Ok(())
}

async fn activate_when_ready(
    signaling: &mut Signaling,
    stats: &mut SignalStats,
) -> Result<(), BridgeError> {
    if stats.seen.contains(&CanaryStage::SessionCreated)
        && stats.seen.contains(&CanaryStage::AnswerReceived)
        && !stats.seen.contains(&CanaryStage::Activated)
    {
        signaling.activate().await?;
        stats.seen.insert(CanaryStage::Activated);
    }
    Ok(())
}

fn evidence(
    duration: Duration,
    stats: &SignalStats,
    media: PeerSnapshot,
    teardown_complete: bool,
) -> AudioCanaryEvidence {
    AudioCanaryEvidence {
        protocol: "ring_signalsocket_webrtc_audio_v1",
        requested_seconds: duration.as_secs(),
        observed: {
            let mut seen = stats.seen.clone();
            if media.connected {
                seen.insert(CanaryStage::PeerConnected);
            }
            seen.into_iter().collect()
        },
        codec: media.codec,
        received_packets: media.received_packets,
        received_bytes: media.received_bytes,
        silent_packets_sent: media.silent_packets,
        remote_close_code: stats.remote_close_code,
        teardown: if teardown_complete {
            Teardown::Complete
        } else {
            Teardown::Incomplete
        },
    }
}

pub(crate) fn audio_is_sendrecv(sdp: &str) -> bool {
    let mut in_audio = false;
    for line in sdp.lines().map(str::trim) {
        if line.starts_with("m=") {
            in_audio = line.starts_with("m=audio ");
        } else if in_audio && line == "a=sendrecv" {
            return true;
        }
    }
    false
}

fn protocol(message: &str) -> BridgeError {
    BridgeError::Protocol(message.into())
}
