use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use vistoda_provider_kit::sealed_store::SealedFile;
use zeroize::Zeroizing;

use crate::{
    error::BridgeError,
    ring_sealed::{SESSION_LABEL, sealed_error, sealed_file},
};

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
    /// Loads the sealed session that replaces the plaintext file at `path`.
    pub fn load(path: &Path) -> Result<Self, BridgeError> {
        RingSessionStore::new(path.to_path_buf()).load()
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

/// The Ring session, sealed next to the configured plaintext path
/// (`ring_sealed`).
#[derive(Clone)]
pub struct RingSessionStore {
    file: SealedFile,
}

impl RingSessionStore {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self {
            file: sealed_file(path, SESSION_LABEL, MAX_SESSION_BYTES),
        }
    }

    pub fn load(&self) -> Result<RingSession, BridgeError> {
        let bytes = self
            .file
            .load()
            .map_err(sealed_error)?
            .ok_or_else(|| BridgeError::Io(std::io::ErrorKind::NotFound.into()))?;
        RingSession::parse(&bytes)
    }

    pub(crate) fn persist(&self, session: &RingSession) -> Result<(), BridgeError> {
        let document = SessionDocumentRef {
            schema_version: 1,
            hardware_id: session.hardware_id,
            refresh_token: session.refresh_token(),
        };
        let bytes = Zeroizing::new(serde_json::to_vec(&document)?);
        self.file.save(&bytes).map_err(sealed_error)
    }
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
