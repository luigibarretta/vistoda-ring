use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::oneshot;
use uuid::Uuid;

use super::{RingAudioSessions, SessionRunner};
use crate::{
    BridgeError,
    ring_audio::{AudioMode, AudioSessionRequest, IceCandidate, NegotiatedAudio},
};

const OFFER: &str = "v=0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 0\r\na=sendrecv\r\n";

struct FakeRunner;

#[async_trait]
impl SessionRunner for FakeRunner {
    async fn run(
        &self,
        _offer_sdp: String,
        ready: oneshot::Sender<Result<NegotiatedAudio, BridgeError>>,
        cancel: oneshot::Receiver<()>,
    ) {
        let _ = ready.send(Ok(NegotiatedAudio {
            answer_sdp: "v=0\r\nm=audio 9 RTP/AVP 0\r\na=sendrecv\r\n".into(),
            ice_candidates: vec![IceCandidate {
                candidate: "candidate:synthetic".into(),
                sdp_mline_index: 0,
            }],
        }));
        let _ = cancel.await;
    }
}

fn request() -> AudioSessionRequest {
    AudioSessionRequest {
        offer_sdp: OFFER.into(),
        mode: AudioMode::Listen,
    }
}

#[tokio::test]
async fn one_device_session_is_exclusive_and_delete_is_idempotent() {
    let sessions = RingAudioSessions::new(Arc::new(FakeRunner));
    let created = sessions
        .start("entrance".into(), request())
        .await
        .unwrap_or_else(|error| panic!("session failed: {error}"));
    assert!(sessions.start("entrance".into(), request()).await.is_err());
    let id = Uuid::parse_str(&created.session_id)
        .unwrap_or_else(|error| panic!("session id failed: {error}"));
    sessions.delete(id).await;
    sessions.delete(id).await;
    tokio::task::yield_now().await;
    assert!(sessions.start("entrance".into(), request()).await.is_err());
    assert!(sessions.start("other".into(), request()).await.is_ok());
}

#[tokio::test]
async fn invalid_offer_never_reserves_the_device() {
    let sessions = RingAudioSessions::new(Arc::new(FakeRunner));
    let mut invalid = request();
    invalid.offer_sdp = "v=0\r\nm=video 9 RTP/AVP 96".into();
    assert!(sessions.start("entrance".into(), invalid).await.is_err());
    assert!(sessions.start("entrance".into(), request()).await.is_ok());
}
