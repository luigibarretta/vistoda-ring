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
    expected_device_id: Option<String>,
    #[serde(default = "default_limit")]
    limit: u8,
    cursor: Option<String>,
}

pub fn routes() -> Router<Arc<Runtime>> {
    Router::new()
        .route("/v1/devices/{device}/history", get(history))
        .route("/v1/devices/{device}/activity-probe", get(activity_probe))
}

async fn history(
    State(runtime): State<Arc<Runtime>>,
    Path(device): Path<String>,
    Query(query): Query<HistoryQuery>,
    headers: HeaderMap,
) -> Result<Json<RingHistoryPage>, BridgeError> {
    require_bearer(&headers, &runtime.config.api_token)?;
    let target = runtime.device(&device)?;
    let expected = target
        .expected_id(query.expected_device_id.as_deref())?
        .to_string();
    let page = target
        .provider
        .client()
        .await?
        .history_expected(query.limit, query.cursor.as_deref(), Some(&expected))
        .await?;
    drop(target);
    Ok(Json(page))
}

/// Read-only, redacted diagnostics: which Ring feeds record app unlocks.
async fn activity_probe(
    State(runtime): State<Arc<Runtime>>,
    Path(device): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Vec<crate::ring_client::ProbeSummary>>, BridgeError> {
    require_bearer(&headers, &runtime.config.api_token)?;
    let target = runtime.device(&device)?;
    let summaries = target.provider.client().await?.activity_probe().await?;
    drop(target);
    Ok(Json(summaries))
}

const fn default_limit() -> u8 {
    20
}
