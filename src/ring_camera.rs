//! Native Ring camera inventory, kept separate from entrance controls.
use crate::{
    BridgeError,
    model::{CapabilityPhase, MediaCapabilities, MediaCapability},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_CAMERAS: usize = 32;

#[derive(Default, Deserialize)]
struct Envelope {
    #[serde(default)]
    doorbots: Vec<RawCamera>,
    #[serde(default)]
    stickup_cams: Vec<RawCamera>,
    #[serde(default)]
    authorized_doorbots: Vec<RawCamera>,
    #[serde(default)]
    other: Vec<OtherDevice>,
}

#[derive(Deserialize)]
struct OtherDevice {
    id: u64,
}

#[derive(Deserialize)]
struct RawCamera {
    id: u64,
    kind: String,
    description: String,
    #[serde(default)]
    location_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CameraInventory {
    pub cameras: Vec<CameraSummary>,
}

#[derive(Debug, Serialize)]
pub struct CameraSummary {
    pub device_id: String,
    pub name: String,
    pub model: String,
    pub location_name: Option<String>,
    pub capabilities: MediaCapabilities,
    #[serde(skip)]
    location_id: Option<String>,
}

pub fn parse(input: &[u8]) -> Result<CameraInventory, BridgeError> {
    let envelope: Envelope = serde_json::from_slice(input)?;
    let count =
        envelope.doorbots.len() + envelope.stickup_cams.len() + envelope.authorized_doorbots.len();
    if count > MAX_CAMERAS || count + envelope.other.len() > 512 {
        return Err(protocol());
    }
    let mut ids: BTreeSet<_> = envelope.other.into_iter().map(|device| device.id).collect();
    let mut cameras = Vec::new();
    for device in envelope
        .doorbots
        .into_iter()
        .chain(envelope.stickup_cams)
        .chain(envelope.authorized_doorbots)
    {
        if device.id == 0
            || !ids.insert(device.id)
            || !safe_text(&device.description)
            || !safe_text(&device.kind)
            || device.kind.starts_with("intercom_")
        {
            return Err(protocol());
        }
        cameras.push(CameraSummary {
            device_id: device.id.to_string(),
            name: device.description,
            model: device.kind,
            location_id: device.location_id,
            location_name: None,
            capabilities: capabilities(),
        });
    }
    cameras.sort_by_key(|camera| camera.device_id.parse::<u64>().unwrap_or_default());
    Ok(CameraInventory { cameras })
}

impl CameraInventory {
    pub fn locations(&mut self, body: &[u8]) {
        let names = crate::ring_inventory::location_names(body).unwrap_or_default();
        for camera in &mut self.cameras {
            camera.location_name = camera
                .location_id
                .as_ref()
                .and_then(|id| names.get(id))
                .cloned();
        }
    }
}

pub fn capabilities() -> MediaCapabilities {
    MediaCapabilities {
        available: vec![
            MediaCapability::LiveVideoReceive,
            MediaCapability::LiveAudioReceive,
            MediaCapability::LiveAudioTransmit,
        ],
        phase: CapabilityPhase::ProtocolResearch,
    }
}

fn safe_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

fn protocol() -> BridgeError {
    BridgeError::Protocol("invalid or oversized Ring camera inventory".into())
}
