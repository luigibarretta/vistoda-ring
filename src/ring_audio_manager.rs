use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};

use async_trait::async_trait;
use tokio::{
    sync::{Mutex, oneshot},
    time::Instant,
};
use uuid::Uuid;

use crate::{
    BridgeError,
    ring_audio::{
        AudioSessionCreated, AudioSessionRequest, NegotiatedAudio, SESSION_SECONDS, validate_offer,
    },
    ring_audio_worker::ProductionSessionRunner,
};

const START_TIMEOUT: Duration = Duration::from_secs(25);
const SESSION_COOLDOWN: Duration = Duration::from_secs(10);

struct ActiveSession {
    device: String,
    cancel: Option<oneshot::Sender<()>>,
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
    );
}

#[derive(Clone)]
pub struct RingAudioSessions {
    state: Arc<Mutex<SessionState>>,
    runner: Arc<dyn SessionRunner>,
}

impl RingAudioSessions {
    pub fn production(session_file: PathBuf) -> Self {
        Self::new(Arc::new(ProductionSessionRunner::new(session_file)))
    }

    pub(crate) fn new(runner: Arc<dyn SessionRunner>) -> Self {
        Self {
            state: Arc::new(Mutex::new(SessionState::default())),
            runner,
        }
    }

    pub async fn start(
        &self,
        device: String,
        request: AudioSessionRequest,
    ) -> Result<AudioSessionCreated, BridgeError> {
        validate_offer(&request.offer_sdp)?;
        let id = Uuid::new_v4();
        let (cancel_tx, cancel_rx) = oneshot::channel();
        let (ready_tx, ready_rx) = oneshot::channel();
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
                },
            );
        }
        self.spawn(id, request.offer_sdp, ready_tx, cancel_rx);
        let negotiated = match tokio::time::timeout(START_TIMEOUT, ready_rx).await {
            Ok(Ok(Ok(value))) => value,
            Ok(Ok(Err(error))) => {
                self.delete(id).await;
                return Err(error);
            }
            Ok(Err(_)) | Err(_) => {
                self.delete(id).await;
                return Err(BridgeError::UpstreamUnavailable);
            }
        };
        Ok(AudioSessionCreated {
            session_id: id.to_string(),
            answer_sdp: negotiated.answer_sdp,
            ice_candidates: negotiated.ice_candidates,
            mode: request.mode,
            expires_in: SESSION_SECONDS,
        })
    }

    pub async fn delete(&self, id: Uuid) {
        let cancel = self
            .state
            .lock()
            .await
            .active
            .get_mut(&id)
            .and_then(|active| active.cancel.take());
        if let Some(cancel) = cancel {
            let _ = cancel.send(());
        }
    }

    fn spawn(
        &self,
        id: Uuid,
        offer: String,
        ready: oneshot::Sender<Result<NegotiatedAudio, BridgeError>>,
        cancel: oneshot::Receiver<()>,
    ) {
        let runner = Arc::clone(&self.runner);
        let state = Arc::clone(&self.state);
        tokio::spawn(async move {
            runner.run(offer, ready, cancel).await;
            let mut state = state.lock().await;
            if let Some(active) = state.active.remove(&id) {
                state
                    .cooldowns
                    .insert(active.device, Instant::now() + SESSION_COOLDOWN);
            }
        });
    }
}

#[cfg(test)]
#[path = "ring_audio_manager_tests.rs"]
mod tests;
