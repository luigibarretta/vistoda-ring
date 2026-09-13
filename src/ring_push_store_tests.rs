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

#[test]
fn clear_removes_only_the_push_registration_and_is_idempotent() {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
    let path = directory.path().join("push.json");
    let store = RingPushStore::new(path.clone());
    store
        .persist(&state())
        .unwrap_or_else(|error| panic!("persist: {error}"));
    store
        .clear()
        .unwrap_or_else(|error| panic!("clear: {error}"));
    assert!(!path.exists());
    store
        .clear()
        .unwrap_or_else(|error| panic!("second clear: {error}"));
}
