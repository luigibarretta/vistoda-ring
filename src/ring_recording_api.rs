use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};

use crate::{
    api::Runtime,
    auth::require_bearer,
    error::BridgeError,
    ring_recording::{RecordingImport, RecordingImportRequest, RecordingList},
};

pub fn routes() -> Router<Arc<Runtime>> {
    Router::new()
        .route("/v1/devices/{device}/recording-imports", post(start_import))
        .route(
            "/v1/devices/{device}/recording-imports/{import}",
            get(import_status),
        )
        .route("/v1/devices/{device}/recordings", get(recordings))
        .route(
            "/v1/devices/{device}/recordings/{recording}",
            get(media).delete(delete_recording),
        )
}

async fn start_import(
    State(runtime): State<Arc<Runtime>>,
    Path(device): Path<String>,
    headers: HeaderMap,
    Json(input): Json<RecordingImportRequest>,
) -> Result<(StatusCode, Json<RecordingImport>), BridgeError> {
    authorize_device(&runtime, &headers, &device)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(runtime.recordings.start(input).await?),
    ))
}

async fn import_status(
    State(runtime): State<Arc<Runtime>>,
    Path((device, import)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<RecordingImport>, BridgeError> {
    authorize_device(&runtime, &headers, &device)?;
    let id = uuid::Uuid::parse_str(&import).map_err(|_| BridgeError::RecordingNotFound)?;
    Ok(Json(runtime.recordings.status(id).await?))
}

async fn recordings(
    State(runtime): State<Arc<Runtime>>,
    Path(device): Path<String>,
    headers: HeaderMap,
) -> Result<Json<RecordingList>, BridgeError> {
    authorize_device(&runtime, &headers, &device)?;
    Ok(Json(RecordingList {
        recordings: runtime.recordings.list()?,
    }))
}

async fn media(
    State(runtime): State<Arc<Runtime>>,
    Path((device, recording)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, BridgeError> {
    authorize_device(&runtime, &headers, &device)?;
    let id = uuid::Uuid::parse_str(&recording).map_err(|_| BridgeError::RecordingNotFound)?;
    Ok((
        [("content-type", "audio/mp4")],
        runtime.recordings.media(id)?,
    ))
}

async fn delete_recording(
    State(runtime): State<Arc<Runtime>>,
    Path((device, recording)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<StatusCode, BridgeError> {
    authorize_device(&runtime, &headers, &device)?;
    if let Ok(id) = uuid::Uuid::parse_str(&recording) {
        runtime.recordings.delete(id)?;
    }
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
