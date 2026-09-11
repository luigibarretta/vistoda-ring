use std::{fs, path::Path, process::Command};

const ID: &str = "12345678-1234-1234-1234-123456789abc";

fn migrate(source: &Path, target: &Path) -> bool {
    Command::new("sh")
        .args([
            "-eu",
            "-c",
            ". \"$HELPER\"; migrate_recordings \"$SOURCE\" \"$TARGET\"",
        ])
        .env(
            "HELPER",
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("packaging/home-assistant/recording-storage.sh"),
        )
        .env("SOURCE", source)
        .env("TARGET", target)
        .status()
        .unwrap_or_else(|error| panic!("migration: {error}"))
        .success()
}

#[test]
fn physical_device_directories_and_legacy_files_migrate_without_reassignment() {
    let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
    let source = temp.path().join("source");
    let target = temp.path().join("target");
    for device in ["device-101", "device-202"] {
        fs::create_dir_all(source.join(device))
            .unwrap_or_else(|error| panic!("device directory: {error}"));
        fs::write(source.join(device).join(format!("{ID}.webm")), device)
            .unwrap_or_else(|error| panic!("recording: {error}"));
    }
    fs::write(source.join(format!("{ID}.json")), "legacy")
        .unwrap_or_else(|error| panic!("legacy manifest: {error}"));
    assert!(migrate(&source, &target));
    assert_eq!(
        source
            .read_dir()
            .unwrap_or_else(|error| panic!("source: {error}"))
            .count(),
        0
    );
    for device in ["device-101", "device-202"] {
        assert_eq!(
            fs::read_to_string(target.join(device).join(format!("{ID}.webm")))
                .unwrap_or_else(|error| panic!("copied: {error}")),
            device
        );
    }
    assert_eq!(
        fs::read_to_string(target.join(format!("{ID}.json")))
            .unwrap_or_else(|error| panic!("legacy: {error}")),
        "legacy"
    );
}

#[test]
fn later_conflict_preserves_every_source_directory() {
    let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
    let source = temp.path().join("source");
    let target = temp.path().join("target");
    for device in ["device-101", "device-202"] {
        fs::create_dir_all(source.join(device))
            .unwrap_or_else(|error| panic!("source directory: {error}"));
        fs::write(source.join(device).join(format!("{ID}.webm")), "source")
            .unwrap_or_else(|error| panic!("recording: {error}"));
    }
    fs::create_dir_all(target.join("device-202"))
        .unwrap_or_else(|error| panic!("target directory: {error}"));
    fs::write(
        target.join("device-202").join(format!("{ID}.webm")),
        "conflict",
    )
    .unwrap_or_else(|error| panic!("conflict: {error}"));
    assert!(!migrate(&source, &target));
    for device in ["device-101", "device-202"] {
        assert_eq!(
            fs::read_to_string(source.join(device).join(format!("{ID}.webm")))
                .unwrap_or_else(|error| panic!("source retained: {error}")),
            "source"
        );
    }
}

#[test]
fn symlinks_and_unrecognized_directories_fail_without_removing_source() {
    let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
    let source = temp.path().join("source");
    let target = temp.path().join("target");
    fs::create_dir(&source).unwrap_or_else(|error| panic!("source: {error}"));
    let recording = source.join(format!("{ID}.webm"));
    fs::write(&recording, "source").unwrap_or_else(|error| panic!("recording: {error}"));
    std::os::unix::fs::symlink(temp.path().join("missing"), source.join("device-101"))
        .unwrap_or_else(|error| panic!("symlink: {error}"));
    assert!(!migrate(&source, &target));
    assert!(recording.is_file());
    fs::remove_file(source.join("device-101"))
        .unwrap_or_else(|error| panic!("remove fixture symlink: {error}"));
    fs::create_dir(source.join("unknown")).unwrap_or_else(|error| panic!("unknown: {error}"));
    assert!(!migrate(&source, &target));
    assert!(recording.is_file());
}

#[test]
fn two_paths_to_the_same_archive_do_not_remove_files() {
    let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
    let source = temp.path().join("source");
    fs::create_dir(&source).unwrap_or_else(|error| panic!("source: {error}"));
    let recording = source.join(format!("{ID}.webm"));
    fs::write(&recording, "source").unwrap_or_else(|error| panic!("recording: {error}"));
    assert!(migrate(&source, &source.join(".")));
    assert_eq!(
        fs::read_to_string(recording).unwrap_or_else(|error| panic!("retained: {error}")),
        "source"
    );
}
