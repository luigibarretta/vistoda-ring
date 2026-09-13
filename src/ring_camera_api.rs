//! Exact camera-ID routes never materialize Intercom unlock/audio aliases.
use crate::{
    BridgeError, Runtime,
    auth::require_bearer,
    ring_audio::{AudioSessionCreated, AudioSessionRequest, SessionEndReason},
    ring_audio_manager::RingAudioSessions,
    ring_audio_worker::ProductionSessionRunner,
    ring_camera::{CameraInventory, MAX_CAMERAS},
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    routing::{delete, get, post},
};
use serde::Deserialize;
use std::sync::Arc;

pub fn routes() -> Router<Arc<Runtime>> {
    Router::new()
        .route("/v1/cameras", get(cameras))
        .route("/v1/cameras/{device}/video/sessions", post(start))
        .route(
            "/v1/cameras/{device}/video/sessions/{session}",
            delete(stop),
        )
}

async fn cameras(
    State(runtime): State<Arc<Runtime>>,
    headers: HeaderMap,
) -> Result<
    (
        [(header::HeaderName, &'static str); 1],
        Json<CameraInventory>,
    ),
    BridgeError,
> {
    require_bearer(&headers, &runtime.config.api_token)?;
    let client = runtime
        .provider
        .client()
        .await
        .map_err(|_| BridgeError::UpstreamUnavailable)?;
    let result = client
        .camera_inventory()
        .await
        .map_err(|_| BridgeError::UpstreamUnavailable)?;
    drop(client);
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(result)))
}

fn exact_id(path: &str, expected: Option<&str>) -> Result<u64, BridgeError> {
    let id = crate::ring_expected_device::parse_id(path)?;
    if expected != Some(path) {
        return Err(crate::ring_expected_device::mismatch());
    }
    Ok(id)
}

async fn start(
    State(runtime): State<Arc<Runtime>>,
    Path(device): Path<String>,
    headers: HeaderMap,
    Json(input): Json<AudioSessionRequest>,
) -> Result<
    (
        StatusCode,
        [(header::HeaderName, &'static str); 1],
        Json<AudioSessionCreated>,
    ),
    BridgeError,
> {
    require_bearer(&headers, &runtime.config.api_token)?;
    let id = exact_id(&device, input.expected_device_id.as_deref())?;
    crate::ring_video::validate_request(&input)?;
    let inventory = runtime.provider.client().await?.camera_inventory().await?;
    if !inventory
        .cameras
        .iter()
        .any(|camera| camera.device_id == device)
    {
        return Err(BridgeError::DeviceNotFound);
    }
    let mut targets = runtime.camera_sessions.lock().await;
    if !targets.contains_key(&id) && targets.len() >= MAX_CAMERAS {
        return Err(BridgeError::UpstreamUnavailable);
    }
    let target = targets
        .entry(id)
        .or_insert_with(|| {
            RingAudioSessions::build(
                Arc::new(ProductionSessionRunner::camera(Arc::new(
                    runtime.provider.scoped(Some(id)),
                ))),
                Arc::clone(&runtime.metrics),
            )
        })
        .clone();
    drop(targets);
    let result = target.start(device, input).await?;
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(result),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StopQuery {
    expected_device_id: String,
    reason: Option<SessionEndReason>,
}

async fn stop(
    State(runtime): State<Arc<Runtime>>,
    Path((device, session)): Path<(String, String)>,
    Query(query): Query<StopQuery>,
    headers: HeaderMap,
) -> Result<StatusCode, BridgeError> {
    require_bearer(&headers, &runtime.config.api_token)?;
    let id = exact_id(&device, Some(&query.expected_device_id))?;
    let reason = query.reason.unwrap_or(SessionEndReason::UserStop);
    if !reason.is_client() {
        return Err(BridgeError::InvalidRequest(
            "invalid client stop reason".into(),
        ));
    }
    let session = uuid::Uuid::parse_str(&session)
        .map_err(|_| BridgeError::InvalidRequest("invalid session ID".into()))?;
    let target = runtime.camera_sessions.lock().await.get(&id).cloned();
    if let Some(target) = target {
        target.delete(session, reason).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
#[path = "ring_camera_tests.rs"]
mod tests;
