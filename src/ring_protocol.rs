use serde::Serialize;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{error::BridgeError, ring_session::RingSession};

pub const OAUTH_ENDPOINT: &str = "https://oauth.ring.com/oauth/token";
pub const SESSION_ENDPOINT: &str = "https://api.ring.com/clients_api/session";
pub const DISCOVERY_ENDPOINT: &str = "https://api.ring.com/clients_api/ring_devices";
pub const CLIENT_API_ROOT: &str = "https://api.ring.com/clients_api";
pub const STREAM_TICKET_ENDPOINT: &str =
    "https://prd-api-us.prd.rings.solutions/api/v1/clap/ticket/request/signalsocket";
pub const SIGNALING_ORIGIN: &str = "wss://api.prod.signalling.ring.devices.a2z.com:443/ws";
pub const USER_AGENT: &str = "android:com.ringapp";
pub const CLIENT_ID: &str = "ring_official_android";
pub const API_VERSION: u8 = 11;

#[derive(Serialize)]
struct RefreshGrant<'a> {
    client_id: &'static str,
    scope: &'static str,
    grant_type: &'static str,
    refresh_token: &'a str,
}

#[derive(Serialize)]
struct SessionRequest<'a> {
    device: SessionDevice<'a>,
}

#[derive(Serialize)]
struct SessionDevice<'a> {
    hardware_id: Uuid,
    metadata: SessionMetadata<'a>,
    os: &'static str,
}

#[derive(Serialize)]
struct SessionMetadata<'a> {
    api_version: u8,
    device_model: &'a str,
}

pub struct ProtocolResearch<'a> {
    session: &'a RingSession,
}

impl<'a> ProtocolResearch<'a> {
    #[must_use]
    pub const fn new(session: &'a RingSession) -> Self {
        Self { session }
    }

    #[must_use]
    pub const fn oauth_endpoint(&self) -> &'static str {
        OAUTH_ENDPOINT
    }

    #[must_use]
    pub const fn hardware_id(&self) -> Uuid {
        self.session.hardware_id()
    }

    pub fn refresh_body(&self) -> Result<Zeroizing<Vec<u8>>, BridgeError> {
        let grant = RefreshGrant {
            client_id: CLIENT_ID,
            scope: "client",
            grant_type: "refresh_token",
            refresh_token: self.session.refresh_token(),
        };
        Ok(Zeroizing::new(serde_json::to_vec(&grant)?))
    }

    pub fn session_body(&self, display_name: &str) -> Result<Vec<u8>, BridgeError> {
        if display_name.is_empty()
            || display_name.len() > 64
            || !display_name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'-' | b'_'))
        {
            return Err(BridgeError::Configuration(
                "session display name contains unsupported characters".into(),
            ));
        }
        let request = SessionRequest {
            device: SessionDevice {
                hardware_id: self.session.hardware_id(),
                metadata: SessionMetadata {
                    api_version: API_VERSION,
                    device_model: display_name,
                },
                os: "android",
            },
        };
        Ok(serde_json::to_vec(&request)?)
    }
}
