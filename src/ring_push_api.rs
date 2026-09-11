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
    expected_device_id: Option<String>,
    generation: Option<String>,
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
    let target = runtime.device(&device)?;
    let device_id = target.expected_id(query.expected_device_id.as_deref())?;
    drop(target);
    if query
        .generation
        .as_ref()
        .is_some_and(|value| uuid::Uuid::parse_str(value).is_err())
    {
        return Err(BridgeError::InvalidRequest(
            "invalid cursor generation".into(),
        ));
    }
    if query.wait > 30 {
        return Err(BridgeError::InvalidRequest(
            "event wait must be at most 30 seconds".into(),
        ));
    }
    let queue = runtime.push.events(Some(device_id))?;
    let latest = queue.latest_sequence().await;
    let cursor_reset = query
        .generation
        .as_deref()
        .is_some_and(|value| value != queue.generation())
        || query.after.is_some_and(|after| after > latest);
    let after = if cursor_reset { Some(0) } else { query.after };
    let events = if let Some(after) = after {
        queue
            .wait_after(after, Duration::from_secs(u64::from(query.wait)))
            .await
    } else {
        Vec::new()
    };
    let next_sequence = if let Some(after) = after {
        events
            .last()
            .map_or(after, |event| event.sequence.max(after))
    } else {
        queue.latest_sequence().await
    };
    Ok(Json(RingPushEventBatch {
        device_id: device_id.to_string(),
        cursor_reset,
        events,
        next_sequence,
        generation: queue.generation().to_owned(),
        connected: runtime.push.connected(),
    }))
}
