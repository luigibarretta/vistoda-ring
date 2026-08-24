use std::{fs, path::Path, process::Command};

const ID: &str = "12345678-1234-1234-1234-123456789abc";

#[test]
fn verified_migration_moves_generated_archive_files() {
    let temporary = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let source = temporary.path().join("source");
    let target = temporary.path().join("target");
    fs::create_dir(&source).unwrap_or_else(|error| panic!("source failed: {error}"));
    fs::write(source.join(format!("{ID}.json")), b"manifest")
        .unwrap_or_else(|error| panic!("manifest failed: {error}"));
    fs::write(source.join(format!("{ID}.webm")), b"media")
        .unwrap_or_else(|error| panic!("media failed: {error}"));
    assert!(migration(&source, &target).success());
    assert!(
        source
            .read_dir()
            .is_ok_and(|mut entries| entries.next().is_none())
    );
    assert_eq!(
        fs::read(target.join(format!("{ID}.json"))).ok(),
        Some(b"manifest".to_vec())
    );
    assert_eq!(
        fs::read(target.join(format!("{ID}.webm"))).ok(),
        Some(b"media".to_vec())
    );
}

#[test]
fn conflicting_target_fails_without_removing_source() {
    let temporary = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let source = temporary.path().join("source");
    let target = temporary.path().join("target");
    fs::create_dir(&source).unwrap_or_else(|error| panic!("source failed: {error}"));
    fs::create_dir(&target).unwrap_or_else(|error| panic!("target failed: {error}"));
    let name = format!("{ID}.webm");
    fs::write(source.join(&name), b"source")
        .unwrap_or_else(|error| panic!("write failed: {error}"));
    fs::write(target.join(&name), b"conflict")
        .unwrap_or_else(|error| panic!("write failed: {error}"));
    assert!(!migration(&source, &target).success());
    assert_eq!(fs::read(source.join(name)).ok(), Some(b"source".to_vec()));
}

fn migration(source: &Path, target: &Path) -> std::process::ExitStatus {
    let helper =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("packaging/home-assistant/recording-storage.sh");
    Command::new("sh")
        .arg("-eu")
        .arg("-c")
        .arg(". \"$HELPER\"; migrate_recordings \"$SOURCE\" \"$TARGET\"")
        .env("HELPER", helper)
        .env("SOURCE", source)
        .env("TARGET", target)
        .status()
        .unwrap_or_else(|error| panic!("migration process failed: {error}"))
}
