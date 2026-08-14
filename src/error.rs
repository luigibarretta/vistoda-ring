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
            Self::Configuration(_) | Self::Io(_) | Self::Json(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "internal")
            }
        };
        (status, Json(json!({ "error": code }))).into_response()
    }
}
