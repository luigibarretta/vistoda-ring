use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Take, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::error::BridgeError;

const MAX_SESSION_BYTES: usize = 16 * 1024;
const MIN_TOKEN_BYTES: usize = 32;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionDocument {
    schema_version: u8,
    hardware_id: Uuid,
    refresh_token: Zeroizing<String>,
}

#[derive(Serialize)]
struct SessionDocumentRef<'a> {
    schema_version: u8,
    hardware_id: Uuid,
    refresh_token: &'a str,
}

pub struct RingSession {
    hardware_id: Uuid,
    refresh_token: Zeroizing<String>,
}

impl RingSession {
    pub(crate) fn enrolled(
        hardware_id: Uuid,
        refresh_token: Zeroizing<String>,
    ) -> Result<Self, BridgeError> {
        validate_token(&refresh_token)?;
        Ok(Self {
            hardware_id,
            refresh_token,
        })
    }
    pub fn load(path: &Path) -> Result<Self, BridgeError> {
        let file = open_restricted(path)?;
        validate_permissions(&file)?;
        let bytes = read_bounded(file)?;
        Self::parse(&bytes)
    }

    pub fn parse(input: &[u8]) -> Result<Self, BridgeError> {
        if input.len() > MAX_SESSION_BYTES {
            return Err(configuration("session file exceeds 16 KiB"));
        }
        let document: SessionDocument = serde_json::from_slice(input)?;
        if document.schema_version != 1 {
            return Err(configuration("session schema version must be 1"));
        }
        validate_token(&document.refresh_token)?;
        Ok(Self {
            hardware_id: document.hardware_id,
            refresh_token: document.refresh_token,
        })
    }

    #[must_use]
    pub const fn hardware_id(&self) -> Uuid {
        self.hardware_id
    }

    pub(crate) fn refresh_token(&self) -> &str {
        self.refresh_token.as_str()
    }

    pub(crate) fn replace_refresh_token(
        &mut self,
        token: Zeroizing<String>,
    ) -> Result<(), BridgeError> {
        validate_token(&token)?;
        self.refresh_token = token;
        Ok(())
    }
}

pub struct RingSessionStore {
    path: PathBuf,
}

impl RingSessionStore {
    #[must_use]
    pub const fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Result<RingSession, BridgeError> {
        RingSession::load(&self.path)
    }

    pub(crate) fn persist(&self, session: &RingSession) -> Result<(), BridgeError> {
        let parent = self
            .path
            .parent()
            .filter(|value| !value.as_os_str().is_empty())
            .ok_or_else(|| configuration("session path must have a parent directory"))?;
        let name = self
            .path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| configuration("session filename is invalid"))?;
        let temporary = parent.join(format!(".{name}.{}.tmp", Uuid::new_v4()));
        let result = self.write_and_replace(session, parent, &temporary);
        if result.is_err() {
            let _ignored = fs::remove_file(&temporary);
        }
        result
    }

    fn write_and_replace(
        &self,
        session: &RingSession,
        parent: &Path,
        temporary: &Path,
    ) -> Result<(), BridgeError> {
        let document = SessionDocumentRef {
            schema_version: 1,
            hardware_id: session.hardware_id,
            refresh_token: session.refresh_token(),
        };
        let bytes = Zeroizing::new(serde_json::to_vec(&document)?);
        let mut file = create_private(temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(temporary, &self.path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    }
}

fn read_bounded(file: File) -> Result<Zeroizing<Vec<u8>>, BridgeError> {
    let mut bytes = Zeroizing::new(Vec::with_capacity(4 * 1024));
    let limit = u64::try_from(MAX_SESSION_BYTES + 1)
        .map_err(|_| configuration("session size limit is invalid"))?;
    let mut reader: Take<File> = file.take(limit);
    reader.read_to_end(&mut bytes)?;
    if bytes.len() > MAX_SESSION_BYTES {
        return Err(configuration("session file exceeds 16 KiB"));
    }
    Ok(bytes)
}

fn open_restricted(path: &Path) -> Result<File, BridgeError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    options.open(path).map_err(BridgeError::Io)
}

fn create_private(path: &Path) -> Result<File, BridgeError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600).custom_flags(libc::O_CLOEXEC);
    options.open(path).map_err(BridgeError::Io)
}

#[cfg(unix)]
fn validate_permissions(file: &File) -> Result<(), BridgeError> {
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(configuration(
            "session must be a regular file without group or other permissions",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_permissions(file: &File) -> Result<(), BridgeError> {
    if !file.metadata()?.is_file() {
        return Err(configuration("session must be a regular file"));
    }
    Ok(())
}

fn validate_token(token: &str) -> Result<(), BridgeError> {
    if token.len() < MIN_TOKEN_BYTES
        || token.len() > MAX_SESSION_BYTES
        || !token.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(configuration(
            "refresh token must be 32-16384 visible ASCII bytes",
        ));
    }
    Ok(())
}

fn configuration(message: &str) -> BridgeError {
    BridgeError::Configuration(message.into())
}
