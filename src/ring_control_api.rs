use std::sync::Arc;

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch, post},
};

use crate::{
    api::Runtime,
    error::BridgeError,
    ring_control::{RingDeviceStatus, VolumeUpdate},
};

pub fn routes() -> Router<Arc<Runtime>> {
    Router::new()
        .route("/v1/devices/{device}/status", get(device_status))
        .route("/v1/devices/{device}/unlock", post(unlock_door))
        .route("/v1/devices/{device}/settings", patch(update_settings))
}

async fn device_status(
    State(runtime): State<Arc<Runtime>>,
    Path(device): Path<String>,
) -> Result<Json<RingDeviceStatus>, BridgeError> {
    ensure_device(&runtime, &device)?;
    Ok(Json(
        runtime
            .device(&device)?
            .provider
            .client()
            .await?
            .device_status()
            .await?,
    ))
}

async fn unlock_door(
    State(runtime): State<Arc<Runtime>>,
    Path(device): Path<String>,
    body: Bytes,
) -> Result<StatusCode, BridgeError> {
    ensure_device(&runtime, &device)?;
    if body.len() > 256 {
        return Err(BridgeError::InvalidRequest("unlock body too large".into()));
    }
    let input: UnlockRequest = if body.is_empty() {
        UnlockRequest {
            expected_device_id: None,
        }
    } else {
        serde_json::from_slice(&body)
            .map_err(|_| BridgeError::InvalidRequest("invalid unlock body".into()))?
    };
    let target = runtime.device(&device)?;
    let expected = target
        .expected_id(input.expected_device_id.as_deref())?
        .to_string();
    target
        .provider
        .client()
        .await?
        .unlock_expected(Some(&expected))
        .await?;
    drop(target);
    Ok(StatusCode::NO_CONTENT)
}

async fn update_settings(
    State(runtime): State<Arc<Runtime>>,
    Path(device): Path<String>,
    Json(mut update): Json<VolumeUpdate>,
) -> Result<StatusCode, BridgeError> {
    ensure_device(&runtime, &device)?;
    let target = runtime.device(&device)?;
    update.expected_device_id = Some(
        target
            .expected_id(update.expected_device_id.as_deref())?
            .to_string(),
    );
    target
        .provider
        .client()
        .await?
        .update_volume(&update)
        .await?;
    drop(target);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct UnlockRequest {
    expected_device_id: Option<String>,
}

fn ensure_device(runtime: &Runtime, device: &str) -> Result<(), BridgeError> {
    runtime.device(device)?;
    Ok(())
}
