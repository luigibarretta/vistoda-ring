use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::get,
};
use serde::Serialize;
use tower_http::trace::TraceLayer;

use crate::{
    auth::require_bearer,
    config::BridgeConfig,
    error::BridgeError,
    model::{DeviceSummary, MediaCapabilities},
};

pub struct Runtime {
    pub config: BridgeConfig,
}

impl Runtime {
    #[must_use]
    pub const fn new(config: BridgeConfig) -> Self {
        Self { config }
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
        .route("/v1/devices/{device}/capabilities", get(capabilities))
        .layer(TraceLayer::new_for_http())
        .with_state(runtime)
}

async fn health() -> Json<Health<'static>> {
    Json(Health {
        status: "ok",
        phase: "protocol_research",
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
            capabilities: MediaCapabilities::research_only(),
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
    Ok(Json(MediaCapabilities::research_only()))
}
