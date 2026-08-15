use std::collections::BTreeSet;

use serde::Deserialize;
use zeroize::Zeroizing;

use crate::error::BridgeError;

const MAX_DEVICES: usize = 512;
const MAX_DESCRIPTION_BYTES: usize = 128;

#[derive(Deserialize)]
pub struct OAuthResponse {
    pub access_token: Zeroizing<String>,
    pub expires_in: u64,
    pub refresh_token: Zeroizing<String>,
    pub scope: String,
    pub token_type: String,
}

#[derive(Deserialize)]
struct DeviceEnvelope {
    #[serde(default)]
    other: Vec<RawDevice>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TicketResponse {
    ticket: Zeroizing<String>,
    #[serde(default, rename = "responseTimestampe")]
    response_timestamp: Option<u64>,
}

#[derive(Deserialize)]
struct RawDevice {
    id: u64,
    kind: String,
    description: String,
    #[serde(default)]
    location_id: Option<String>,
    #[serde(default)]
    settings: RawSettings,
    #[serde(default)]
    features: RawFeatures,
}

pub struct RingIntercomIdentity {
    id: u64,
    description: String,
    location_id: Option<String>,
    recording_enabled: bool,
    recordings_visible: bool,
}

#[derive(Default, Deserialize)]
struct RawSettings {
    #[serde(default)]
    recording_enabled: bool,
    #[serde(default)]
    show_recordings: bool,
}

#[derive(Default, Deserialize)]
struct RawFeatures {
    #[serde(default)]
    show_recordings: bool,
}

impl RingIntercomIdentity {
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    #[must_use]
    pub fn location_id(&self) -> Option<&str> {
        self.location_id.as_deref()
    }

    #[must_use]
    pub const fn recording_enabled(&self) -> bool {
        self.recording_enabled
    }

    #[must_use]
    pub const fn recordings_visible(&self) -> bool {
        self.recordings_visible
    }
}

pub fn parse_oauth(input: &[u8]) -> Result<OAuthResponse, BridgeError> {
    let response: OAuthResponse = serde_json::from_slice(input)?;
    if response.expires_in < 120
        || response.expires_in > 86_400
        || response.scope != "client"
        || !response.token_type.eq_ignore_ascii_case("bearer")
        || !valid_token(&response.access_token)
        || !valid_token(&response.refresh_token)
    {
        return Err(BridgeError::Protocol("invalid OAuth response".into()));
    }
    Ok(response)
}

pub fn parse_devices(input: &[u8]) -> Result<Vec<RingIntercomIdentity>, BridgeError> {
    let envelope: DeviceEnvelope = serde_json::from_slice(input)?;
    if envelope.other.len() > MAX_DEVICES {
        return Err(BridgeError::Protocol(
            "device inventory is too large".into(),
        ));
    }
    let mut ids = BTreeSet::new();
    let mut intercoms = Vec::new();
    for device in envelope
        .other
        .into_iter()
        .filter(|device| device.kind == "intercom_handset_audio")
    {
        if device.id == 0 || !ids.insert(device.id) || !valid_description(&device.description) {
            return Err(BridgeError::Protocol(
                "invalid Ring Intercom identity".into(),
            ));
        }
        intercoms.push(RingIntercomIdentity {
            id: device.id,
            description: device.description,
            location_id: device.location_id,
            recording_enabled: device.settings.recording_enabled,
            recordings_visible: device.settings.show_recordings || device.features.show_recordings,
        });
    }
    Ok(intercoms)
}

pub fn parse_ticket(input: &[u8]) -> Result<Zeroizing<String>, BridgeError> {
    let response: TicketResponse = serde_json::from_slice(input)?;
    if !valid_token(&response.ticket) || response.response_timestamp == Some(0) {
        return Err(BridgeError::Protocol("invalid stream ticket".into()));
    }
    Ok(response.ticket)
}

fn valid_token(token: &str) -> bool {
    (32..=16 * 1024).contains(&token.len()) && token.bytes().all(|byte| byte.is_ascii_graphic())
}

fn valid_description(description: &str) -> bool {
    !description.is_empty()
        && description.len() <= MAX_DESCRIPTION_BYTES
        && !description.chars().any(char::is_control)
}
