use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub(crate) struct EventEnvelope {
    #[serde(default)]
    pub events: Vec<RawEvent>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawEvent {
    pub recording_status: Option<String>,
    pub state: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RecordingEvidence {
    pub recording_enabled: bool,
    pub recordings_visible: bool,
    pub location_available: bool,
    pub recent_events: usize,
    pub ready_recordings: usize,
}
