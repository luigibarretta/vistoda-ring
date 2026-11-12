//! Ring secrets at rest, sealed with the provider kit
//! (`vistoda_provider_kit::sealed_store`).
//!
//! `ring-session.json` (Ring OAuth refresh token) and `ring-push.json` (FCM
//! registration) used to be plaintext. Each now lives in a sibling `.sealed`
//! file (AES-256-GCM under its own HKDF sub-key of the random `store.key` in
//! the same directory). A plaintext file left by an earlier release is sealed
//! on first use, read back and only then removed. The configured paths keep
//! naming the plaintext files, so deployments do not change.

use std::path::PathBuf;

use vistoda_provider_kit::{
    KeyLabel,
    sealed_store::{KeyFile, SealError, SealedFile},
};

use crate::error::BridgeError;

pub const SESSION_LABEL: KeyLabel = KeyLabel::new("vistoda-ring/session/v1");
pub const PUSH_LABEL: KeyLabel = KeyLabel::new("vistoda-ring/push/v1");

/// The sealed file replacing the plaintext file at `path`.
pub fn sealed_file(path: PathBuf, label: KeyLabel, max_plaintext: usize) -> SealedFile {
    let sealed = path.with_extension("sealed");
    // Base64 and the envelope roughly add a third; leave room for both.
    let max_bytes = u64::try_from(max_plaintext.saturating_mul(2)).unwrap_or(u64::MAX);
    SealedFile::new(sealed.clone(), KeyFile::beside(&sealed, label))
        .migrating_plaintext(path)
        .with_max_bytes(max_bytes)
}

/// A missing file stays an I/O `NotFound` (callers treat it as "not
/// enrolled"); every other refusal is a configuration error.
pub fn sealed_error(error: SealError) -> BridgeError {
    match error {
        SealError::File(error) if error.is_not_found() => {
            BridgeError::Io(std::io::ErrorKind::NotFound.into())
        }
        error => BridgeError::Configuration(format!("Ring secret store: {error}")),
    }
}
