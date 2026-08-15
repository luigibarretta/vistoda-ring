use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{delete, get, post},
};
use serde::Serialize;
use tower_http::trace::TraceLayer;

use crate::{
    auth::require_bearer,
    config::BridgeConfig,
    error::BridgeError,
    model::{DeviceSummary, MediaCapabilities},
    ring_audio::{AudioSessionCreated, AudioSessionRequest},
    ring_audio_manager::RingAudioSessions,
    ring_enrollment::{
        EnrollmentStart, EnrollmentStarted, EnrollmentVerified, RingEnrollmentManager,
        VerifyEnrollment,
    },
    ring_recording_manager::RingRecordings,
};

pub struct Runtime {
    pub config: BridgeConfig,
    enrollment: RingEnrollmentManager,
    audio: RingAudioSessions,
    pub(crate) recordings: Arc<RingRecordings>,
}

impl Runtime {
    pub fn new(config: BridgeConfig) -> Result<Self, BridgeError> {
        let enrollment = RingEnrollmentManager::production(config.session_file.clone())?;
        let audio = RingAudioSessions::production(config.session_file.clone());
        let recordings =
            RingRecordings::production(config.session_file.clone(), config.recording_dir.clone())?;
        Ok(Self {
            config,
            enrollment,
            audio,
            recordings,
        })
    }
}

#[derive(Serialize)]
struct Health<'a> {
    status: &'a str,
    phase: &'a str,
    version: &'a str,
}

#[derive(Serialize)]
struct DeviceList {
    devices: Vec<DeviceSummary>,
}

pub fn router(runtime: Arc<Runtime>) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/v1/devices", get(devices))
        .route("/v1/enrollments", post(start_enrollment))
        .route(
            "/v1/enrollments/{enrollment}",
            post(verify_enrollment).delete(cancel_enrollment),
        )
        .route("/v1/devices/{device}/capabilities", get(capabilities))
        .route(
            "/v1/devices/{device}/audio/sessions",
            post(start_audio_session),
        )
        .route(
            "/v1/devices/{device}/audio/sessions/{session}",
            delete(delete_audio_session),
        )
        .merge(crate::ring_recording_api::routes())
        .layer(TraceLayer::new_for_http())
        .with_state(runtime)
}

async fn start_enrollment(
    State(runtime): State<Arc<Runtime>>,
    headers: HeaderMap,
    Json(input): Json<EnrollmentStart>,
) -> Result<Json<EnrollmentStarted>, BridgeError> {
    require_bearer(&headers, &runtime.config.api_token)?;
    Ok(Json(runtime.enrollment.start(input).await?))
}

async fn verify_enrollment(
    State(runtime): State<Arc<Runtime>>,
    Path(enrollment): Path<String>,
    headers: HeaderMap,
    Json(input): Json<VerifyEnrollment>,
) -> Result<Json<EnrollmentVerified>, BridgeError> {
    require_bearer(&headers, &runtime.config.api_token)?;
    Ok(Json(runtime.enrollment.verify(&enrollment, input).await?))
}

async fn cancel_enrollment(
    State(runtime): State<Arc<Runtime>>,
    Path(enrollment): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, BridgeError> {
    require_bearer(&headers, &runtime.config.api_token)?;
    runtime.enrollment.cancel(&enrollment).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn health() -> Json<Health<'static>> {
    Json(Health {
        status: "ok",
        phase: "verified",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn devices(
    State(runtime): State<Arc<Runtime>>,
    headers: HeaderMap,
) -> Result<Json<DeviceList>, BridgeError> {
    require_bearer(&headers, &runtime.config.api_token)?;
    let devices = runtime
        .config
        .devices
        .iter()
        .map(|(alias, device)| DeviceSummary {
            alias: alias.clone(),
            kind: device.kind,
            capabilities: MediaCapabilities::verified_audio_recordings(),
        })
        .collect();
    Ok(Json(DeviceList { devices }))
}

async fn capabilities(
    State(runtime): State<Arc<Runtime>>,
    Path(device): Path<String>,
    headers: HeaderMap,
) -> Result<Json<MediaCapabilities>, BridgeError> {
    require_bearer(&headers, &runtime.config.api_token)?;
    if !runtime.config.devices.contains_key(&device) {
        return Err(BridgeError::DeviceNotFound);
    }
    Ok(Json(MediaCapabilities::verified_audio_recordings()))
}

async fn start_audio_session(
    State(runtime): State<Arc<Runtime>>,
    Path(device): Path<String>,
    headers: HeaderMap,
    Json(input): Json<AudioSessionRequest>,
) -> Result<(StatusCode, Json<AudioSessionCreated>), BridgeError> {
    require_bearer(&headers, &runtime.config.api_token)?;
    if !runtime.config.devices.contains_key(&device) {
        return Err(BridgeError::DeviceNotFound);
    }
    let session = runtime.audio.start(device, input).await?;
    Ok((StatusCode::CREATED, Json(session)))
}

async fn delete_audio_session(
    State(runtime): State<Arc<Runtime>>,
    Path((device, session)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<StatusCode, BridgeError> {
    require_bearer(&headers, &runtime.config.api_token)?;
    if !runtime.config.devices.contains_key(&device) {
        return Err(BridgeError::DeviceNotFound);
    }
    if let Ok(id) = uuid::Uuid::parse_str(&session) {
        runtime.audio.delete(id).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}
