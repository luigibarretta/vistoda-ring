use reqwest::StatusCode;
use serde::Deserialize;
use zeroize::Zeroizing;

use crate::error::BridgeError;

const BODY_LIMIT: usize = 64 * 1024;

#[derive(Deserialize)]
struct ChallengeResponse {
    tsv_state: String,
}

pub fn validate_email(value: &str) -> Result<(), BridgeError> {
    let mut parts = value.split('@');
    if value.len() > 254
        || !value.is_ascii()
        || !value.bytes().all(|byte| byte.is_ascii_graphic())
        || parts.next().is_none_or(str::is_empty)
        || parts.next().is_none_or(str::is_empty)
        || parts.next().is_some()
    {
        return Err(BridgeError::InvalidCredentials);
    }
    Ok(())
}

pub fn validate_password(value: &str) -> Result<(), BridgeError> {
    if value.is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
        return Err(BridgeError::InvalidCredentials);
    }
    Ok(())
}

pub fn validate_otp(value: &str) -> Result<(), BridgeError> {
    if value.len() != 6 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(BridgeError::InvalidOtp);
    }
    Ok(())
}

pub fn map_start_status(status: StatusCode) -> BridgeError {
    match status {
        StatusCode::BAD_REQUEST | StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            BridgeError::InvalidCredentials
        }
        StatusCode::TOO_MANY_REQUESTS => BridgeError::RateLimited,
        _ => BridgeError::UpstreamUnavailable,
    }
}

pub fn map_verify_status(status: StatusCode) -> BridgeError {
    match status {
        StatusCode::BAD_REQUEST
        | StatusCode::UNAUTHORIZED
        | StatusCode::FORBIDDEN
        | StatusCode::PRECONDITION_FAILED => BridgeError::InvalidOtp,
        StatusCode::TOO_MANY_REQUESTS => BridgeError::RateLimited,
        _ => BridgeError::UpstreamUnavailable,
    }
}

pub async fn success_body(response: reqwest::Response) -> Result<Zeroizing<Vec<u8>>, BridgeError> {
    read_bounded(response).await
}

pub async fn validate_challenge(response: reqwest::Response) -> Result<(), BridgeError> {
    let body = read_bounded(response).await?;
    let challenge: ChallengeResponse =
        serde_json::from_slice(&body).map_err(|_| BridgeError::UpstreamUnavailable)?;
    if challenge.tsv_state.is_empty()
        || challenge.tsv_state.len() > 64
        || !challenge
            .tsv_state
            .bytes()
            .all(|byte| byte.is_ascii_graphic())
    {
        return Err(BridgeError::UpstreamUnavailable);
    }
    Ok(())
}

async fn read_bounded(mut response: reqwest::Response) -> Result<Zeroizing<Vec<u8>>, BridgeError> {
    let mut body = Zeroizing::new(Vec::new());
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| BridgeError::UpstreamUnavailable)?
    {
        if body.len().saturating_add(chunk.len()) > BODY_LIMIT {
            return Err(BridgeError::UpstreamUnavailable);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}
