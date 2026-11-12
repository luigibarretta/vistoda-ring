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

use crate::{
    config::BridgeConfig, error::BridgeError, ring_push_store::RingPushStore,
    ring_session::RingSessionStore,
};

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

/// Only a missing sealed file reads as "not enrolled" (an I/O `NotFound`);
/// a sealed file whose `store.key` is missing, and every other refusal, is a
/// configuration error, so a new enrollment never silently replaces it.
pub fn sealed_error(error: SealError) -> BridgeError {
    match error {
        SealError::File(error) if error.is_not_found() => {
            BridgeError::Io(std::io::ErrorKind::NotFound.into())
        }
        error => BridgeError::Configuration(format!("Ring secret store: {error}")),
    }
}

/// Seals the plaintext state of earlier releases when `serve` starts.
///
/// This runs before any request reads it. A session that cannot be migrated
/// stops startup; a push registration problem is only logged (the push service
/// quarantines or reports it and registers again).
pub async fn migrate_sealed_state(config: &BridgeConfig) -> Result<(), BridgeError> {
    let session = RingSessionStore::new(config.session_file.clone());
    let push = RingPushStore::new(config.push_file.clone());
    tokio::task::spawn_blocking(move || {
        if let Err(error) = push.load() {
            tracing::warn!(%error, "Ring push registration could not be loaded at startup");
        }
        session.migrate()
    })
    .await
    .map_err(|error| BridgeError::Io(std::io::Error::other(error)))?
}

#[cfg(test)]
#[path = "ring_sealed_tests.rs"]
mod tests;
