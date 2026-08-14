use std::{path::PathBuf, time::Duration};

use reqwest::{Client, redirect::Policy};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    error::BridgeError,
    ring_protocol::{CLIENT_ID, USER_AGENT},
    ring_session::{RingSession, RingSessionStore},
    ring_wire::parse_oauth,
};

#[derive(Serialize)]
struct PasswordGrant<'a> {
    client_id: &'static str,
    scope: &'static str,
    grant_type: &'static str,
    username: &'a str,
    password: &'a str,
}

pub struct RingEnrollmentProvider {
    http: Client,
    oauth_endpoint: String,
    store: RingSessionStore,
}

impl RingEnrollmentProvider {
    pub fn new(
        session_file: PathBuf,
        oauth_endpoint: String,
        https_only: bool,
    ) -> Result<Self, BridgeError> {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .redirect(Policy::none())
            .https_only(https_only)
            .build()
            .map_err(|_| BridgeError::UpstreamUnavailable)?;
        Ok(Self {
            http,
            oauth_endpoint,
            store: RingSessionStore::new(session_file),
        })
    }

    pub async fn password_grant(
        &self,
        email: &str,
        password: &str,
        hardware_id: Uuid,
        otp: &str,
    ) -> Result<reqwest::Response, BridgeError> {
        self.http
            .post(&self.oauth_endpoint)
            .header("2fa-support", "true")
            .header("2fa-code", otp)
            .header("hardware_id", hardware_id.to_string())
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .json(&PasswordGrant {
                client_id: CLIENT_ID,
                scope: "client",
                grant_type: "password",
                username: email,
                password,
            })
            .send()
            .await
            .map_err(|_| BridgeError::UpstreamUnavailable)
    }

    pub fn persist_session(&self, hardware_id: Uuid, body: &[u8]) -> Result<(), BridgeError> {
        let oauth = parse_oauth(body)?;
        let session = RingSession::enrolled(hardware_id, oauth.refresh_token)?;
        self.store.persist(&session)
    }
}
