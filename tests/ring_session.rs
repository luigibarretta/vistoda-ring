use std::{fs, path::Path};

use ring_intercom_bridge::{
    ring_protocol::{
        API_VERSION, CLIENT_ID, DISCOVERY_ENDPOINT, OAUTH_ENDPOINT, ProtocolResearch,
        SESSION_ENDPOINT, USER_AGENT,
    },
    ring_session::RingSession,
};
use serde_json::Value;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use tempfile::tempdir;

const HARDWARE_ID: &str = "846f72e4-6b44-46a1-b3f5-5e8054486327";
const TOKEN: &str = "synthetic_refresh_token_1234567890abcdef";

#[test]
fn strict_session_parses_valid_document() {
    let session = RingSession::parse(document().as_bytes())
        .unwrap_or_else(|error| panic!("session parse failed: {error}"));
    assert_eq!(session.hardware_id().to_string(), HARDWARE_ID);
}

#[test]
fn password_and_unknown_fields_are_rejected() {
    let invalid = document().replace(
        "\"refresh_token\"",
        "\"password\":\"not-allowed\",\"refresh_token\"",
    );
    assert!(RingSession::parse(invalid.as_bytes()).is_err());
}

#[test]
fn protocol_shapes_match_the_researched_client_contract() {
    let session = RingSession::parse(document().as_bytes())
        .unwrap_or_else(|error| panic!("session parse failed: {error}"));
    let protocol = ProtocolResearch::new(&session);
    let refresh: Value = serde_json::from_slice(
        &protocol
            .refresh_body()
            .unwrap_or_else(|error| panic!("refresh body failed: {error}")),
    )
    .unwrap_or_else(|error| panic!("refresh JSON failed: {error}"));
    assert_eq!(protocol.oauth_endpoint(), OAUTH_ENDPOINT);
    assert_eq!(refresh["client_id"], CLIENT_ID);
    assert_eq!(refresh["scope"], "client");
    assert_eq!(refresh["grant_type"], "refresh_token");
    assert_eq!(refresh["refresh_token"], TOKEN);
    assert_eq!(SESSION_ENDPOINT, "https://api.ring.com/clients_api/session");
    assert_eq!(
        DISCOVERY_ENDPOINT,
        "https://api.ring.com/clients_api/ring_devices"
    );
    assert_eq!(USER_AGENT, "android:com.ringapp");
}

#[test]
fn session_registration_is_stable_and_bounded() {
    let session = RingSession::parse(document().as_bytes())
        .unwrap_or_else(|error| panic!("session parse failed: {error}"));
    let body = ProtocolResearch::new(&session)
        .session_body("ring-intercom-bridge")
        .unwrap_or_else(|error| panic!("session body failed: {error}"));
    let value: Value = serde_json::from_slice(&body)
        .unwrap_or_else(|error| panic!("session JSON failed: {error}"));
    assert_eq!(value["device"]["hardware_id"], HARDWARE_ID);
    assert_eq!(value["device"]["metadata"]["api_version"], API_VERSION);
    assert_eq!(
        value["device"]["metadata"]["device_model"],
        "ring-intercom-bridge"
    );
    assert_eq!(value["device"]["os"], "android");
}

#[cfg(unix)]
#[test]
fn session_file_requires_private_permissions() {
    let directory = tempdir().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let path = directory.path().join("ring-session.json");
    fs::write(&path, document()).unwrap_or_else(|error| panic!("write failed: {error}"));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644))
        .unwrap_or_else(|error| panic!("chmod failed: {error}"));
    assert!(RingSession::load(Path::new(&path)).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .unwrap_or_else(|error| panic!("chmod failed: {error}"));
    assert!(RingSession::load(Path::new(&path)).is_ok());
}

#[cfg(unix)]
#[test]
fn session_file_rejects_symlinks() {
    let directory = tempdir().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let target = directory.path().join("target.json");
    let link = directory.path().join("ring-session.json");
    fs::write(&target, document()).unwrap_or_else(|error| panic!("write failed: {error}"));
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600))
        .unwrap_or_else(|error| panic!("chmod failed: {error}"));
    symlink(&target, &link).unwrap_or_else(|error| panic!("symlink failed: {error}"));
    assert!(RingSession::load(Path::new(&link)).is_err());
}

fn document() -> String {
    format!(
        "{{\"schema_version\":1,\"hardware_id\":\"{HARDWARE_ID}\",\"refresh_token\":\"{TOKEN}\"}}"
    )
}
