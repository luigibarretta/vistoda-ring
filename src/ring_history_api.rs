use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::get,
};
use serde::Deserialize;

use crate::{
    api::Runtime, auth::require_bearer, error::BridgeError, ring_history::RingHistoryPage,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryQuery {
    #[serde(default = "default_limit")]
    limit: u8,
    cursor: Option<String>,
}

pub fn routes() -> Router<Arc<Runtime>> {
    Router::new().route("/v1/devices/{device}/history", get(history))
}

async fn history(
    State(runtime): State<Arc<Runtime>>,
    Path(device): Path<String>,
    Query(query): Query<HistoryQuery>,
    headers: HeaderMap,
) -> Result<Json<RingHistoryPage>, BridgeError> {
    require_bearer(&headers, &runtime.config.api_token)?;
    if !runtime.config.devices.contains_key(&device) {
        return Err(BridgeError::DeviceNotFound);
    }
    Ok(Json(
        runtime
            .provider
            .client()
            .await?
            .history(query.limit, query.cursor.as_deref())
            .await?,
    ))
}

const fn default_limit() -> u8 {
    20
}
