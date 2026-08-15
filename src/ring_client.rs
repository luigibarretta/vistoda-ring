pub use crate::ring_wire::RingIntercomIdentity;
use crate::{
    error::BridgeError,
    ring_http::checked_body,
    ring_protocol::{
        DISCOVERY_ENDPOINT, OAUTH_ENDPOINT, ProtocolResearch, SESSION_ENDPOINT,
        STREAM_TICKET_ENDPOINT, USER_AGENT,
    },
    ring_session::{RingSession, RingSessionStore},
    ring_wire::{OAuthResponse, parse_devices, parse_oauth},
};
use reqwest::{Client, StatusCode, redirect::Policy};
use serde_json::Value;
use std::{path::PathBuf, time::Duration};
use tokio::sync::Mutex;
use tokio::time::Instant;
use zeroize::Zeroizing;
pub struct AudioCallGrant {
    pub device_id: u64,
    pub ticket: Zeroizing<String>,
}
const AUTH_BODY_LIMIT: usize = 64 * 1024;
const SESSION_BODY_LIMIT: usize = 64 * 1024;
const DISCOVERY_BODY_LIMIT: usize = 2 * 1024 * 1024;
const EXPIRY_MARGIN: Duration = Duration::from_mins(1);
const SESSION_LIFETIME: Duration = Duration::from_hours(12);
struct Endpoints {
    oauth: String,
    session: String,
    discovery: String,
}
impl Endpoints {
    fn production() -> Self {
        Self {
            oauth: OAUTH_ENDPOINT.into(),
            session: SESSION_ENDPOINT.into(),
            discovery: DISCOVERY_ENDPOINT.into(),
        }
    }
}
struct AccessToken {
    value: Zeroizing<String>,
    valid_until: Instant,
}
struct ClientState {
    session: RingSession,
    access: Option<AccessToken>,
    registered_until: Option<Instant>,
    rotation_pending: bool,
}
pub struct RingReadOnlyClient {
    http: Client,
    endpoints: Endpoints,
    store: RingSessionStore,
    state: Mutex<ClientState>,
}
impl RingReadOnlyClient {
    pub fn new(session_path: PathBuf) -> Result<Self, BridgeError> {
        Self::build(session_path, Endpoints::production(), true)
    }

    fn build(
        session_path: PathBuf,
        endpoints: Endpoints,
        https_only: bool,
    ) -> Result<Self, BridgeError> {
        let store = RingSessionStore::new(session_path);
        let session = store.load()?;
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .redirect(Policy::none())
            .https_only(https_only)
            .build()
            .map_err(|error| BridgeError::Transport("client setup", error))?;
        Ok(Self {
            http,
            endpoints,
            store,
            state: Mutex::new(ClientState {
                session,
                access: None,
                registered_until: None,
                rotation_pending: false,
            }),
        })
    }

    pub async fn discover_intercoms(&self) -> Result<Vec<RingIntercomIdentity>, BridgeError> {
        let mut state = self.state.lock().await;
        for attempt in 0..=1 {
            self.ensure_authenticated(&mut state).await?;
            if let Err(error) = self.ensure_registered(&mut state).await {
                if attempt == 0 && is_unauthorized(&error) {
                    invalidate_auth(&mut state);
                    continue;
                }
                return Err(error);
            }
            let response = self.discovery_request(&state).await?;
            if response.status() == StatusCode::UNAUTHORIZED && attempt == 0 {
                invalidate_auth(&mut state);
                continue;
            }
            let body = checked_body(response, "device discovery", DISCOVERY_BODY_LIMIT).await?;
            drop(state);
            return parse_devices(&body);
        }
        Err(BridgeError::Protocol(
            "discovery retry was exhausted".into(),
        ))
    }

