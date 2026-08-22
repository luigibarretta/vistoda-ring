use std::{collections::BTreeSet, sync::Arc, time::Duration};

use tokio::{
    sync::{mpsc, oneshot},
    time::{Instant, interval_at},
};

use crate::{
    BridgeError,
    ring_audio::{SESSION_SECONDS, SessionEndReason},
    ring_media_peer::MediaPeer,
    ring_provider::RingProvider,
    ring_relay_metrics::RelayMetrics,
    ring_relay_protocol::RelayStage,
    ring_signaling::{Incoming, Signaling},
};

const START_TIMEOUT: Duration = Duration::from_secs(25);

pub struct RelayWorker {
    provider: Arc<RingProvider>,
    metrics: Arc<RelayMetrics>,
}

struct ActiveCall {
    peer: MediaPeer,
    signaling: Signaling,
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum Stage {
    SessionCreated,
    AnswerReceived,
    CameraConnected,
    CameraOptionsSent,
    Activated,
    PeerConnected,
    ActiveSent,
}

type Stages = BTreeSet<Stage>;

impl RelayWorker {
    pub const fn new(provider: Arc<RingProvider>, metrics: Arc<RelayMetrics>) -> Self {
        Self { provider, metrics }
    }

    pub async fn run(
        self,
        ring_audio: mpsc::Sender<Vec<u8>>,
        client_audio: mpsc::Receiver<Vec<u8>>,
        stages: mpsc::Sender<RelayStage>,
        cancel: oneshot::Receiver<()>,
    ) -> SessionEndReason {
        let Ok(call) = self.start(ring_audio, client_audio).await else {
            return SessionEndReason::StartupFailed;
        };
        call.run(stages, cancel).await
    }

    async fn start(
        &self,
        ring_audio: mpsc::Sender<Vec<u8>>,
        client_audio: mpsc::Receiver<Vec<u8>>,
    ) -> Result<ActiveCall, BridgeError> {
        let peer = MediaPeer::new_relay(ring_audio, Arc::clone(&self.metrics)).await?;
        let offer = match peer.offer().await {
            Ok(value) => value,
            Err(error) => {
                let _ = peer.close().await;
                return Err(error);
            }
        };
        let grant = self.provider.client().await?.prepare_audio_call().await?;
        let mut signaling = match Signaling::connect(&grant.ticket, grant.device_id).await {
            Ok(value) => value,
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
        peer.start_audio(
            client_audio,
            Duration::from_secs(SESSION_SECONDS),
            Some(Arc::clone(&self.metrics)),
        );
        Ok(ActiveCall { peer, signaling })
    }
}

impl ActiveCall {
    async fn run(
        mut self,
        stage_sender: mpsc::Sender<RelayStage>,
        mut cancel: oneshot::Receiver<()>,
    ) -> SessionEndReason {
        let reason = self.drive(&stage_sender, &mut cancel).await;
        let _ = self.signaling.close().await;
        let _ = self.peer.close().await;
        reason
    }

    async fn drive(
        &mut self,
        stage_sender: &mpsc::Sender<RelayStage>,
        cancel: &mut oneshot::Receiver<()>,
    ) -> SessionEndReason {
        let session_deadline = Instant::now() + Duration::from_secs(SESSION_SECONDS);
        let startup_deadline = Instant::now() + START_TIMEOUT;
        let mut pings = interval_at(
            Instant::now() + Duration::from_secs(5),
            Duration::from_secs(5),
        );
        let connected = self.peer.wait_connected(START_TIMEOUT);
        tokio::pin!(connected);
        let mut stages = Stages::new();
        loop {
            tokio::select! {
                _ = &mut *cancel => return SessionEndReason::UserStop,
                () = tokio::time::sleep_until(session_deadline) => {
                    return SessionEndReason::LifetimeExpired;
                }
                () = tokio::time::sleep_until(startup_deadline), if !stages.contains(&Stage::ActiveSent) => {
                    return SessionEndReason::StartupFailed;
                }
                result = &mut connected, if !stages.contains(&Stage::PeerConnected) => {
                    if result.is_err() { return SessionEndReason::StartupFailed }
                    stages.insert(Stage::PeerConnected);
                }
                _ = pings.tick() => {
                    if self.signaling.ping().await.is_err() {
                        return SessionEndReason::SignalingFailed;
                    }
                }
                message = self.signaling.next() => {
                    match message {
                        Ok(Some(message)) => {
                            if self.handle(message, &mut stages).await.is_err() {
                                return SessionEndReason::SignalingFailed;
                            }
                        }
                        Ok(None) => return SessionEndReason::RemoteClosed,
                        Err(_) => return SessionEndReason::SignalingFailed,
                    }
                }
            }
            if make_active(&mut self.signaling, &mut stages).await.is_err() {
                return SessionEndReason::SignalingFailed;
            }
            if stages.contains(&Stage::Activated)
                && stages.contains(&Stage::CameraConnected)
                && stages.contains(&Stage::PeerConnected)
                && !stages.contains(&Stage::ActiveSent)
            {
                if stage_sender.send(RelayStage::Active).await.is_err() {
                    return SessionEndReason::ConnectionEnded;
                }
                stages.insert(Stage::ActiveSent);
            }
        }
    }

    async fn handle(&self, message: Incoming, stages: &mut Stages) -> Result<(), BridgeError> {
        match message {
            Incoming::Answer(sdp) if !stages.contains(&Stage::AnswerReceived) => {
                self.peer.accept_answer(sdp).await?;
                stages.insert(Stage::AnswerReceived);
            }
            Incoming::Ice { candidate, line } => self.peer.add_ice(candidate, line).await?,
            Incoming::SessionCreated => {
                stages.insert(Stage::SessionCreated);
            }
            Incoming::CameraConnected => {
                stages.insert(Stage::CameraConnected);
            }
            Incoming::Close { .. } => return Err(BridgeError::UpstreamUnavailable),
            Incoming::Answer(_) | Incoming::Other => {}
        }
        Ok(())
    }
}

async fn make_active(signaling: &mut Signaling, stages: &mut Stages) -> Result<(), BridgeError> {
    if stages.contains(&Stage::SessionCreated)
        && stages.contains(&Stage::AnswerReceived)
        && !stages.contains(&Stage::Activated)
    {
        signaling.activate().await?;
        stages.insert(Stage::Activated);
    }
    if stages.contains(&Stage::SessionCreated)
        && stages.contains(&Stage::CameraConnected)
        && !stages.contains(&Stage::CameraOptionsSent)
    {
        signaling.camera_options().await?;
        stages.insert(Stage::CameraOptionsSent);
    }
    Ok(())
}
