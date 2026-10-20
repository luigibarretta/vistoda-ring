use super::{RingPushState, RingPushStore, StoredPush};
use fcm_push_listener::{Registration, Session, WebPushKeys};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

pub fn state() -> RingPushState {
    state_with("x")
}

/// A valid state whose FCM token repeats `token` (one character) 64 times.
pub fn state_with(token: &str) -> RingPushState {
    RingPushState {
        registration: Registration {
            fcm_token: token.repeat(64),
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

fn present(store: &RingPushStore) -> RingPushState {
    match store.load() {
        Ok(StoredPush::Present(state)) => state,
        Ok(_) => panic!("state missing"),
        Err(error) => panic!("load: {error}"),
    }
}

#[test]
fn state_round_trip_is_private_and_bounded() {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
    let path = directory.path().join("push.json");
    let store = RingPushStore::new(path.clone());
    assert!(matches!(store.load(), Ok(StoredPush::Absent)));
    store
        .persist(&state())
        .unwrap_or_else(|error| panic!("persist: {error}"));
    let loaded = present(&store);
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

#[test]
fn persist_atomically_replaces_the_previous_registration() {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
    let store = RingPushStore::new(directory.path().join("push.json"));
    store
        .persist(&state_with("a"))
        .unwrap_or_else(|error| panic!("persist: {error}"));
    store
        .persist(&state_with("b"))
        .unwrap_or_else(|error| panic!("replace: {error}"));
    assert_eq!(present(&store).registration.fcm_token, "b".repeat(64));
    let leftovers = std::fs::read_dir(directory.path())
        .unwrap_or_else(|error| panic!("read_dir: {error}"))
        .count();
    assert_eq!(leftovers, 1, "no temporary file may remain");
}

#[cfg(unix)]
#[test]
fn corrupt_content_is_moved_aside_but_unsafe_permissions_fail_closed() {
    use std::fs;
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
    let path = directory.path().join("push.json");
    let store = RingPushStore::new(path.clone());
    for content in [&b"{not json"[..], br#"{"schema_version":2}"#] {
        fs::write(&path, content).unwrap_or_else(|error| panic!("write: {error}"));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .unwrap_or_else(|error| panic!("chmod: {error}"));
        assert!(matches!(store.load(), Ok(StoredPush::Quarantined)));
        assert!(!path.exists());
        assert_eq!(
            fs::read(store.quarantine_path()).unwrap_or_else(|error| panic!("aside: {error}")),
            content
        );
        assert!(matches!(store.load(), Ok(StoredPush::Absent)));
    }
    fs::write(&path, b"{not json").unwrap_or_else(|error| panic!("write: {error}"));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644))
        .unwrap_or_else(|error| panic!("chmod: {error}"));
    assert!(store.load().is_err());
    assert!(path.exists(), "an unsafe file is reported, never moved");
}