    pub async fn prepare_audio_call(&self) -> Result<AudioCallGrant, BridgeError> {
        let devices = self.discover_intercoms().await?;
        let device_id = match devices.as_slice() {
            [device] => device.id(),
            _ => return Err(BridgeError::Protocol("expected one Ring Intercom".into())),
        };
        let mut state = self.state.lock().await;
        self.ensure_authenticated(&mut state).await?;
        self.ensure_registered(&mut state).await?;
        let response = self
            .http
            .post(STREAM_TICKET_ENDPOINT)
            .bearer_auth(access_value(&state)?)
            .header("hardware_id", state.session.hardware_id().to_string())
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .send()
            .await
            .map_err(|error| BridgeError::Transport("stream ticket", error))?;
        drop(state);
        let body = checked_body(response, "stream ticket", AUTH_BODY_LIMIT).await?;
        Ok(AudioCallGrant {
            device_id,
            ticket: crate::ring_wire::parse_ticket(&body)?,
        })
    }
    async fn ensure_authenticated(&self, state: &mut ClientState) -> Result<(), BridgeError> {
        if state.rotation_pending {
            self.store.persist(&state.session)?;
            state.rotation_pending = false;
        }
        if state
            .access
            .as_ref()
            .is_some_and(|token| token.valid_until > Instant::now() + EXPIRY_MARGIN)
        {
            return Ok(());
        }
        self.store.persist(&state.session)?;
        let protocol = ProtocolResearch::new(&state.session);
        let response = self
            .http
            .post(&self.endpoints.oauth)
            .header("2fa-support", "true")
            .header("2fa-code", "")
            .header("hardware_id", protocol.hardware_id().to_string())
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(protocol.refresh_body()?.to_vec())
            .send()
            .await
            .map_err(|error| BridgeError::Transport("OAuth refresh", error))?;
        let body = checked_body(response, "OAuth refresh", AUTH_BODY_LIMIT).await?;
        self.accept_oauth(state, parse_oauth(&body)?)
    }

    fn accept_oauth(
        &self,
        state: &mut ClientState,
        response: OAuthResponse,
    ) -> Result<(), BridgeError> {
        let valid_until = Instant::now() + Duration::from_secs(response.expires_in);
        state.access = Some(AccessToken {
            value: response.access_token,
            valid_until,
        });
        state
            .session
            .replace_refresh_token(response.refresh_token)?;
        state.rotation_pending = true;
        self.store.persist(&state.session)?;
        state.rotation_pending = false;
        Ok(())
    }

    async fn ensure_registered(&self, state: &mut ClientState) -> Result<(), BridgeError> {
        if state
            .registered_until
            .is_some_and(|deadline| deadline > Instant::now())
        {
            return Ok(());
        }
        let access = access_value(state)?;
        let protocol = ProtocolResearch::new(&state.session);
        let response = self
            .http
            .post(&self.endpoints.session)
            .bearer_auth(access)
            .header("hardware_id", protocol.hardware_id().to_string())
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .json(&serde_json::from_slice::<Value>(
                &protocol.session_body("ring-intercom-bridge")?,
            )?)
            .send()
            .await
            .map_err(|error| BridgeError::Transport("session registration", error))?;
        checked_body(response, "session registration", SESSION_BODY_LIMIT).await?;
        state.registered_until = Some(Instant::now() + SESSION_LIFETIME);
        Ok(())
    }

    async fn discovery_request(
        &self,
        state: &ClientState,
    ) -> Result<reqwest::Response, BridgeError> {
        self.http
            .get(&self.endpoints.discovery)
            .bearer_auth(access_value(state)?)
            .header("hardware_id", state.session.hardware_id().to_string())
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|error| BridgeError::Transport("device discovery", error))
    }
}

fn access_value(state: &ClientState) -> Result<&str, BridgeError> {
    state
        .access
        .as_ref()
        .map(|token| token.value.as_str())
        .ok_or_else(|| BridgeError::Protocol("access token is unavailable".into()))
}

fn invalidate_auth(state: &mut ClientState) {
    state.access = None;
    state.registered_until = None;
}

const fn is_unauthorized(error: &BridgeError) -> bool {
    matches!(error, BridgeError::VendorRejected { status: 401, .. })
}

#[cfg(test)]
#[path = "ring_client_tests.rs"]
mod tests;
