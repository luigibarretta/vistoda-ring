use std::sync::{Arc, atomic::Ordering};

use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{patch, put},
};
use serde_json::{Value, json};

use super::{MockState, valid_bearer};

pub fn routes() -> Router<Arc<MockState>> {
    Router::new()
        .route("/commands/v1/devices/42/device_rpc", put(unlock))
        .route("/doorbots/42", put(doorbell_volume))
        .route("/devices/v1/devices/42/settings", patch(volume_settings))
}

async fn unlock(
    State(state): State<Arc<MockState>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if !valid_bearer(&headers)
        || body["command_name"] != "device_rpc"
        || body["request"]["method"] != "unlock_door"
        || body["request"]["params"]["door_id"] != 0
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    state.control_calls.fetch_add(1, Ordering::SeqCst);
    Json(json!({"result": {"code": 0}})).into_response()
}

async fn doorbell_volume(
    State(state): State<Arc<MockState>>,
    headers: HeaderMap,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    if !valid_bearer(&headers)
        || query.get("doorbot[settings][doorbell_volume]") != Some(&"7".into())
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    state.control_calls.fetch_add(1, Ordering::SeqCst);
    Json(json!({})).into_response()
}

async fn volume_settings(
    State(state): State<Arc<MockState>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let volume = &body["volume_settings"];
    if !valid_bearer(&headers) || (volume["mic_volume"] != 8 && volume["voice_volume"] != 7) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    state.control_calls.fetch_add(1, Ordering::SeqCst);
    Json(json!({})).into_response()
}
