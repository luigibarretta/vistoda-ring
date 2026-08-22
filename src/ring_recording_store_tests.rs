use std::fs;

use tempfile::tempdir;

use super::RecordingStore;

fn mp4() -> Vec<u8> {
    let mut body = vec![0_u8; 2048];
    body[4..8].copy_from_slice(b"ftyp");
    body
}

fn webm() -> Vec<u8> {
    let mut body = vec![0_u8; 2048];
    body[..4].copy_from_slice(&[0x1a, 0x45, 0xdf, 0xa3]);
    body
}

#[test]
fn commit_list_read_and_idempotent_delete_are_consistent() {
    let directory = tempdir().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let store = RecordingStore::new(directory.path().join("recordings"))
        .unwrap_or_else(|error| panic!("store failed: {error}"));
    let item = store
        .commit(100, 110, "audio/mp4", &mp4())
        .unwrap_or_else(|error| panic!("commit failed: {error}"));
    assert_eq!(store.list().unwrap_or_default(), vec![item.clone()]);
    assert_eq!(
        store.read(item.recording_id).unwrap_or_default(),
        ("audio/mp4".into(), mp4())
    );
    store
        .delete(item.recording_id)
        .unwrap_or_else(|error| panic!("delete failed: {error}"));
    store
        .delete(item.recording_id)
        .unwrap_or_else(|error| panic!("second delete failed: {error}"));
    assert!(store.list().unwrap_or_default().is_empty());
}

#[test]
fn unsafe_media_and_symlinked_roots_are_rejected() {
    let directory = tempdir().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let store = RecordingStore::new(directory.path().join("recordings"))
        .unwrap_or_else(|error| panic!("store failed: {error}"));
    assert!(store.commit(100, 110, "audio/mp4", b"not media").is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(directory.path(), directory.path().join("link"))
            .unwrap_or_else(|error| panic!("symlink failed: {error}"));
        assert!(RecordingStore::new(directory.path().join("link")).is_err());
    }
    assert!(fs::read_dir(directory.path()).is_ok());
}

#[test]
fn webm_recording_round_trips_with_its_content_type() {
    let directory = tempdir().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let store = RecordingStore::new(directory.path().join("recordings"))
        .unwrap_or_else(|error| panic!("store failed: {error}"));
    let item = store
        .commit(200, 220, "audio/webm;codecs=opus", &webm())
        .unwrap_or_else(|error| panic!("commit failed: {error}"));
    assert_eq!(item.media_type, "audio/webm");
    assert_eq!(store.read(item.recording_id).unwrap_or_default().1, webm());
}
