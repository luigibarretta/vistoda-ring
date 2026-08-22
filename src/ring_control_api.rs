use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, patch, post},
};

use crate::{
    api::Runtime,
    auth::require_bearer,
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
    headers: HeaderMap,
) -> Result<Json<RingDeviceStatus>, BridgeError> {
    authorize_device(&runtime, &headers, &device)?;
    Ok(Json(
        runtime.provider.client().await?.device_status().await?,
    ))
}

async fn unlock_door(
    State(runtime): State<Arc<Runtime>>,
    Path(device): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, BridgeError> {
    authorize_device(&runtime, &headers, &device)?;
    runtime.provider.client().await?.unlock().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn update_settings(
    State(runtime): State<Arc<Runtime>>,
    Path(device): Path<String>,
    headers: HeaderMap,
    Json(update): Json<VolumeUpdate>,
) -> Result<StatusCode, BridgeError> {
    authorize_device(&runtime, &headers, &device)?;
    runtime
        .provider
        .client()
        .await?
        .update_volume(&update)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn authorize_device(
    runtime: &Runtime,
    headers: &HeaderMap,
    device: &str,
) -> Result<(), BridgeError> {
    require_bearer(headers, &runtime.config.api_token)?;
    if !runtime.config.devices.contains_key(device) {
        return Err(BridgeError::DeviceNotFound);
    }
    Ok(())
}
