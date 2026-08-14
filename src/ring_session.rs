use std::{
    fs::{File, OpenOptions},
    io::{Read, Take},
    path::Path,
};

use serde::Deserialize;
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

pub struct RingSession {
    hardware_id: Uuid,
    refresh_token: Zeroizing<String>,
}

impl RingSession {
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
