use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("configuration is invalid: {0}")]
    Configuration(String),
    #[error("request is unauthorized")]
    Unauthorized,
    #[error("device was not found")]
    DeviceNotFound,
    #[error("request is invalid: {0}")]
    InvalidRequest(String),
    #[error("an audio session is already active for this device")]
    SessionBusy,
    #[error("vendor credentials were rejected")]
    InvalidCredentials,
    #[error("verification code was rejected")]
    InvalidOtp,
    #[error("another enrollment is active")]
    EnrollmentBusy,
    #[error("enrollment expired or was already consumed")]
    EnrollmentExpired,
    #[error("request is rate limited")]
    RateLimited,
    #[error("vendor enrollment is temporarily unavailable")]
    UpstreamUnavailable,
    #[error("Ring call recording is not enabled or available")]
    RecordingUnavailable,
    #[error("research fixture was rejected: {0}")]
    UnsafeFixture(String),
    #[error("vendor response was rejected: {0}")]
    Protocol(String),
    #[error("Ring rejected {operation} with HTTP {status}")]
    VendorRejected {
        operation: &'static str,
        status: u16,
    },
    #[error("Ring transport failed during {0}")]
    Transport(&'static str, #[source] reqwest::Error),
    #[error("I/O failed")]
    Io(#[from] std::io::Error),
    #[error("JSON is invalid")]
    Json(#[from] serde_json::Error),
}

impl IntoResponse for BridgeError {
    fn into_response(self) -> Response {
        let (status, code) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::DeviceNotFound => (StatusCode::NOT_FOUND, "device_not_found"),
            Self::InvalidRequest(_) => (StatusCode::BAD_REQUEST, "invalid_request"),
            Self::SessionBusy => (StatusCode::CONFLICT, "session_busy"),
            Self::InvalidCredentials => (StatusCode::UNPROCESSABLE_ENTITY, "invalid_auth"),
            Self::InvalidOtp => (StatusCode::UNPROCESSABLE_ENTITY, "invalid_otp"),
            Self::EnrollmentBusy => (StatusCode::CONFLICT, "enrollment_busy"),
            Self::EnrollmentExpired => (StatusCode::GONE, "enrollment_expired"),
            Self::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
            Self::UpstreamUnavailable => (StatusCode::BAD_GATEWAY, "upstream_unavailable"),
            Self::RecordingUnavailable => {
                (StatusCode::UNPROCESSABLE_ENTITY, "recording_unavailable")
            }
            Self::Configuration(_)
            | Self::UnsafeFixture(_)
            | Self::Protocol(_)
            | Self::VendorRejected { .. }
            | Self::Transport(_, _)
            | Self::Io(_)
            | Self::Json(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        };
        (status, Json(json!({ "error": code }))).into_response()
    }
}
