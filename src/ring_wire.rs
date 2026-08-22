use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::Value;
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
    battery_life: Option<Value>,
    #[serde(default)]
    alerts: RawAlerts,
}

pub struct RingIntercomIdentity {
    id: u64,
    description: String,
    location_id: Option<String>,
    battery: Option<u8>,
    online: bool,
    doorbell_volume: Option<u8>,
    mic_volume: Option<u8>,
    voice_volume: Option<u8>,
}

#[derive(Default, Deserialize)]
struct RawSettings {
    #[serde(default, rename = "doorbell_volume")]
    doorbell: Option<u8>,
    #[serde(default, rename = "mic_volume")]
    mic: Option<u8>,
    #[serde(default, rename = "voice_volume")]
    voice: Option<u8>,
}

#[derive(Default, Deserialize)]
struct RawAlerts {
    #[serde(default)]
    connection: Option<String>,
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
    pub const fn battery(&self) -> Option<u8> {
        self.battery
    }

    #[must_use]
    pub const fn online(&self) -> bool {
        self.online
    }

    #[must_use]
    pub const fn volumes(&self) -> (Option<u8>, Option<u8>, Option<u8>) {
        (self.doorbell_volume, self.mic_volume, self.voice_volume)
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
        let battery = parse_battery(device.battery_life.as_ref())?;
        for value in [
            device.settings.doorbell,
            device.settings.mic,
            device.settings.voice,
        ]
        .into_iter()
        .flatten()
        {
            if value > 11 {
                return Err(BridgeError::Protocol("invalid Ring volume".into()));
            }
        }
        intercoms.push(RingIntercomIdentity {
            id: device.id,
            description: device.description,
            location_id: device.location_id,
            battery,
            online: device.alerts.connection.as_deref() != Some("offline"),
            doorbell_volume: device.settings.doorbell,
            mic_volume: device.settings.mic,
            voice_volume: device.settings.voice,
        });
    }
    Ok(intercoms)
}

fn parse_battery(value: Option<&Value>) -> Result<Option<u8>, BridgeError> {
    let parsed = match value {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Number(number)) => number.as_u64(),
        Some(Value::String(text)) => text.parse::<u64>().ok(),
        Some(_) => None,
    };
    match parsed {
        Some(level) => {
            Ok(Some(u8::try_from(level.min(100)).map_err(|_| {
                BridgeError::Protocol("invalid Ring battery".into())
            })?))
        }
        None => Err(BridgeError::Protocol("invalid Ring battery".into())),
    }
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
