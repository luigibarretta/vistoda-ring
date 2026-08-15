use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeviceConfig {
    pub kind: DeviceKind,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    RingIntercomAudio,
    RingIntercomVideo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaCapability {
    LiveAudioReceive,
    LiveAudioTransmit,
    LiveVideoReceive,
    Recordings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityPhase {
    ProtocolResearch,
    Verified,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MediaCapabilities {
    pub available: Vec<MediaCapability>,
    pub phase: CapabilityPhase,
}

impl MediaCapabilities {
    #[must_use]
    pub const fn research_only() -> Self {
        Self {
            available: Vec::new(),
            phase: CapabilityPhase::ProtocolResearch,
        }
    }

    #[must_use]
    pub fn verified_audio() -> Self {
        Self {
            available: vec![
                MediaCapability::LiveAudioReceive,
                MediaCapability::LiveAudioTransmit,
            ],
            phase: CapabilityPhase::Verified,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct DeviceSummary {
    pub alias: String,
    pub kind: DeviceKind,
    pub capabilities: MediaCapabilities,
}
