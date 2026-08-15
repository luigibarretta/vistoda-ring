use std::fs;

use tempfile::tempdir;

use super::RecordingStore;

fn media() -> Vec<u8> {
    let mut body = vec![0_u8; 2048];
    body[4..8].copy_from_slice(b"ftyp");
    body
}

#[test]
fn commit_list_read_and_idempotent_delete_are_consistent() {
    let directory = tempdir().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let store = RecordingStore::new(directory.path().join("recordings"))
        .unwrap_or_else(|error| panic!("store failed: {error}"));
    let item = store
        .commit(100, 110, &media())
        .unwrap_or_else(|error| panic!("commit failed: {error}"));
    assert_eq!(store.list().unwrap_or_default(), vec![item.clone()]);
    assert_eq!(store.read(item.recording_id).unwrap_or_default(), media());
    assert_eq!(
        store.find_trigger(104).unwrap_or_default(),
        Some(item.clone())
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
    assert!(store.commit(100, 110, b"not media").is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(directory.path(), directory.path().join("link"))
            .unwrap_or_else(|error| panic!("symlink failed: {error}"));
        assert!(RecordingStore::new(directory.path().join("link")).is_err());
    }
    assert!(fs::read_dir(directory.path()).is_ok());
}
