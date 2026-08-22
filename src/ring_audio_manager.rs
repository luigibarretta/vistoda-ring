use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use tokio::{
    sync::{Mutex, oneshot, watch},
    time::Instant,
};
use uuid::Uuid;

use crate::{
    BridgeError,
    ring_audio::{
        AudioMode, AudioSessionCreated, AudioSessionRequest, NegotiatedAudio, SESSION_SECONDS,
        SessionEndReason, validate_request,
    },
    ring_audio_worker::ProductionSessionRunner,
    ring_metrics::RingMetrics,
    ring_provider::RingProvider,
};

const START_TIMEOUT: Duration = Duration::from_secs(25);
const STOP_TIMEOUT: Duration = Duration::from_secs(5);
const SESSION_COOLDOWN: Duration = Duration::from_secs(10);

struct ActiveSession {
    device: String,
    cancel: Option<oneshot::Sender<()>>,
    done: watch::Receiver<bool>,
    requested_end: Arc<AtomicUsize>,
}

struct SessionTask {
    id: Uuid,
    mode: AudioMode,
    started_at: Instant,
    requested_end: Arc<AtomicUsize>,
    offer: String,
    ready: oneshot::Sender<Result<NegotiatedAudio, BridgeError>>,
    cancel: oneshot::Receiver<()>,
    done: watch::Sender<bool>,
}

#[derive(Default)]
struct SessionState {
    active: BTreeMap<Uuid, ActiveSession>,
    cooldowns: BTreeMap<String, Instant>,
}

#[async_trait]
pub trait SessionRunner: Send + Sync {
    async fn run(
        &self,
        offer_sdp: String,
        ready: oneshot::Sender<Result<NegotiatedAudio, BridgeError>>,
        cancel: oneshot::Receiver<()>,
    ) -> SessionEndReason;
}

#[derive(Clone)]
pub struct RingAudioSessions {
    state: Arc<Mutex<SessionState>>,
    runner: Arc<dyn SessionRunner>,
    metrics: Arc<RingMetrics>,
}

impl RingAudioSessions {
    pub fn production(provider: Arc<RingProvider>, metrics: Arc<RingMetrics>) -> Self {
        Self::build(Arc::new(ProductionSessionRunner::new(provider)), metrics)
    }

    #[cfg(test)]
    pub(crate) fn new(runner: Arc<dyn SessionRunner>) -> Self {
        Self::build(runner, Arc::new(RingMetrics::default()))
    }

    fn build(runner: Arc<dyn SessionRunner>, metrics: Arc<RingMetrics>) -> Self {
        Self {
            state: Arc::new(Mutex::new(SessionState::default())),
            runner,
            metrics,
        }
    }

    pub async fn start(
        &self,
        device: String,
        request: AudioSessionRequest,
    ) -> Result<AudioSessionCreated, BridgeError> {
        validate_request(&request)?;
        let id = Uuid::new_v4();
        let started_at = Instant::now();
        let (cancel_tx, cancel_rx) = oneshot::channel();
        let (ready_tx, ready_rx) = oneshot::channel();
        let (done_tx, done_rx) = watch::channel(false);
        let requested_end = Arc::new(AtomicUsize::new(0));
        {
            let mut state = self.state.lock().await;
            state
                .cooldowns
                .retain(|_, deadline| *deadline > Instant::now());
            if state
                .active
                .values()
                .any(|session| session.device == device)
            {
                return Err(BridgeError::SessionBusy);
            }
            if state.cooldowns.contains_key(&device) {
                return Err(BridgeError::RateLimited);
            }
            state.active.insert(
                id,
                ActiveSession {
                    device,
                    cancel: Some(cancel_tx),
                    done: done_rx,
                    requested_end: Arc::clone(&requested_end),
                },
            );
        }
        self.metrics.reserved();
        self.spawn(SessionTask {
            id,
            mode: request.mode,
            started_at,
            requested_end,
            offer: request.offer_sdp,
            ready: ready_tx,
            cancel: cancel_rx,
            done: done_tx,
        });
        let negotiated = match tokio::time::timeout(START_TIMEOUT, ready_rx).await {
            Ok(Ok(Ok(value))) => value,
            Ok(Ok(Err(error))) => {
                let _ = self.delete(id, SessionEndReason::StartFailed).await;
                return Err(error);
            }
            Ok(Err(_)) | Err(_) => {
                let _ = self.delete(id, SessionEndReason::StartFailed).await;
                return Err(BridgeError::UpstreamUnavailable);
            }
        };
        self.metrics.started(request.mode, request.ice_gathering_ms);
        tracing::info!(
            session_id = %id,
            mode = ?request.mode,
            ice_gathering_ms = request.ice_gathering_ms,
            "Ring audio communication started"
        );
        Ok(AudioSessionCreated {
            session_id: id.to_string(),
            answer_sdp: negotiated.answer_sdp,
            ice_candidates: negotiated.ice_candidates,
            mode: request.mode,
            expires_in: SESSION_SECONDS,
        })
    }

    pub async fn delete(&self, id: Uuid, reason: SessionEndReason) -> Result<(), BridgeError> {
        let (cancel, mut done) = {
            let mut state = self.state.lock().await;
            let result = state.active.get_mut(&id).map(|active| {
                active
                    .requested_end
                    .store(reason as usize + 1, Ordering::Release);
                (active.cancel.take(), active.done.clone())
            });
            drop(state);
            let Some(result) = result else {
                return Ok(());
            };
            result
        };
        if let Some(cancel) = cancel {
            let _ = cancel.send(());
        }
        if !*done.borrow() {
            tokio::time::timeout(STOP_TIMEOUT, done.changed())
                .await
                .map_err(|_| BridgeError::UpstreamUnavailable)?
                .map_err(|_| BridgeError::UpstreamUnavailable)?;
        }
        Ok(())
    }

    fn spawn(&self, task: SessionTask) {
        let runner = Arc::clone(&self.runner);
        let state = Arc::clone(&self.state);
        let metrics = Arc::clone(&self.metrics);
        tokio::spawn(async move {
            let runner_reason = runner.run(task.offer, task.ready, task.cancel).await;
            let reason = requested_reason(&task.requested_end).unwrap_or(runner_reason);
            let mut state = state.lock().await;
            if let Some(active) = state.active.remove(&task.id) {
                state
                    .cooldowns
                    .insert(active.device, Instant::now() + SESSION_COOLDOWN);
            }
            drop(state);
            let duration_ms =
                u64::try_from(task.started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
            metrics.ended(reason, duration_ms);
            tracing::info!(
                session_id = %task.id,
                mode = ?task.mode,
                reason = reason.as_str(),
                duration_ms,
                "Ring audio communication ended"
            );
            let _ = task.done.send(true);
        });
    }
}

fn requested_reason(value: &AtomicUsize) -> Option<SessionEndReason> {
    let raw = value.load(Ordering::Acquire);
    raw.checked_sub(1)
        .and_then(|index| SessionEndReason::ALL.get(index).copied())
        .filter(|reason| reason.is_client())
}

#[cfg(test)]
#[path = "ring_audio_manager_tests.rs"]
mod tests;
