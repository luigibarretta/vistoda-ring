use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use tokio::sync::oneshot;
use tokio::time::{Instant, interval_at};

use crate::{
    BridgeError,
    ring_audio::{
        IceCandidate, NegotiatedAudio, SESSION_SECONDS, SessionEndReason, validate_answer,
    },
    ring_audio_manager::SessionRunner,
    ring_provider::RingProvider,
    ring_signaling::{Incoming, Signaling},
};

const NEGOTIATION_TIMEOUT: Duration = Duration::from_secs(20);
const ICE_SETTLE: Duration = Duration::from_millis(750);
const MAX_ICE_CANDIDATES: usize = 64;

pub struct ProductionSessionRunner {
    provider: Arc<RingProvider>,
}

impl ProductionSessionRunner {
    pub const fn new(provider: Arc<RingProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl SessionRunner for ProductionSessionRunner {
    async fn run(
        &self,
        offer_sdp: String,
        ready: oneshot::Sender<Result<NegotiatedAudio, BridgeError>>,
        mut cancel: oneshot::Receiver<()>,
    ) -> SessionEndReason {
        let (mut signaling, negotiated) = match self.negotiate(&offer_sdp).await {
            Ok(value) => value,
            Err(error) => {
                let _ = ready.send(Err(error));
                return SessionEndReason::StartupFailed;
            }
        };
        if ready.send(Ok(negotiated)).is_err() {
            let _ = signaling.close().await;
            return SessionEndReason::StartupFailed;
        }
        let deadline = Instant::now() + Duration::from_secs(SESSION_SECONDS);
        let mut pings = interval_at(
            Instant::now() + Duration::from_secs(5),
            Duration::from_secs(5),
        );
        let reason = loop {
            tokio::select! {
                _ = &mut cancel => break SessionEndReason::UserStop,
                () = tokio::time::sleep_until(deadline) => {
                    break SessionEndReason::LifetimeExpired;
                }
                _ = pings.tick() => {
                    if signaling.ping().await.is_err() {
                        break SessionEndReason::SignalingFailed;
                    }
                }
                message = signaling.next() => {
                    match message {
                        Ok(Some(Incoming::Close { .. }) | None) => {
                            break SessionEndReason::RemoteClosed;
                        }
                        Err(_) => break SessionEndReason::SignalingFailed,
                        Ok(Some(_)) => {}
                    }
                }
            }
        };
        let _ = signaling.close().await;
        reason
    }
}

impl ProductionSessionRunner {
    async fn negotiate(&self, offer: &str) -> Result<(Signaling, NegotiatedAudio), BridgeError> {
        let grant = self.provider.client().await?.prepare_audio_call().await?;
        let mut signaling = Signaling::connect(&grant.ticket, grant.device_id).await?;
        signaling.offer(offer).await?;
        let negotiated = match self.collect(&mut signaling).await {
            Ok(value) => value,
            Err(error) => {
                let _ = signaling.close().await;
                return Err(error);
            }
        };
        Ok((signaling, negotiated))
    }

    async fn collect(&self, signaling: &mut Signaling) -> Result<NegotiatedAudio, BridgeError> {
        let deadline = Instant::now() + NEGOTIATION_TIMEOUT;
        let mut answer = None;
        let mut candidates = Vec::new();
        let mut session_created = false;
        let mut camera_connected = false;
        let mut activated = false;
        loop {
            if activated && camera_connected {
                self.settle_ice(signaling, &mut candidates).await?;
                return Ok(NegotiatedAudio {
                    answer_sdp: answer.ok_or_else(|| protocol("Ring answer is unavailable"))?,
                    ice_candidates: candidates,
                });
            }
            let message = tokio::time::timeout_at(deadline, signaling.next())
                .await
                .map_err(|_| BridgeError::UpstreamUnavailable)?
                .ok()
                .flatten()
                .ok_or_else(|| protocol("Ring closed signaling during negotiation"))?;
            match message {
                Incoming::Answer(sdp) => {
                    validate_answer(&sdp)?;
                    answer = Some(sdp);
                }
                Incoming::Ice { candidate, line } => push_ice(&mut candidates, candidate, line)?,
                Incoming::SessionCreated => session_created = true,
                Incoming::CameraConnected => camera_connected = true,
                Incoming::Close { .. } => return Err(protocol("Ring closed the audio session")),
                Incoming::Other => {}
            }
            if session_created && answer.is_some() && !activated {
                signaling.activate().await?;
                activated = true;
            }
            if session_created && camera_connected {
                signaling.camera_options().await?;
            }
        }
    }

    async fn settle_ice(
        &self,
        signaling: &mut Signaling,
        candidates: &mut Vec<IceCandidate>,
    ) -> Result<(), BridgeError> {
        let deadline = Instant::now() + ICE_SETTLE;
        loop {
            match tokio::time::timeout_at(deadline, signaling.next()).await {
                Ok(Ok(Some(Incoming::Ice { candidate, line }))) => {
                    push_ice(candidates, candidate, line)?;
                }
                Ok(Ok(Some(Incoming::Close { .. } | Incoming::Other) | None)) | Err(_) => break,
                Ok(Ok(Some(_))) => {}
                Ok(Err(error)) => return Err(error),
            }
        }
        Ok(())
    }
}

fn push_ice(
    target: &mut Vec<IceCandidate>,
    candidate: String,
    line: u16,
) -> Result<(), BridgeError> {
    if target.len() >= MAX_ICE_CANDIDATES || candidate.len() > 4096 {
        return Err(protocol("Ring ICE candidates exceed the limit"));
    }
    target.push(IceCandidate {
        candidate,
        sdp_mline_index: line,
    });
    Ok(())
}

fn protocol(message: &str) -> BridgeError {
    BridgeError::Protocol(message.into())
}
