use std::{fs, io::Write, path::PathBuf};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use uuid::Uuid;

use crate::{
    error::BridgeError,
    ring_recording::RecordingItem,
    ring_recording_media::{MAX_MEDIA_BYTES, RecordingMedia},
};

const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
const RETENTION_SECONDS: i64 = 30 * 24 * 60 * 60;

pub struct RecordingStore {
    root: PathBuf,
}

impl RecordingStore {
    pub fn new(root: PathBuf) -> Result<Self, BridgeError> {
        if root.exists() {
            validate_root(&root)?;
        }
        Ok(Self { root })
    }

    pub fn commit(
        &self,
        started_at: i64,
        ended_at: i64,
        content_type: &str,
        media: &[u8],
    ) -> Result<RecordingItem, BridgeError> {
        self.ensure_root()?;
        let kind = RecordingMedia::parse(content_type)?;
        kind.validate(media)?;
        let id = Uuid::new_v4();
        let saved_at = now()?;
        let item = RecordingItem {
            recording_id: id,
            started_at,
            ended_at,
            saved_at,
            bytes: u64::try_from(media.len())
                .map_err(|_| BridgeError::Protocol("recording is too large".into()))?,
            media_type: kind.content_type().into(),
        };
        self.atomic_write(&format!("{id}.{}", kind.extension()), media)?;
        if let Err(error) = self.atomic_write(&format!("{id}.json"), &serde_json::to_vec(&item)?) {
            let _ignored = fs::remove_file(self.media_path(id, kind));
            return Err(error);
        }
        self.cleanup(saved_at)?;
        Ok(item)
    }

    pub fn list(&self) -> Result<Vec<RecordingItem>, BridgeError> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        validate_root(&self.root)?;
        let mut items = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let path = entry?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let metadata = fs::symlink_metadata(&path)?;
            if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 4096 {
                return Err(BridgeError::Protocol("recording manifest is unsafe".into()));
            }
            let item: RecordingItem = serde_json::from_slice(&fs::read(path)?)?;
            validate_item(&item)?;
            items.push(item);
        }
        items.sort_by_key(|item| std::cmp::Reverse(item.ended_at));
        Ok(items)
    }

    pub fn read(&self, id: Uuid) -> Result<(String, Vec<u8>), BridgeError> {
        if !self.root.exists() {
            return Err(BridgeError::RecordingNotFound);
        }
        let item = self.item(id)?;
        let kind = RecordingMedia::parse(&item.media_type)?;
        let path = self.media_path(id, kind);
        let metadata = fs::symlink_metadata(&path).map_err(not_found)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > MAX_MEDIA_BYTES as u64
        {
            return Err(BridgeError::RecordingNotFound);
        }
        let media = fs::read(path)?;
        kind.validate(&media)?;
        Ok((item.media_type, media))
    }

    pub fn delete(&self, id: Uuid) -> Result<(), BridgeError> {
        if !self.root.exists() {
            return Ok(());
        }
        remove_if_present(self.root.join(format!("{id}.mp4")))?;
        remove_if_present(self.root.join(format!("{id}.webm")))?;
        remove_if_present(self.root.join(format!("{id}.json")))
    }

    fn item(&self, id: Uuid) -> Result<RecordingItem, BridgeError> {
        let path = self.root.join(format!("{id}.json"));
        let metadata = fs::symlink_metadata(&path).map_err(not_found)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 4096 {
            return Err(BridgeError::RecordingNotFound);
        }
        let item = serde_json::from_slice::<RecordingItem>(&fs::read(path)?)?;
        validate_item(&item)?;
        Ok(item)
    }

    fn atomic_write(&self, name: &str, body: &[u8]) -> Result<(), BridgeError> {
        let temporary = self.root.join(format!(".{}.tmp", Uuid::new_v4()));
        let target = self.root.join(name);
        let mut options = fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&temporary)?;
        if let Err(error) = file.write_all(body).and_then(|()| file.sync_all()) {
            let _ignored = fs::remove_file(&temporary);
            return Err(error.into());
        }
        fs::rename(temporary, target)?;
        Ok(())
    }

    fn cleanup(&self, current: i64) -> Result<(), BridgeError> {
        let mut items = self.list()?;
        let mut total = items.iter().map(|item| item.bytes).sum::<u64>();
        items.sort_by_key(|item| item.saved_at);
        for item in items {
            if current.saturating_sub(item.saved_at) > RETENTION_SECONDS
                || total > MAX_ARCHIVE_BYTES
            {
                self.delete(item.recording_id)?;
                total = total.saturating_sub(item.bytes);
            }
        }
        Ok(())
    }

    fn media_path(&self, id: Uuid, kind: RecordingMedia) -> PathBuf {
        self.root.join(format!("{id}.{}", kind.extension()))
    }

    fn ensure_root(&self) -> Result<(), BridgeError> {
        fs::create_dir_all(&self.root)?;
        validate_root(&self.root)?;
        #[cfg(unix)]
        fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))?;
        Ok(())
    }
}

fn validate_root(root: &std::path::Path) -> Result<(), BridgeError> {
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(BridgeError::Configuration(
            "recording directory must be a real directory".into(),
        ));
    }
    Ok(())
}

fn validate_item(item: &RecordingItem) -> Result<(), BridgeError> {
    let _kind = RecordingMedia::parse(&item.media_type)?;
    if item.bytes > MAX_MEDIA_BYTES as u64 || item.started_at > item.ended_at {
        return Err(BridgeError::Protocol(
            "recording manifest is invalid".into(),
        ));
    }
    Ok(())
}

fn remove_if_present(path: PathBuf) -> Result<(), BridgeError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn not_found(error: std::io::Error) -> BridgeError {
    if error.kind() == std::io::ErrorKind::NotFound {
        BridgeError::RecordingNotFound
    } else {
        error.into()
    }
}

fn now() -> Result<i64, BridgeError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| i64::try_from(value.as_secs()).unwrap_or(i64::MAX))
        .map_err(|_| BridgeError::Protocol("system clock is invalid".into()))
}

#[cfg(test)]
#[path = "ring_recording_store_tests.rs"]
mod tests;
