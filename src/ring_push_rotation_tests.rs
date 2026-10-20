//! Make-before-break rotation through a fake registrar and a real store.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use async_trait::async_trait;
use fcm_push_listener::Registration;

use super::{
    PushError,
    rotation::{PushRegistrar, prepare},
};
use crate::{
    ring_push_metrics::RingPushMetrics,
    ring_push_store::{RingPushStore, StoredPush, tests::state_with},
};

/// Hands out registrations with tokens "b…", "c…", … and records hand-offs
/// together with the token stored on disk at that moment.
struct FakeRegistrar {
    store: Arc<RingPushStore>,
    registrations: AtomicUsize,
    fail_register: AtomicBool,
    fail_new_hand_off: AtomicBool,
    hand_offs: Mutex<Vec<(String, Option<String>)>>,
}

impl FakeRegistrar {
    fn new(store: &Arc<RingPushStore>) -> Self {
        Self {
            store: Arc::clone(store),
            registrations: AtomicUsize::new(0),
            fail_register: AtomicBool::new(false),
            fail_new_hand_off: AtomicBool::new(false),
            hand_offs: Mutex::new(Vec::new()),
        }
    }

    fn hand_offs(&self) -> Vec<(String, Option<String>)> {
        self.hand_offs
            .lock()
            .unwrap_or_else(|error| panic!("{error}"))
            .clone()
    }
}

#[async_trait]
impl PushRegistrar for FakeRegistrar {
    async fn register(&self) -> Result<Registration, PushError> {
        if self.fail_register.load(Ordering::SeqCst) {
            return Err(PushError::Fcm(
                "registration",
                "PHONE_REGISTRATION_ERROR".into(),
            ));
        }
        let index = self.registrations.fetch_add(1, Ordering::SeqCst);
        let letter = char::from(b'b' + u8::try_from(index).unwrap_or_default());
        Ok(state_with(&letter.to_string()).registration)
    }

    async fn hand_off(&self, token: &str) -> Result<Vec<String>, PushError> {
        let on_disk = match self.store.load() {
            Ok(StoredPush::Present(state)) => Some(state.registration.fcm_token),
            _ => None,
        };
        self.hand_offs
            .lock()
            .unwrap_or_else(|error| panic!("{error}"))
            .push((token.to_owned(), on_disk));
        if self.fail_new_hand_off.load(Ordering::SeqCst) && !token.starts_with('a') {
            return Err(PushError::Fcm("hand-off", "synthetic".into()));
        }
        Ok(vec!["42".into()])
    }
}

fn stored_token(store: &RingPushStore) -> Option<String> {
    match store.load() {
        Ok(StoredPush::Present(state)) => Some(state.registration.fcm_token),
        _ => None,
    }
}

fn seeded() -> (tempfile::TempDir, Arc<RingPushStore>) {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
    let store = Arc::new(RingPushStore::new(directory.path().join("push.json")));
    store
        .persist(&state_with("a"))
        .unwrap_or_else(|error| panic!("persist: {error}"));
    (directory, store)
}

async fn run(
    store: &Arc<RingPushStore>,
    registrar: &FakeRegistrar,
    metrics: &RingPushMetrics,
    rotate: &mut bool,
) -> String {
    prepare(store, registrar, metrics, rotate)
        .await
        .unwrap_or_else(|error| panic!("prepare: {error}"))
        .state
        .registration
        .fcm_token
}

#[tokio::test]
async fn reconnects_reuse_the_stored_registration() {
    let (_directory, store) = seeded();
    let registrar = FakeRegistrar::new(&store);
    let metrics = RingPushMetrics::default();
    let mut rotate = false;
    for _ in 0..3 {
        assert_eq!(
            run(&store, &registrar, &metrics, &mut rotate).await,
            "a".repeat(64)
        );
    }
    assert_eq!(registrar.registrations.load(Ordering::SeqCst), 0);
    assert_eq!(registrar.hand_offs().len(), 3);
}

#[tokio::test]
async fn rotation_hands_the_new_registration_over_before_replacing_the_file() {
    let (_directory, store) = seeded();
    let registrar = FakeRegistrar::new(&store);
    let metrics = RingPushMetrics::default();
    let mut rotate = true;
    let token = run(&store, &registrar, &metrics, &mut rotate).await;
    assert_eq!(token, "b".repeat(64));
    assert!(!rotate);
    // Ring received the new token while the old one was still on disk.
    assert_eq!(
        registrar.hand_offs(),
        [("b".repeat(64), Some("a".repeat(64)))]
    );
    assert_eq!(stored_token(&store), Some("b".repeat(64)));
    assert!(
        metrics
            .render()
            .contains("vistoda_ring_push_rotations_total 1\n")
    );
}

#[tokio::test]
async fn a_failed_rotation_keeps_and_uses_the_old_registration() {
    let (_directory, store) = seeded();
    let registrar = FakeRegistrar::new(&store);
    let metrics = RingPushMetrics::default();
    registrar.fail_register.store(true, Ordering::SeqCst);
    let mut rotate = true;
    assert_eq!(
        run(&store, &registrar, &metrics, &mut rotate).await,
        "a".repeat(64)
    );
    assert!(!rotate, "a failed rotation waits for the next signal");
    registrar.fail_register.store(false, Ordering::SeqCst);
    registrar.fail_new_hand_off.store(true, Ordering::SeqCst);
    rotate = true;
    assert_eq!(
        run(&store, &registrar, &metrics, &mut rotate).await,
        "a".repeat(64)
    );
    assert_eq!(stored_token(&store), Some("a".repeat(64)));
    let tokens: Vec<String> = registrar.hand_offs().into_iter().map(|(t, _)| t).collect();
    assert_eq!(tokens, ["a".repeat(64), "b".repeat(64), "a".repeat(64)]);
    let rendered = metrics.render();
    assert!(rendered.contains("vistoda_ring_push_errors_total{stage=\"rotation\"} 2\n"));
    assert!(rendered.contains("vistoda_ring_push_rotations_total 0\n"));
}

#[tokio::test]
async fn a_missing_or_corrupt_registration_is_replaced_and_reported() {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
    let path = directory.path().join("push.json");
    let store = Arc::new(RingPushStore::new(path.clone()));
    let registrar = FakeRegistrar::new(&store);
    let metrics = RingPushMetrics::default();
    let mut rotate = false;
    assert_eq!(
        run(&store, &registrar, &metrics, &mut rotate).await,
        "b".repeat(64)
    );
    assert!(!metrics.state_was_reset());
    std::fs::write(&path, b"{truncated").unwrap_or_else(|error| panic!("write: {error}"));
    assert_eq!(
        run(&store, &registrar, &metrics, &mut rotate).await,
        "c".repeat(64)
    );
    assert!(metrics.state_was_reset());
    assert!(store.quarantine_path().exists());
    assert_eq!(stored_token(&store), Some("c".repeat(64)));
}
