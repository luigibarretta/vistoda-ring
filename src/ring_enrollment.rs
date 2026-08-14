use std::{path::PathBuf, time::Duration};

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::{sync::Mutex, time::Instant};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
    error::BridgeError,
    ring_enrollment_provider::RingEnrollmentProvider,
    ring_enrollment_support::{
        map_start_status, map_verify_status, success_body, validate_challenge, validate_email,
        validate_otp, validate_password,
    },
    ring_protocol::OAUTH_ENDPOINT,
};

const ENROLLMENT_TTL: Duration = Duration::from_secs(120);
const START_COOLDOWN: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentStart {
    email: String,
    password: Zeroizing<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyEnrollment {
    code: Zeroizing<String>,
}

#[derive(Serialize)]
pub struct EnrollmentStarted {
    enrollment_id: String,
    next_step: &'static str,
    expires_in: u64,
}

#[derive(Serialize)]
pub struct EnrollmentVerified {
    status: &'static str,
}

struct PendingEnrollment {
    id: Uuid,
    hardware_id: Uuid,
    email: String,
    password: Zeroizing<String>,
    expires_at: Instant,
}

enum StartOutcome {
    Otp(PendingEnrollment),
    Complete,
}

#[derive(Default)]
struct EnrollmentState {
    pending: Option<PendingEnrollment>,
    last_start: Option<Instant>,
    in_flight: bool,
}

pub struct RingEnrollmentManager {
    provider: RingEnrollmentProvider,
    state: Mutex<EnrollmentState>,
}

impl RingEnrollmentManager {
    pub fn production(session_file: PathBuf) -> Result<Self, BridgeError> {
        Self::build(session_file, OAUTH_ENDPOINT.into(), true)
    }

    fn build(
        session_file: PathBuf,
        oauth_endpoint: String,
        https_only: bool,
    ) -> Result<Self, BridgeError> {
        Ok(Self {
            provider: RingEnrollmentProvider::new(session_file, oauth_endpoint, https_only)?,
            state: Mutex::new(EnrollmentState::default()),
        })
    }

    pub async fn start(&self, input: EnrollmentStart) -> Result<EnrollmentStarted, BridgeError> {
        validate_email(&input.email)?;
        validate_password(&input.password)?;
        let mut state = self.state.lock().await;
        expire_pending(&mut state);
        if state.pending.is_some() || state.in_flight {
            return Err(BridgeError::EnrollmentBusy);
        }
        if state
            .last_start
            .is_some_and(|last| last + START_COOLDOWN > Instant::now())
        {
            return Err(BridgeError::RateLimited);
        }
        state.last_start = Some(Instant::now());
        state.in_flight = true;
        drop(state);
        let id = Uuid::new_v4();
        let hardware_id = Uuid::new_v4();
        let response = self
            .provider
            .password_grant(&input.email, &input.password, hardware_id, "")
            .await;
        let outcome = match response {
            Err(error) => Err(error),
            Ok(response) => match response.status() {
                StatusCode::PRECONDITION_FAILED => validate_challenge(response).await.map(|()| {
                    StartOutcome::Otp(PendingEnrollment {
                        id,
                        hardware_id,
                        email: input.email,
                        password: input.password,
                        expires_at: Instant::now() + ENROLLMENT_TTL,
                    })
                }),
                StatusCode::OK => success_body(response)
                    .await
                    .and_then(|body| self.provider.persist_session(hardware_id, &body))
                    .map(|()| StartOutcome::Complete),
                status => Err(map_start_status(status)),
            },
        };
        let mut state = self.state.lock().await;
        state.in_flight = false;
        let result = match outcome {
            Ok(StartOutcome::Otp(pending)) => {
                state.pending = Some(pending);
                Ok(started(id, "otp", ENROLLMENT_TTL.as_secs()))
            }
            Ok(StartOutcome::Complete) => Ok(started(id, "complete", 0)),
            Err(error) => Err(error),
        };
        drop(state);
        result
    }

    pub async fn verify(
        &self,
        enrollment_id: &str,
        input: VerifyEnrollment,
    ) -> Result<EnrollmentVerified, BridgeError> {
        validate_otp(&input.code)?;
        let id = Uuid::parse_str(enrollment_id).map_err(|_| BridgeError::EnrollmentExpired)?;
        let pending = {
            let mut state = self.state.lock().await;
            expire_pending(&mut state);
            let result = match state.pending.take() {
                Some(pending) if pending.id == id => Ok(pending),
                Some(pending) => {
                    state.pending = Some(pending);
                    Err(BridgeError::EnrollmentExpired)
                }
                None => Err(BridgeError::EnrollmentExpired),
            };
            drop(state);
            result?
        };
        let response = self
            .provider
            .password_grant(
                &pending.email,
                &pending.password,
                pending.hardware_id,
                &input.code,
            )
            .await?;
        if response.status() != StatusCode::OK {
            return Err(map_verify_status(response.status()));
        }
        let body = success_body(response).await?;
        self.provider.persist_session(pending.hardware_id, &body)?;
        Ok(EnrollmentVerified { status: "complete" })
    }

    pub async fn cancel(&self, enrollment_id: &str) {
        let Ok(id) = Uuid::parse_str(enrollment_id) else {
            return;
        };
        let mut state = self.state.lock().await;
        if state
            .pending
            .as_ref()
            .is_some_and(|pending| pending.id == id)
        {
            state.pending = None;
        }
    }
}

fn started(id: Uuid, next_step: &'static str, expires_in: u64) -> EnrollmentStarted {
    EnrollmentStarted {
        enrollment_id: id.to_string(),
        next_step,
        expires_in,
    }
}

fn expire_pending(state: &mut EnrollmentState) {
    if state
        .pending
        .as_ref()
        .is_some_and(|pending| pending.expires_at <= Instant::now())
    {
        state.pending = None;
    }
}

#[cfg(test)]
#[path = "ring_enrollment_tests.rs"]
mod tests;
