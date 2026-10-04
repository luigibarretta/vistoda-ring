use std::sync::{Arc, atomic::Ordering};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, patch, post, put},
};
use serde_json::{Value, json};

use super::{MockState, valid_bearer};

pub fn routes() -> Router<Arc<MockState>> {
    Router::new()
        .route("/commands/v1/devices/{device}/device_rpc", put(unlock))
        .route("/doorbots/42", put(doorbell_volume))
        .route("/devices/v1/devices/42/settings", patch(volume_settings))
        .route(
            "/notification_settings/v1/doorbots/42/event_types",
            get(event_types),
        )
        .route(
            "/notification_settings/v1/doorbots/42/event_types/unlock",
            post(subscribe_unlock),
        )
}

async fn event_types(State(state): State<Arc<MockState>>, headers: HeaderMap) -> Response {
    if !valid_bearer(&headers) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if state.control_calls.load(Ordering::SeqCst) == 0 {
        Json(json!(["ding", "motion"])).into_response()
    } else {
        Json(json!(["ding", "motion", "unlock"])).into_response()
    }
}

async fn subscribe_unlock(State(state): State<Arc<MockState>>, headers: HeaderMap) -> Response {
    if !valid_bearer(&headers) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    state.control_calls.fetch_add(1, Ordering::SeqCst);
    StatusCode::NO_CONTENT.into_response()
}

pub async fn ticket(State(state): State<Arc<MockState>>, headers: HeaderMap) -> Response {
    if !valid_bearer(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    state.ticket_calls.fetch_add(1, Ordering::SeqCst);
    Json(json!({"ticket":"synthetic_stream_ticket_for_testing_0123456789"})).into_response()
}

async fn unlock(
    Path(device): Path<usize>,
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
    state.last_control_device.store(device, Ordering::SeqCst);
    Json(json!({"result": {"code": 0}})).into_response()
}

pub async fn discover(State(state): State<Arc<MockState>>, headers: HeaderMap) -> Response {
    let call = state.discovery_calls.fetch_add(1, Ordering::SeqCst);
    if state
        .unavailable_discoveries
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
            remaining.checked_sub(1)
        })
        .is_ok()
    {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    if state.first_discovery_unauthorized && call == 0 {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if state.rate_limit_discovery {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    if !valid_bearer(&headers) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    Json(json!({"doorbots": if state.include_cameras { vec![json!({"id": 51, "kind": "doorbell_v4", "description": "Front camera", "location_id": "loc-1"})] } else { vec![] },
    "stickup_cams": if state.include_cameras { vec![json!({"id": 52, "kind": "stickup_cam", "description": "Garden camera"})] } else { vec![] },
    "other": [
        {"id": 42, "kind": "intercom_handset_audio", "description": "Synthetic Entrance Intercom",
         "location_id": "loc-1", "battery_life": "73", "alerts": {"connection": "online"},
         "settings": {"doorbell_volume": 6, "mic_volume": 10, "voice_volume": 9}},
        {"id": 43, "kind": if state.additional_intercoms > 0 { "intercom_handset_audio" } else { "third_party_garage_door_opener" },
         "description": "Synthetic Other", "location_id": "loc-1", "alerts": {"connection": "online"}}
    ]})).into_response()
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
