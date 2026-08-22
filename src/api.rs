use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use tower_http::trace::TraceLayer;

use crate::{
    auth::require_bearer,
    config::BridgeConfig,
    error::BridgeError,
    model::{DeviceSummary, MediaCapabilities},
    ring_audio::{AudioSessionCreated, AudioSessionRequest, SessionEndReason},
    ring_audio_manager::RingAudioSessions,
    ring_enrollment::{
        EnrollmentStart, EnrollmentStarted, EnrollmentVerified, RingEnrollmentManager,
        VerifyEnrollment,
    },
    ring_metrics::RingMetrics,
    ring_provider::RingProvider,
    ring_recording_manager::RingRecordings,
};

pub struct Runtime {
    pub config: BridgeConfig,
    enrollment: RingEnrollmentManager,
    audio: RingAudioSessions,
    pub(crate) recordings: Arc<RingRecordings>,
    metrics: Arc<RingMetrics>,
    pub(crate) provider: Arc<RingProvider>,
}

impl Runtime {
    pub fn new(config: BridgeConfig) -> Result<Self, BridgeError> {
        let enrollment = RingEnrollmentManager::production(config.session_file.clone())?;
        let provider = Arc::new(RingProvider::new(config.session_file.clone()));
        let metrics = Arc::new(RingMetrics::default());
        let audio = RingAudioSessions::production(Arc::clone(&provider), Arc::clone(&metrics));
        let recordings =
            RingRecordings::production(Arc::clone(&provider), config.recording_dir.clone())?;
        Ok(Self {
            config,
            enrollment,
            audio,
            recordings,
            metrics,
            provider,
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
        .route("/metrics", get(prometheus_metrics))
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
        .merge(crate::ring_control_api::routes())
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

async fn prometheus_metrics(
    State(runtime): State<Arc<Runtime>>,
) -> impl axum::response::IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        runtime.metrics.render(),
    )
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
    Query(query): Query<DeleteAudioQuery>,
    headers: HeaderMap,
) -> Result<StatusCode, BridgeError> {
    require_bearer(&headers, &runtime.config.api_token)?;
    if !runtime.config.devices.contains_key(&device) {
        return Err(BridgeError::DeviceNotFound);
    }
    let reason = query.reason.unwrap_or(SessionEndReason::UserStop);
    if !reason.is_client() {
        return Err(BridgeError::InvalidRequest(
            "invalid client stop reason".into(),
        ));
    }
    if let Ok(id) = uuid::Uuid::parse_str(&session) {
        runtime.audio.delete(id, reason).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteAudioQuery {
    reason: Option<SessionEndReason>,
}
