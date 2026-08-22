use std::sync::Arc;

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::get,
};

use crate::{
    api::Runtime,
    auth::require_bearer,
    error::BridgeError,
    ring_recording::{RecordingList, RecordingUploadQuery},
    ring_recording_media::MAX_MEDIA_BYTES,
};

pub fn routes() -> Router<Arc<Runtime>> {
    Router::new()
        .route(
            "/v1/devices/{device}/recordings",
            get(recordings)
                .post(upload_recording)
                .layer(DefaultBodyLimit::max(MAX_MEDIA_BYTES)),
        )
        .route(
            "/v1/devices/{device}/recordings/{recording}",
            get(media).delete(delete_recording),
        )
}

async fn upload_recording(
    State(runtime): State<Arc<Runtime>>,
    Path(device): Path<String>,
    Query(query): Query<RecordingUploadQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<crate::ring_recording::RecordingItem>), BridgeError> {
    authorize_device(&runtime, &headers, &device)?;
    query.validate()?;
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| BridgeError::InvalidRequest("recording content type is required".into()))?;
    Ok((
        StatusCode::CREATED,
        Json(
            runtime
                .recordings
                .commit(query.started_at, query.ended_at, content_type, &body)?,
        ),
    ))
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
    let (content_type, body) = runtime.recordings.media(id)?;
    Ok(([("content-type", content_type)], body))
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
