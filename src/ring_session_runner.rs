use crate::{
    BridgeError,
    ring_audio::{AudioSessionRequest, NegotiatedAudio, SessionEndReason, validate_request},
};
use tokio::sync::oneshot;

#[async_trait::async_trait]
pub trait SessionRunner: Send + Sync {
    fn validate(&self, request: &AudioSessionRequest) -> Result<(), BridgeError> {
        validate_request(request)
    }

    async fn run(
        &self,
        offer_sdp: String,
        expected_device_id: Option<String>,
        ready: oneshot::Sender<Result<NegotiatedAudio, BridgeError>>,
        cancel: oneshot::Receiver<()>,
    ) -> SessionEndReason;
}
