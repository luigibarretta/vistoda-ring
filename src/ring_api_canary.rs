use std::{path::Path, time::Duration};

use reqwest::{Client, StatusCode, Url};
use serde::Serialize;
use zeroize::Zeroizing;

use crate::{
    BridgeError,
    ring_audio::{AudioMode, AudioSessionCreated, AudioSessionRequest},
    ring_http::checked_body,
    ring_media_peer::MediaPeer,
};

const BODY_LIMIT: usize = 128 * 1024;

#[derive(Debug, Serialize)]
pub struct ApiCanaryEvidence {
    protocol: &'static str,
    connected: bool,
    codec: Option<String>,
    received_packets: u64,
    received_bytes: u64,
    silent_packets_sent: u64,
    delete_status: u16,
    teardown_complete: bool,
}

impl ApiCanaryEvidence {
    #[must_use]
    pub fn passes_release_gate(&self) -> bool {
        self.connected
            && self.codec.as_deref() == Some("audio/PCMU")
            && self.received_packets > 0
            && self.received_bytes > 0
            && self.silent_packets_sent > 0
            && self.delete_status == StatusCode::NO_CONTENT.as_u16()
            && self.teardown_complete
    }
}

pub async fn run_api_canary(
    base_url: &str,
    token_file: &Path,
    duration: Duration,
) -> Result<ApiCanaryEvidence, BridgeError> {
    if !(Duration::from_secs(5)..=Duration::from_secs(30)).contains(&duration) {
        return Err(protocol("API canary duration must be 5-30 seconds"));
    }
    let base = local_base_url(base_url)?;
    let token = read_token(token_file).await?;
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| BridgeError::Transport("API canary setup", error))?;
    let peer = Box::pin(MediaPeer::new()).await?;
    let offer = peer.offer().await?;
    let endpoint = base
        .join("v1/devices/entrance/audio/sessions")
        .map_err(|_| protocol("API canary session URL is invalid"))?;
    let response = client
        .post(endpoint.clone())
        .bearer_auth(token.as_str())
        .json(&AudioSessionRequest {
            offer_sdp: offer,
            mode: AudioMode::Listen,
            ice_gathering_ms: None,
        })
        .send()
        .await
        .map_err(|error| BridgeError::Transport("API canary start", error))?;
    let body = checked_body(response, "API canary start", BODY_LIMIT).await?;
    let session: AudioSessionCreated = serde_json::from_slice(&body)?;
    let result = apply_session(&peer, &session, duration).await;
    let delete_status = delete_session(&client, &endpoint, &token, &session.session_id).await;
    let peer_closed = peer.close().await.is_ok();
    result?;
    let media = peer.snapshot().await;
    Ok(ApiCanaryEvidence {
        protocol: "ring_consumer_webrtc_audio_v1",
        connected: media.connected,
        codec: media.codec,
        received_packets: media.received_packets,
        received_bytes: media.received_bytes,
        silent_packets_sent: media.silent_packets,
        delete_status: delete_status?,
        teardown_complete: peer_closed,
    })
}

async fn apply_session(
    peer: &MediaPeer,
    session: &AudioSessionCreated,
    duration: Duration,
) -> Result<(), BridgeError> {
    peer.accept_answer(session.answer_sdp.clone()).await?;
    for candidate in &session.ice_candidates {
        peer.add_ice(candidate.candidate.clone(), candidate.sdp_mline_index)
            .await?;
    }
    peer.start_silence(duration);
    tokio::time::sleep(duration).await;
    Ok(())
}

async fn delete_session(
    client: &Client,
    endpoint: &Url,
    token: &str,
    session_id: &str,
) -> Result<u16, BridgeError> {
    let target = endpoint
        .join(&format!("sessions/{session_id}"))
        .map_err(|_| protocol("API canary delete URL is invalid"))?;
    let response = client
        .delete(target)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|error| BridgeError::Transport("API canary delete", error))?;
    let status = response.status().as_u16();
    checked_body(response, "API canary delete", BODY_LIMIT).await?;
    Ok(status)
}

fn local_base_url(value: &str) -> Result<Url, BridgeError> {
    let url = Url::parse(value).map_err(|_| protocol("API canary URL is invalid"))?;
    if url.scheme() != "http"
        || !url
            .host_str()
            .is_some_and(|host| matches!(host, "127.0.0.1" | "localhost" | "::1"))
        || url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(protocol("API canary URL must be a loopback HTTP origin"));
    }
    Ok(url)
}

async fn read_token(path: &Path) -> Result<Zeroizing<String>, BridgeError> {
    let metadata = tokio::fs::symlink_metadata(path).await?;
    if !metadata.is_file() || metadata.len() > 4096 {
        return Err(protocol("API canary token file is invalid"));
    }
    let bytes = tokio::fs::read(path).await?;
    let value = String::from_utf8(bytes).map_err(|_| protocol("API canary token is invalid"))?;
    let value = Zeroizing::new(value.trim().to_owned());
    if value.len() < 32 {
        return Err(protocol("API canary token is invalid"));
    }
    Ok(value)
}

fn protocol(message: &str) -> BridgeError {
    BridgeError::Protocol(message.into())
}

#[cfg(test)]
#[path = "ring_api_canary_tests.rs"]
mod tests;
