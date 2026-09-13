use crate::{
    BridgeError,
    ring_audio::{AudioSessionRequest, NegotiatedAudio, SessionEndReason},
    ring_audio_manager::{RingAudioSessions, SessionRunner},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::oneshot;

struct VideoRunner(Arc<AtomicBool>);

#[async_trait::async_trait]
impl SessionRunner for VideoRunner {
    fn validate(&self, request: &AudioSessionRequest) -> Result<(), BridgeError> {
        crate::ring_video::validate_request(request)
    }
    async fn run(
        &self,
        offer: String,
        expected: Option<String>,
        ready: oneshot::Sender<Result<NegotiatedAudio, BridgeError>>,
        cancel: oneshot::Receiver<()>,
    ) -> SessionEndReason {
        assert_eq!(expected.as_deref(), Some("51"));
        let _ = ready.send(Ok(NegotiatedAudio {
            answer_sdp: offer.replace("a=recvonly", "a=sendonly"),
            ice_candidates: vec![],
        }));
        let _ = cancel.await;
        self.0.store(true, Ordering::SeqCst);
        SessionEndReason::UserStop
    }
}

#[tokio::test]
async fn camera_sessions_keep_exclusive_physical_ownership_and_idempotent_teardown() {
    let stopped = Arc::new(AtomicBool::new(false));
    let first = RingAudioSessions::new(Arc::new(VideoRunner(Arc::clone(&stopped))));
    let other = RingAudioSessions::new(Arc::new(VideoRunner(Arc::new(AtomicBool::new(false)))));
    let request = AudioSessionRequest { expected_device_id: Some("51".into()),
        offer_sdp: "v=0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 0\r\na=sendrecv\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\na=rtpmap:96 H264/90000\r\na=recvonly\r\n".into(),
        mode: crate::ring_audio::AudioMode::Listen, ice_gathering_ms: None };
    let created = first
        .start("51".into(), request.clone())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(created.expires_in, 120);
    assert!(created.answer_sdp.contains("H264/90000"));
    let id = uuid::Uuid::parse_str(&created.session_id).unwrap_or_else(|e| panic!("{e}"));
    other
        .delete(id, SessionEndReason::UserStop)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(!stopped.load(Ordering::SeqCst));
    assert!(matches!(
        first.start("51".into(), request.clone()).await,
        Err(BridgeError::SessionBusy)
    ));
    first
        .delete(id, SessionEndReason::UserStop)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    first
        .delete(id, SessionEndReason::UserStop)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(stopped.load(Ordering::SeqCst));
    assert!(matches!(
        first.start("51".into(), request).await,
        Err(BridgeError::RateLimited)
    ));
}
