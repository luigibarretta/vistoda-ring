use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::BridgeError;

#[derive(Clone, Debug)]
pub(crate) struct ProviderRecording {
    pub ding_id: String,
    pub created_at: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct EventEnvelope {
    #[serde(default)]
    pub events: Vec<RawEvent>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawEvent {
    pub ding_id_str: String,
    pub created_at: String,
    pub recording_status: Option<String>,
    pub state: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RecordingUrl {
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct RecordingEvidence {
    pub recording_enabled: bool,
    pub recordings_visible: bool,
    pub location_available: bool,
    pub recent_events: usize,
    pub ready_recordings: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordingImportRequest {
    pub triggered_at: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingImportState {
    Pending,
    Complete,
    Unavailable,
    Expired,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
pub struct RecordingImport {
    pub import_id: Uuid,
    pub triggered_at: i64,
    pub state: RecordingImportState,
    pub recording_id: Option<Uuid>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct RecordingItem {
    pub recording_id: Uuid,
    pub triggered_at: i64,
    pub event_at: i64,
    pub saved_at: i64,
    pub bytes: u64,
    pub media_type: String,
}

#[derive(Debug, Serialize)]
pub struct RecordingList {
    pub recordings: Vec<RecordingItem>,
}

pub(crate) fn parse_created_at(value: &str) -> Result<i64, BridgeError> {
    time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        .map(time::OffsetDateTime::unix_timestamp)
        .map_err(|_| BridgeError::Protocol("Ring event timestamp is invalid".into()))
}

pub(crate) fn valid_provider_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}
