use std::{sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::get,
};
use serde::Deserialize;

use crate::{
    api::Runtime, auth::require_bearer, error::BridgeError, ring_push_event::RingPushEventBatch,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventsQuery {
    after: Option<u64>,
    #[serde(default)]
    wait: u8,
}

pub fn routes() -> Router<Arc<Runtime>> {
    Router::new().route("/v1/devices/{device}/events", get(events))
}

async fn events(
    State(runtime): State<Arc<Runtime>>,
    Path(device): Path<String>,
    Query(query): Query<EventsQuery>,
    headers: HeaderMap,
) -> Result<Json<RingPushEventBatch>, BridgeError> {
    require_bearer(&headers, &runtime.config.api_token)?;
    if !runtime.config.devices.contains_key(&device) {
        return Err(BridgeError::DeviceNotFound);
    }
    if query.wait > 30 {
        return Err(BridgeError::InvalidRequest(
            "event wait must be at most 30 seconds".into(),
        ));
    }
    let queue = runtime.push.events();
    let events = if let Some(after) = query.after {
        queue
            .wait_after(after, Duration::from_secs(u64::from(query.wait)))
            .await
    } else {
        Vec::new()
    };
    let next_sequence = if let Some(after) = query.after {
        events
            .last()
            .map_or(after, |event| event.sequence.max(after))
    } else {
        queue.latest_sequence().await
    };
    Ok(Json(RingPushEventBatch {
        events,
        next_sequence,
        generation: queue.generation().to_owned(),
        connected: runtime.push.connected(),
    }))
}
