use std::{collections::BTreeMap, env, net::SocketAddr, path::PathBuf};

use crate::{
    error::BridgeError,
    model::{DeviceConfig, DeviceKind},
};

#[derive(Clone)]
pub struct BridgeConfig {
    pub bind_host: String,
    pub bind_port: u16,
    pub api_token: Vec<u8>,
    pub devices: BTreeMap<String, DeviceConfig>,
    pub session_file: PathBuf,
    pub recording_dir: PathBuf,
}

impl BridgeConfig {
    pub async fn from_env() -> Result<Self, BridgeError> {
        let token_path = path("RING_INTERCOM_API_TOKEN_FILE", "/run/secrets/api_token");
        let devices_path = path("RING_INTERCOM_DEVICES_FILE", "/config/devices.json");
        let token = tokio::fs::read(token_path).await?;
        let devices = tokio::fs::read_to_string(devices_path).await?;
        Self::new(
            value("RING_INTERCOM_BIND_HOST", "0.0.0.0"),
            integer("RING_INTERCOM_BIND_PORT", 8775, 1024, 65_535)?,
            token,
            serde_json::from_str(&devices)?,
        )
        .map(|config| {
            config
                .with_session_file(path(
                    "RING_INTERCOM_SESSION_FILE",
                    "/data/ring-session.json",
                ))
                .with_recording_dir(path("RING_INTERCOM_RECORDING_DIR", "/data/recordings"))
        })
    }

    pub fn new(
        bind_host: String,
        bind_port: u16,
        api_token: Vec<u8>,
        devices: BTreeMap<String, DeviceConfig>,
    ) -> Result<Self, BridgeError> {
        let api_token = trim_ascii(api_token);
        if api_token.len() < 32 {
            return Err(BridgeError::Configuration(
                "API token must contain at least 32 bytes".into(),
            ));
        }
        validate_devices(&devices)?;
        Ok(Self {
            bind_host,
            bind_port,
            api_token,
            devices,
            session_file: PathBuf::from("/data/ring-session.json"),
            recording_dir: PathBuf::from("/data/recordings"),
        })
    }

    #[must_use]
    pub fn with_session_file(mut self, path: PathBuf) -> Self {
        self.session_file = path;
        self
    }

    #[must_use]
    pub fn with_recording_dir(mut self, path: PathBuf) -> Self {
        self.recording_dir = path;
        self
    }

    pub fn socket_address(&self) -> Result<SocketAddr, BridgeError> {
        format!("{}:{}", self.bind_host, self.bind_port)
            .parse()
            .map_err(|_| BridgeError::Configuration("bind address is invalid".into()))
    }
}

fn validate_devices(devices: &BTreeMap<String, DeviceConfig>) -> Result<(), BridgeError> {
    if devices.is_empty() {
        return Err(BridgeError::Configuration(
            "at least one intercom alias is required".into(),
        ));
    }
    for (alias, device) in devices {
        if alias.is_empty()
            || !alias
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(BridgeError::Configuration(
                "aliases must be alphanumeric with '-' or '_'".into(),
            ));
        }
        if device.kind != DeviceKind::RingIntercomAudio {
            return Err(BridgeError::Configuration(
                "only Ring Intercom Audio is admitted during protocol research".into(),
            ));
        }
    }
    Ok(())
}

fn trim_ascii(mut value: Vec<u8>) -> Vec<u8> {
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value.pop();
    }
    let start = value
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(value.len());
    value.drain(..start);
    value
}

fn value(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_owned())
}

fn path(name: &str, default: &str) -> PathBuf {
    PathBuf::from(value(name, default))
}

fn integer(name: &str, default: u64, minimum: u64, maximum: u64) -> Result<u16, BridgeError> {
    let raw = value(name, &default.to_string());
    let parsed = raw
        .parse::<u64>()
        .map_err(|_| BridgeError::Configuration(format!("{name} must be an integer")))?;
    if !(minimum..=maximum).contains(&parsed) {
        return Err(BridgeError::Configuration(format!(
            "{name} must be between {minimum} and {maximum}"
        )));
    }
    u16::try_from(parsed)
        .map_err(|_| BridgeError::Configuration(format!("{name} is outside the port range")))
}
