use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::BridgeError;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct RecordingItem {
    pub recording_id: Uuid,
    pub started_at: i64,
    pub ended_at: i64,
    pub saved_at: i64,
    pub bytes: u64,
    pub media_type: String,
}

#[derive(Debug, Serialize)]
pub struct RecordingList {
    pub recordings: Vec<RecordingItem>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordingUploadQuery {
    pub started_at: i64,
    pub ended_at: i64,
}

impl RecordingUploadQuery {
    pub fn validate(&self) -> Result<(), BridgeError> {
        let current = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| BridgeError::InvalidRequest("system clock is invalid".into()))?
            .as_secs();
        let current = i64::try_from(current).unwrap_or(i64::MAX);
        let duration = self.ended_at.saturating_sub(self.started_at);
        if !(0..=180).contains(&duration)
            || self.started_at < current.saturating_sub(3600)
            || self.ended_at > current.saturating_add(60)
        {
            return Err(BridgeError::InvalidRequest(
                "recording timestamps are outside the bounded call window".into(),
            ));
        }
        Ok(())
    }
}
