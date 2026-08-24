use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use thiserror::Error;

#[derive(Clone, Copy, Debug)]
pub(crate) struct HttpErrorContext {
    pub code: &'static str,
    pub class: &'static str,
}

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
    #[error("recording was not found")]
    RecordingNotFound,
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
        let (status, context) = match &self {
            Self::Unauthorized => response(StatusCode::UNAUTHORIZED, "unauthorized", "auth"),
            Self::DeviceNotFound => response(StatusCode::NOT_FOUND, "device_not_found", "routing"),
            Self::InvalidRequest(_) => {
                response(StatusCode::BAD_REQUEST, "invalid_request", "validation")
            }
            Self::SessionBusy => response(StatusCode::CONFLICT, "session_busy", "concurrency"),
            Self::InvalidCredentials => response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_auth",
                "provider_auth",
            ),
            Self::InvalidOtp => response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_otp",
                "provider_auth",
            ),
            Self::EnrollmentBusy => {
                response(StatusCode::CONFLICT, "enrollment_busy", "concurrency")
            }
            Self::EnrollmentExpired => {
                response(StatusCode::GONE, "enrollment_expired", "lifecycle")
            }
            Self::RateLimited => response(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "provider_limit",
            ),
            Self::UpstreamUnavailable => response(
                StatusCode::BAD_GATEWAY,
                "upstream_unavailable",
                "provider_availability",
            ),
            Self::RecordingNotFound => {
                response(StatusCode::NOT_FOUND, "recording_not_found", "storage")
            }
            Self::Configuration(_) => response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "configuration",
            ),
            Self::UnsafeFixture(_) => {
                response(StatusCode::INTERNAL_SERVER_ERROR, "internal", "fixture")
            }
            Self::Protocol(_) => {
                response(StatusCode::INTERNAL_SERVER_ERROR, "internal", "protocol")
            }
            Self::VendorRejected { .. } => response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "provider_response",
            ),
            Self::Transport(_, _) => {
                response(StatusCode::INTERNAL_SERVER_ERROR, "internal", "transport")
            }
            Self::Io(_) => response(StatusCode::INTERNAL_SERVER_ERROR, "internal", "io"),
            Self::Json(_) => response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "serialization",
            ),
        };
        let mut response = (status, Json(json!({ "error": context.code }))).into_response();
        response.extensions_mut().insert(context);
        response
    }
}

const fn response(
    status: StatusCode,
    code: &'static str,
    class: &'static str,
) -> (StatusCode, HttpErrorContext) {
    (status, HttpErrorContext { code, class })
}
