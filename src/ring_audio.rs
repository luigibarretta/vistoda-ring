use serde::{Deserialize, Serialize};

use crate::BridgeError;

pub const SESSION_SECONDS: u64 = 120;
const SDP_LIMIT: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioMode {
    Listen,
    Talk,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioSessionRequest {
    pub offer_sdp: String,
    pub mode: AudioMode,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IceCandidate {
    pub candidate: String,
    pub sdp_mline_index: u16,
}

#[derive(Clone, Debug, Serialize)]
pub struct AudioSessionCreated {
    pub session_id: String,
    pub answer_sdp: String,
    pub ice_candidates: Vec<IceCandidate>,
    pub mode: AudioMode,
    pub expires_in: u64,
}

pub(crate) struct NegotiatedAudio {
    pub answer_sdp: String,
    pub ice_candidates: Vec<IceCandidate>,
}

pub fn validate_offer(sdp: &str) -> Result<(), BridgeError> {
    if sdp.is_empty() || sdp.len() > SDP_LIMIT || sdp.contains('\0') {
        return Err(invalid("SDP offer is empty or exceeds the limit"));
    }
    let lines: Vec<_> = sdp.lines().map(str::trim).collect();
    if lines.first().copied() != Some("v=0") {
        return Err(invalid("SDP offer has no version line"));
    }
    let audio: Vec<_> = lines
        .iter()
        .filter(|line| line.starts_with("m=audio "))
        .collect();
    if audio.len() != 1 || lines.iter().any(|line| line.starts_with("m=video ")) {
        return Err(invalid(
            "SDP offer must contain exactly one audio media section",
        ));
    }
    if !audio[0]
        .split_ascii_whitespace()
        .skip(3)
        .any(|payload| payload == "0")
    {
        return Err(invalid("SDP offer must advertise PCMU payload 0"));
    }
    if lines.iter().any(|line| line.starts_with("m=application ")) {
        return Err(invalid("SDP data channels are not admitted"));
    }
    if !audio_direction(sdp, "a=sendrecv") {
        return Err(invalid("SDP audio must be sendrecv"));
    }
    Ok(())
}

pub(crate) fn validate_answer(sdp: &str) -> Result<(), BridgeError> {
    if sdp.len() > SDP_LIMIT || !audio_direction(sdp, "a=sendrecv") {
        return Err(BridgeError::Protocol(
            "Ring answer did not negotiate bounded bidirectional audio".into(),
        ));
    }
    Ok(())
}

fn audio_direction(sdp: &str, expected: &str) -> bool {
    let mut in_audio = false;
    for line in sdp.lines().map(str::trim) {
        if line.starts_with("m=") {
            in_audio = line.starts_with("m=audio ");
        } else if in_audio && line == expected {
            return true;
        }
    }
    false
}

fn invalid(message: &str) -> BridgeError {
    BridgeError::InvalidRequest(message.into())
}

#[cfg(test)]
#[path = "ring_audio_tests.rs"]
mod tests;
