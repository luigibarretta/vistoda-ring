use std::{fs, path::PathBuf};

#[test]
fn home_assistant_app_is_private_discovered_and_multiarch() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dockerfile = fs::read_to_string(root.join("packaging/home-assistant/Dockerfile"))
        .unwrap_or_else(|error| panic!("{error}"));
    let standalone =
        fs::read_to_string(root.join("Dockerfile")).unwrap_or_else(|error| panic!("{error}"));
    let runner = fs::read_to_string(root.join("packaging/home-assistant/run.sh"))
        .unwrap_or_else(|error| panic!("{error}"));
    let storage = fs::read_to_string(root.join("packaging/home-assistant/recording-storage.sh"))
        .unwrap_or_else(|error| panic!("{error}"));
    let workflow = fs::read_to_string(root.join(".github/workflows/publish-addon.yaml"))
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(dockerfile.contains("io.hass.type=\"app\""));
    assert!(dockerfile.contains(&format!("ARG BUILD_VERSION={}", env!("CARGO_PKG_VERSION"))));
    assert!(standalone.contains(&format!("ARG VERSION={}", env!("CARGO_PKG_VERSION"))));
    assert_eq!(
        standalone.matches("FROM ").count(),
        standalone.matches("@sha256:").count()
    );
    assert!(dockerfile.contains("HEALTHCHECK"));
    assert!(runner.contains("http://supervisor/discovery"));
    assert!(runner.contains("http://supervisor/addons/self/info"));
    assert!(runner.contains("--rawfile api_token"));
    assert!(runner.contains("managed_app: true"));
    assert!(runner.contains("RING_INTERCOM_RECORDING_DISPLAY_DIR"));
    assert!(runner.contains("RING_INTERCOM_RECORDING_STORAGE_KIND"));
    assert!(runner.contains("migrate_recordings"));
    assert!(storage.contains("cmp -s"));
    assert!(runner.contains("/addon_configs/${app_slug}/recordings"));
    assert!(runner.contains("chown bridge:bridge \"${data_dir}\""));
    assert!(runner.contains("chmod 0600 \"${data_dir}/ring-session.json\""));
    assert!(!runner.contains("8775:8775"));
    assert!(workflow.contains("[\"amd64\", \"aarch64\"]"));
    assert!(workflow.contains("home-assistant/builder/actions/build-image"));
    assert!(workflow.contains("publish-multi-arch-manifest"));
}
