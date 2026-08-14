use std::{
    fs::{File, OpenOptions},
    io::Write,
    net::IpAddr,
    path::Path,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use crate::error::BridgeError;

const MAX_FIXTURE_BYTES: usize = 256 * 1024;
const FORBIDDEN_KEYS: &[&str] = &[
    "authorization",
    "cookie",
    "password",
    "refresh_token",
    "secret",
    "session",
    "token",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    schema_version: u8,
    synthetic: bool,
    response: Value,
}

#[derive(Deserialize)]
struct DeviceEnvelope {
    #[serde(default)]
    other: Vec<RawDevice>,
}

#[derive(Deserialize)]
struct RawDevice {
    id: u64,
    kind: String,
    description: String,
}

pub struct DiscoveredIntercom {
    opaque_id: u64,
    description: String,
}

impl DiscoveredIntercom {
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    #[must_use]
    pub const fn is_synthetic_id(&self) -> bool {
        self.opaque_id <= 1_000
    }
}

pub fn parse_synthetic_discovery_fixture(
    input: &[u8],
) -> Result<Vec<DiscoveredIntercom>, BridgeError> {
    if input.len() > MAX_FIXTURE_BYTES {
        return Err(BridgeError::UnsafeFixture("fixture exceeds 256 KiB".into()));
    }
    let fixture: Fixture = serde_json::from_slice(input)?;
    if fixture.schema_version != 1 || !fixture.synthetic {
        return Err(BridgeError::UnsafeFixture(
            "fixture must declare synthetic schema version 1".into(),
        ));
    }
    validate_redaction(&fixture.response, "response")?;
    let devices: DeviceEnvelope = serde_json::from_value(fixture.response)?;
    devices
        .other
        .into_iter()
        .filter(|device| device.kind == "intercom_handset_audio")
        .map(validate_intercom)
        .collect()
}

pub fn write_synthetic_discovery_fixture(
    path: &Path,
    intercom_count: usize,
) -> Result<(), BridgeError> {
    if intercom_count == 0 || intercom_count > 32 {
        return Err(BridgeError::Protocol(
            "expected between 1 and 32 Ring Intercom devices".into(),
        ));
    }
    let devices = (1..=intercom_count)
        .map(|id| {
            json!({
                "id": id,
                "kind": "intercom_handset_audio",
                "description": format!("Synthetic Ring Intercom Audio {id}")
            })
        })
        .collect::<Vec<_>>();
    let fixture = SyntheticFixture {
        schema_version: 1,
        synthetic: true,
        response: json!({ "other": devices }),
    };
    let bytes = serde_json::to_vec_pretty(&fixture)?;
    let mut file = create_new_fixture(path)?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

#[derive(Serialize)]
struct SyntheticFixture {
    schema_version: u8,
    synthetic: bool,
    response: Value,
}

fn create_new_fixture(path: &Path) -> Result<File, BridgeError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o640).custom_flags(libc::O_CLOEXEC);
    options.open(path).map_err(BridgeError::Io)
}

fn validate_intercom(device: RawDevice) -> Result<DiscoveredIntercom, BridgeError> {
    if device.id == 0 || device.id > 1_000 {
        return Err(BridgeError::UnsafeFixture(
            "synthetic device IDs must be between 1 and 1000".into(),
        ));
    }
    if !device.description.starts_with("Synthetic ") {
        return Err(BridgeError::UnsafeFixture(
            "synthetic descriptions must start with 'Synthetic '".into(),
        ));
    }
    Ok(DiscoveredIntercom {
        opaque_id: device.id,
        description: device.description,
    })
}

fn validate_redaction(value: &Value, path: &str) -> Result<(), BridgeError> {
    match value {
        Value::Object(entries) => {
            for (key, child) in entries {
                let lower = key.to_ascii_lowercase();
                if FORBIDDEN_KEYS.iter().any(|needle| lower.contains(needle)) {
                    return Err(BridgeError::UnsafeFixture(format!(
                        "forbidden key at {path}.{key}"
                    )));
                }
                validate_redaction(child, &format!("{path}.{key}"))?;
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                validate_redaction(child, &format!("{path}[{index}]"))?;
            }
        }
        Value::String(text) => validate_string(text, path)?,
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    Ok(())
}

fn validate_string(text: &str, path: &str) -> Result<(), BridgeError> {
    let looks_like_jwt = text.split('.').count() == 3 && text.len() > 40;
    if text.contains("://") || text.parse::<IpAddr>().is_ok() || looks_like_jwt {
        return Err(BridgeError::UnsafeFixture(format!(
            "network or credential-shaped value at {path}"
        )));
    }
    Ok(())
}
