#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{collections::BTreeMap, fs};

use zeroize::Zeroizing;

use super::migrate_sealed_state;
use crate::{
    config::BridgeConfig,
    error::BridgeError,
    model::{DeviceConfig, DeviceKind},
    ring_push_store::{RingPushStore, StoredPush},
    ring_session::{RingSession, RingSessionStore},
};

const REFRESH: &str = "synthetic_refresh_token_1234567890abcdef";

fn write_private(path: &std::path::Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap_or_else(|error| panic!("write: {error}"));
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .unwrap_or_else(|error| panic!("chmod: {error}"));
}

#[test]
fn a_missing_key_fails_closed_instead_of_reading_as_not_enrolled() {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
    let store = RingSessionStore::new(directory.path().join("ring-session.json"));
    let session = RingSession::enrolled(uuid::Uuid::new_v4(), Zeroizing::new(REFRESH.into()))
        .unwrap_or_else(|error| panic!("session: {error}"));
    store
        .persist(&session)
        .unwrap_or_else(|error| panic!("persist: {error}"));
    fs::remove_file(directory.path().join("store.key"))
        .unwrap_or_else(|error| panic!("remove key: {error}"));
    // A fresh store, as after a restart: the key is not cached any more.
    let restarted = RingSessionStore::new(directory.path().join("ring-session.json"));
    match restarted.load() {
        Err(BridgeError::Configuration(message)) => assert!(message.contains("key")),
        Err(error) => panic!("unexpected error: {error}"),
        Ok(_) => panic!("a session without its key must not load"),
    }
    let absent = RingSessionStore::new(directory.path().join("other.json"));
    assert!(
        matches!(absent.load(), Err(BridgeError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound)
    );
}

#[test]
fn empty_or_oversized_plaintext_registrations_are_quarantined() {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
    let path = directory.path().join("ring-push.json");
    let store = RingPushStore::new(path.clone());
    for content in [Vec::new(), vec![b'x'; 200 * 1024]] {
        write_private(&path, &content);
        assert!(matches!(store.load(), Ok(StoredPush::Quarantined)));
        assert!(!path.exists());
        assert!(store.quarantine_path().exists());
        assert!(matches!(store.load(), Ok(StoredPush::Absent)));
    }
}

#[tokio::test]
async fn serve_startup_seals_the_plaintext_session() {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
    let session = directory.path().join("ring-session.json");
    let document = format!(
        "{{\"schema_version\":1,\"hardware_id\":\"{}\",\"refresh_token\":\"{REFRESH}\"}}",
        uuid::Uuid::new_v4()
    );
    write_private(&session, document.as_bytes());
    let devices = BTreeMap::from([(
        "entrance".into(),
        DeviceConfig {
            device_id: None,
            kind: DeviceKind::RingIntercomAudio,
        },
    )]);
    let config = BridgeConfig::new("127.0.0.1".into(), 8775, vec![b'a'; 32], devices)
        .unwrap_or_else(|error| panic!("config: {error}"))
        .with_session_file(session.clone())
        .with_push_file(directory.path().join("ring-push.json"));
    migrate_sealed_state(&config)
        .await
        .unwrap_or_else(|error| panic!("migrate: {error}"));
    assert!(!session.exists());
    assert!(directory.path().join("ring-session.sealed").exists());
    let loaded = RingSessionStore::new(session)
        .load()
        .unwrap_or_else(|error| panic!("load: {error}"));
    assert_eq!(loaded.refresh_token(), REFRESH);
}
