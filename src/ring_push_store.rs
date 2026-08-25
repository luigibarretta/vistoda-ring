use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Take, Write},
    path::{Path, PathBuf},
};

use fcm_push_listener::Registration;
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::error::BridgeError;

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

impl RingPushState {
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

pub struct RingPushStore {
    path: PathBuf,
}

impl RingPushStore {
    pub const fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Result<Option<RingPushState>, BridgeError> {
        let file = match open_restricted(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        validate_permissions(&file)?;
        let document: PushDocument = serde_json::from_slice(&read_bounded(file)?)?;
        validate_document(&document)?;
        Ok(Some(RingPushState {
            registration: document.registration,
            persistent_ids: document.persistent_ids,
        }))
    }

    pub fn persist(&self, state: &RingPushState) -> Result<(), BridgeError> {
        let parent = self
            .path
            .parent()
            .filter(|value| !value.as_os_str().is_empty())
            .ok_or_else(|| configuration("push state path must have a parent"))?;
        let name = self
            .path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| configuration("push state filename is invalid"))?;
        let temporary = parent.join(format!(".{name}.{}.tmp", Uuid::new_v4()));
        let result = write_and_replace(&self.path, parent, &temporary, state);
        if result.is_err() {
            let _ignored = fs::remove_file(temporary);
        }
        result
    }
}

fn write_and_replace(
    target: &Path,
    parent: &Path,
    temporary: &Path,
    state: &RingPushState,
) -> Result<(), BridgeError> {
    let document = PushDocument {
        schema_version: 1,
        registration: state.registration.clone(),
        persistent_ids: state.persistent_ids.clone(),
    };
    let bytes = Zeroizing::new(serde_json::to_vec(&document)?);
    if bytes.len() > MAX_PUSH_BYTES {
        return Err(configuration("push state exceeds 64 KiB"));
    }
    let mut file = create_private(temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(temporary, target)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn read_bounded(file: File) -> Result<Zeroizing<Vec<u8>>, BridgeError> {
    let mut bytes = Zeroizing::new(Vec::with_capacity(4 * 1024));
    let limit = u64::try_from(MAX_PUSH_BYTES + 1)
        .map_err(|_| configuration("push state size limit is invalid"))?;
    let mut reader: Take<File> = file.take(limit);
    reader.read_to_end(&mut bytes)?;
    if bytes.len() > MAX_PUSH_BYTES {
        return Err(configuration("push state exceeds 64 KiB"));
    }
    Ok(bytes)
}

fn validate_document(document: &PushDocument) -> Result<(), BridgeError> {
    let token = &document.registration.fcm_token;
    if document.schema_version != 1
        || token.len() < 32
        || token.len() > 4096
        || !token.bytes().all(|byte| byte.is_ascii_graphic())
        || document.persistent_ids.len() > MAX_PERSISTENT_IDS
        || document
            .persistent_ids
            .iter()
            .any(|value| value.is_empty() || value.len() > 512 || !value.is_ascii())
    {
        return Err(configuration("push state failed validation"));
    }
    Ok(())
}

fn open_restricted(path: &Path) -> Result<File, std::io::Error> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    options.open(path)
}

fn create_private(path: &Path) -> Result<File, BridgeError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600).custom_flags(libc::O_CLOEXEC);
    Ok(options.open(path)?)
}

#[cfg(unix)]
fn validate_permissions(file: &File) -> Result<(), BridgeError> {
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(configuration("push state permissions are unsafe"));
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_permissions(file: &File) -> Result<(), BridgeError> {
    if !file.metadata()?.is_file() {
        return Err(configuration("push state must be a regular file"));
    }
    Ok(())
}

fn configuration(message: &str) -> BridgeError {
    BridgeError::Configuration(message.into())
}

#[cfg(test)]
mod tests {
    use super::{RingPushState, RingPushStore};
    use fcm_push_listener::{Registration, Session, WebPushKeys};
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn state() -> RingPushState {
        RingPushState {
            registration: Registration {
                fcm_token: "x".repeat(64),
                gcm: Session {
                    android_id: 123,
                    security_token: 456,
                },
                keys: WebPushKeys {
                    public_key: vec![1; 65],
                    private_key: vec![2; 32],
                    auth_secret: vec![3; 16],
                },
            },
            persistent_ids: vec!["one".into()],
        }
    }

    #[test]
    fn state_round_trip_is_private_and_bounded() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let path = directory.path().join("push.json");
        let store = RingPushStore::new(path.clone());
        store
            .persist(&state())
            .unwrap_or_else(|error| panic!("persist: {error}"));
        let loaded = store
            .load()
            .unwrap_or_else(|error| panic!("load: {error}"))
            .unwrap_or_else(|| panic!("state missing"));
        assert_eq!(loaded.registration.fcm_token.len(), 64);
        assert_eq!(loaded.persistent_ids, ["one"]);
        #[cfg(unix)]
        assert_eq!(
            std::fs::metadata(path)
                .unwrap_or_else(|error| panic!("metadata: {error}"))
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
