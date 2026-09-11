use std::sync::{Arc, RwLock};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    middleware,
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};

use crate::{
    auth::require_bearer,
    config::BridgeConfig,
    error::BridgeError,
    model::{DeviceSummary, MediaCapabilities},
    ring_audio::{AudioSessionCreated, AudioSessionRequest, SessionEndReason},
    ring_enrollment::RingEnrollmentManager,
    ring_enrollment_api::{cancel_enrollment, start_enrollment, verify_enrollment},
    ring_metrics::RingMetrics,
    ring_provider::RingProvider,
    ring_push_worker::RingPushService,
    ring_relay_metrics::RelayMetrics,
};

pub struct Runtime {
    pub config: BridgeConfig,
    pub(crate) enrollment: RingEnrollmentManager,
    pub(crate) device_runtimes: RwLock<
        std::collections::BTreeMap<String, Arc<crate::ring_device_runtime::RingDeviceRuntime>>,
    >,
    pub(crate) metrics: Arc<RingMetrics>,
    pub(crate) relay_metrics: Arc<RelayMetrics>,
    pub(crate) provider: Arc<RingProvider>,
    pub(crate) push: Arc<RingPushService>,
    pub(crate) discovery_started: std::sync::atomic::AtomicBool,
    pub(crate) discovery_wakeup: tokio::sync::Notify,
}

impl Runtime {
    pub fn new(config: BridgeConfig) -> Result<Self, BridgeError> {
        let enrollment = RingEnrollmentManager::production(config.session_file.clone())?;
        let provider = Arc::new(RingProvider::new(config.session_file.clone()));
        let metrics = Arc::new(RingMetrics::default());
        let relay_metrics = Arc::new(RelayMetrics::default());
        let device_runtimes =
            crate::ring_device_runtime::build_devices(&config, &provider, &metrics)?;
        let push = Arc::new(RingPushService::new(
            config.push_file.clone(),
            config.devices.values().map(|device| device.device_id),
        ));
        Ok(Self {
            config,
            enrollment,
            device_runtimes: RwLock::new(device_runtimes),
            metrics,
            relay_metrics,
            provider,
            push,
            discovery_started: std::sync::atomic::AtomicBool::new(false),
            discovery_wakeup: tokio::sync::Notify::new(),
        })
    }

    pub fn start_background(self: &Arc<Self>) {
        self.push.start(Arc::clone(&self.provider));
        self.start_discovery();
    }

    pub(crate) fn device(
        &self,
        alias: &str,
    ) -> Result<Arc<crate::ring_device_runtime::RingDeviceRuntime>, BridgeError> {
        self.device_runtimes
            .read()
            .map_err(|_| BridgeError::UpstreamUnavailable)?
            .get(alias)
            .cloned()
            .ok_or(BridgeError::DeviceNotFound)
    }
}

#[derive(Serialize)]
struct Health<'a> {
    status: &'a str,
    phase: &'a str,
    version: &'a str,
    push_connected: bool,
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
        .merge(crate::ring_history_api::routes())
        .merge(crate::ring_inventory_api::routes())
        .merge(crate::ring_push_api::routes())
        .merge(crate::ring_relay_api::routes())
        .merge(crate::ring_recording_api::routes())
        .fallback(|| async { StatusCode::NOT_FOUND })
        .layer(middleware::from_fn(
            crate::http_observability::observe_request,
        ))
        .with_state(runtime)
}

async fn health(State(runtime): State<Arc<Runtime>>) -> Json<Health<'static>> {
    Json(Health {
        status: "ok",
        phase: "verified",
        version: env!("CARGO_PKG_VERSION"),
        push_connected: runtime.push.connected(),
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
        format!(
            "{}{}{}",
            runtime.metrics.render(),
            runtime.relay_metrics.render(),
            runtime.push.metrics()
        ),
    )
}

async fn devices(
    State(runtime): State<Arc<Runtime>>,
    headers: HeaderMap,
) -> Result<Json<DeviceList>, BridgeError> {
    require_bearer(&headers, &runtime.config.api_token)?;
    let devices = runtime.device_summaries()?;
    Ok(Json(DeviceList { devices }))
}

async fn capabilities(
    State(runtime): State<Arc<Runtime>>,
    Path(device): Path<String>,
    headers: HeaderMap,
) -> Result<Json<MediaCapabilities>, BridgeError> {
    require_bearer(&headers, &runtime.config.api_token)?;
    runtime.device(&device)?;
    Ok(Json(MediaCapabilities::verified_audio_recordings()))
}

async fn start_audio_session(
    State(runtime): State<Arc<Runtime>>,
    Path(device): Path<String>,
    headers: HeaderMap,
    Json(mut input): Json<AudioSessionRequest>,
) -> Result<(StatusCode, Json<AudioSessionCreated>), BridgeError> {
    require_bearer(&headers, &runtime.config.api_token)?;
    let (target, physical) = runtime.audio_target(&device, input.expected_device_id.as_deref())?;
    input.expected_device_id = Some(physical.to_string());
    let session = target.audio.start(physical.to_string(), input).await?;
    drop(target);
    Ok((StatusCode::CREATED, Json(session)))
}

async fn delete_audio_session(
    State(runtime): State<Arc<Runtime>>,
    Path((device, session)): Path<(String, String)>,
    Query(query): Query<DeleteAudioQuery>,
    headers: HeaderMap,
) -> Result<StatusCode, BridgeError> {
    require_bearer(&headers, &runtime.config.api_token)?;
    let (target, _) = runtime.audio_target(&device, query.expected_device_id.as_deref())?;
    let reason = query.reason.unwrap_or(SessionEndReason::UserStop);
    if !reason.is_client() {
        return Err(BridgeError::InvalidRequest(
            "invalid client stop reason".into(),
        ));
    }
    if let Ok(id) = uuid::Uuid::parse_str(&session) {
        target.audio.delete(id, reason).await?;
    }
    drop(target);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteAudioQuery {
    expected_device_id: Option<String>,
    reason: Option<SessionEndReason>,
}
