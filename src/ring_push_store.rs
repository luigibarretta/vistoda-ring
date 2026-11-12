use std::path::{Path, PathBuf};

use fcm_push_listener::Registration;
use serde::{Deserialize, Serialize};
use vistoda_provider_kit::{
    private_file,
    sealed_store::{SealError, SealedFile},
};
use zeroize::Zeroizing;

use crate::{
    error::BridgeError,
    ring_sealed::{PUSH_LABEL, sealed_error, sealed_file},
};

const MAX_PUSH_BYTES: usize = 64 * 1024;
const MAX_PERSISTENT_IDS: usize = 100;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PushDocument {
    schema_version: u8,
    registration: Registration,
    persistent_ids: Vec<String>,
}

pub struct RingPushState {
    pub registration: Registration,
    pub persistent_ids: Vec<String>,
}

/// What `load` found. An unreadable document is moved aside, never deleted,
/// so a fresh registration can proceed while the evidence is kept.
pub enum StoredPush {
    Present(RingPushState),
    Absent,
    Quarantined,
}

impl RingPushState {
    pub const fn new(registration: Registration) -> Self {
        Self {
            registration,
            persistent_ids: Vec::new(),
        }
    }

    pub fn remember(&mut self, persistent_id: String) {
        if persistent_id.is_empty()
            || persistent_id.len() > 512
            || self.persistent_ids.contains(&persistent_id)
        {
            return;
        }
        self.persistent_ids.push(persistent_id);
        if self.persistent_ids.len() > MAX_PERSISTENT_IDS {
            self.persistent_ids.remove(0);
        }
    }
}

/// The FCM registration, sealed next to the configured plaintext path
/// (`ring_sealed`).
pub struct RingPushStore {
    file: SealedFile,
}

impl RingPushStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            file: sealed_file(path, PUSH_LABEL, MAX_PUSH_BYTES),
        }
    }

    /// Permission and I/O problems stay fail-closed; content that does not
    /// open (a lost or replaced `store.key`, a damaged envelope) or does not
    /// parse is quarantined: a registration can always be created again.
    pub fn load(&self) -> Result<StoredPush, BridgeError> {
        let bytes = match self.file.load() {
            Ok(Some(bytes)) => Some(bytes),
            Ok(None) => return Ok(StoredPush::Absent),
            Err(
                SealError::Authentication | SealError::Encoding | SealError::UnsupportedVersion(_),
            ) => None,
            Err(SealError::File(error)) if error.is_not_found() => None,
            Err(error) => return Err(sealed_error(error)),
        };
        let document = bytes
            .filter(|bytes| bytes.len() <= MAX_PUSH_BYTES)
            .and_then(|bytes| serde_json::from_slice::<PushDocument>(&bytes).ok())
            .filter(valid_document);
        let Some(document) = document else {
            self.quarantine()?;
            return Ok(StoredPush::Quarantined);
        };
        Ok(StoredPush::Present(RingPushState {
            registration: document.registration,
            persistent_ids: document.persistent_ids,
        }))
    }

    /// The sealed registration file.
    pub fn sealed_path(&self) -> &Path {
        self.file.path()
    }

    /// Path of the single quarantine slot; a newer corrupt file replaces it.
    pub fn quarantine_path(&self) -> PathBuf {
        let mut name = self
            .sealed_path()
            .file_name()
            .unwrap_or_default()
            .to_owned();
        name.push(".corrupt");
        self.sealed_path().with_file_name(name)
    }

    fn quarantine(&self) -> Result<(), BridgeError> {
        private_file::rename(self.sealed_path(), &self.quarantine_path())
            .map_err(|error| sealed_error(SealError::File(error)))?;
        tracing::warn!(
            "Ring push registration file was unreadable; moved it aside and registering again"
        );
        Ok(())
    }

    pub fn persist(&self, state: &RingPushState) -> Result<(), BridgeError> {
        let document = PushDocument {
            schema_version: 1,
            registration: state.registration.clone(),
            persistent_ids: state.persistent_ids.clone(),
        };
        let bytes = Zeroizing::new(serde_json::to_vec(&document)?);
        if bytes.len() > MAX_PUSH_BYTES {
            return Err(configuration("push state exceeds 64 KiB"));
        }
        self.file.save(&bytes).map_err(sealed_error)
    }
}

fn valid_document(document: &PushDocument) -> bool {
    let token = &document.registration.fcm_token;
    document.schema_version == 1
        && (32..=4096).contains(&token.len())
        && token.bytes().all(|byte| byte.is_ascii_graphic())
        && document.persistent_ids.len() <= MAX_PERSISTENT_IDS
        && document
            .persistent_ids
            .iter()
            .all(|value| !value.is_empty() && value.len() <= 512 && value.is_ascii())
}

fn configuration(message: &str) -> BridgeError {
    BridgeError::Configuration(message.into())
}

#[cfg(test)]
#[path = "ring_push_store_tests.rs"]
pub mod tests;
