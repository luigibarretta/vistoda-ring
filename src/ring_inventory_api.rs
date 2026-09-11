//! Authenticated read-only discovery for choosing an enrolled physical intercom.
use crate::{BridgeError, api::Runtime, auth::require_bearer, ring_inventory::IntercomInventory};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, header},
    routing::get,
};
use std::sync::Arc;

pub fn routes() -> Router<Arc<Runtime>> {
    Router::new().route("/v1/intercoms", get(intercoms))
}

async fn intercoms(
    State(runtime): State<Arc<Runtime>>,
    headers: HeaderMap,
) -> Result<
    (
        [(header::HeaderName, &'static str); 1],
        Json<IntercomInventory>,
    ),
    BridgeError,
> {
    require_bearer(&headers, &runtime.config.api_token)?;
    let inventory = runtime
        .refresh_intercoms()
        .await
        .map_err(|_| BridgeError::UpstreamUnavailable)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(inventory)))
}

#[cfg(test)]
#[path = "ring_inventory_api_tests.rs"]
mod tests;
