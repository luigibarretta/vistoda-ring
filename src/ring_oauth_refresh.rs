//! Access-token refresh with the stored, rotating refresh token. A refresh
//! token Ring revoked is reported as `ReauthRequired` and is not presented
//! again for a while, so pollers and API callers cannot hammer the endpoint.

use std::time::Duration;

use tokio::time::Instant;

use super::{AUTH_BODY_LIMIT, AccessToken, ClientState, RingClient};
use crate::{
    error::BridgeError,
    ring_http::checked_refresh_body,
    ring_protocol::{ProtocolResearch, USER_AGENT},
    ring_wire::{OAuthResponse, parse_oauth},
};

const EXPIRY_MARGIN: Duration = Duration::from_mins(1);
/// A revoked token never recovers on its own; re-enrollment replaces the
/// client state. The bounded retry only guards against a false rejection.
const REAUTH_RETRY: Duration = Duration::from_mins(15);

impl RingClient {
    pub(super) async fn ensure_authenticated(
        &self,
        state: &mut ClientState,
    ) -> Result<(), BridgeError> {
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
        if state
            .reauth_until
            .is_some_and(|until| until > Instant::now())
        {
            return Err(BridgeError::ReauthRequired);
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
        let body = match checked_refresh_body(response, "OAuth refresh", AUTH_BODY_LIMIT).await {
            Err(BridgeError::ReauthRequired) => {
                state.reauth_until = Some(Instant::now() + REAUTH_RETRY);
                return Err(BridgeError::ReauthRequired);
            }
            body => body?,
        };
        state.reauth_until = None;
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
}
