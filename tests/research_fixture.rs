use ring_intercom_bridge::research::{
    parse_synthetic_discovery_fixture, write_synthetic_discovery_fixture,
};
use tempfile::tempdir;

#[test]
fn synthetic_discovery_finds_only_ring_intercom_audio() {
    let fixture = include_bytes!("fixtures/ring_devices_intercom_audio.json");
    let devices = parse_synthetic_discovery_fixture(fixture)
        .unwrap_or_else(|error| panic!("synthetic fixture failed: {error}"));
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].description(), "Synthetic Entrance Intercom");
    assert!(devices[0].is_synthetic_id());
}

#[test]
fn credential_shaped_fields_are_rejected() {
    let fixture = br#"{
        "schema_version": 1,
        "synthetic": true,
        "response": {
            "access_token": "not-allowed",
            "other": []
        }
    }"#;
    assert!(parse_synthetic_discovery_fixture(fixture).is_err());
}

#[test]
fn real_looking_network_values_are_rejected() {
    let fixture = br#"{
        "schema_version": 1,
        "synthetic": true,
        "response": {
            "endpoint": "192.0.2.10",
            "other": []
        }
    }"#;
    assert!(parse_synthetic_discovery_fixture(fixture).is_err());
}

#[test]
fn sanitized_writer_never_reuses_vendor_identity() {
    let directory = tempdir().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let path = directory.path().join("discovery.json");
    write_synthetic_discovery_fixture(&path, 2)
        .unwrap_or_else(|error| panic!("fixture write failed: {error}"));
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("fixture read failed: {error}"));
    let devices = parse_synthetic_discovery_fixture(&bytes)
        .unwrap_or_else(|error| panic!("fixture validation failed: {error}"));
    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0].description(), "Synthetic Ring Intercom Audio 1");
    assert!(write_synthetic_discovery_fixture(&path, 1).is_err());
}
