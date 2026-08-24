use std::path::Path;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingStorageKind {
    Private,
    AddonConfig,
    Media,
    Share,
    Custom,
}

impl RecordingStorageKind {
    pub fn parse(value: &str) -> Result<Self, BridgeError> {
        match value {
            "private" => Ok(Self::Private),
            "addon_config" => Ok(Self::AddonConfig),
            "media" => Ok(Self::Media),
            "share" => Ok(Self::Share),
            "custom" => Ok(Self::Custom),
            _ => Err(BridgeError::Configuration(
                "recording storage kind is unsupported".into(),
            )),
        }
    }

    #[must_use]
    pub const fn user_visible(self) -> bool {
        !matches!(self, Self::Private)
    }
}

#[derive(Debug, Serialize)]
pub struct RecordingStorage {
    pub kind: RecordingStorageKind,
    pub directory: String,
    pub user_visible: bool,
}

#[derive(Debug, Serialize)]
pub struct ArchivedRecording {
    #[serde(flatten)]
    pub item: RecordingItem,
    pub storage_path: String,
}

#[derive(Debug, Serialize)]
pub struct RecordingList {
    pub storage: RecordingStorage,
    pub recordings: Vec<ArchivedRecording>,
}

pub fn storage_path(directory: &str, item: &RecordingItem) -> Result<String, BridgeError> {
    let extension =
        crate::ring_recording_media::RecordingMedia::parse(&item.media_type)?.extension();
    Ok(Path::new(directory)
        .join(format!("{}.{}", item.recording_id, extension))
        .to_string_lossy()
        .into_owned())
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
