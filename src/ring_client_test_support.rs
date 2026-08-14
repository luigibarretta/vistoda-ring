use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

use super::super::{Endpoints, RingReadOnlyClient};

const HARDWARE_ID: &str = "846f72e4-6b44-46a1-b3f5-5e8054486327";
const REFRESH_A: &str = "synthetic_refresh_token_a_1234567890abcdef";
pub const REFRESH_B: &str = "synthetic_refresh_token_b_1234567890abcdef";
pub const REFRESH_C: &str = "synthetic_refresh_token_c_1234567890abcdef";
const ACCESS_A: &str = "synthetic_access_token_a_1234567890abcdef";
const ACCESS_B: &str = "synthetic_access_token_b_1234567890abcdef";

#[derive(Default)]
pub struct MockState {
    pub oauth_calls: AtomicUsize,
    pub session_calls: AtomicUsize,
    pub discovery_calls: AtomicUsize,
    pub first_discovery_unauthorized: bool,
    pub reject_oauth: bool,
    pub rate_limit_discovery: bool,
}

pub struct TestHarness {
    pub client: RingReadOnlyClient,
    pub session_path: PathBuf,
    task: tokio::task::JoinHandle<()>,
    _directory: TempDir,
}

impl Drop for TestHarness {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub async fn test_client(state: Arc<MockState>) -> TestHarness {
    let app = Router::new()
        .route("/oauth", post(oauth))
        .route("/session", post(register_session))
        .route("/devices", get(discover))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("mock bind failed: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("mock address failed: {error}"));
    let task = tokio::spawn(async move {
        let _ignored = axum::serve(listener, app).await;
    });
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let session_path = directory.path().join("ring-session.json");
    fs::write(&session_path, session_document(REFRESH_A))
        .unwrap_or_else(|error| panic!("session write failed: {error}"));
    #[cfg(unix)]
    fs::set_permissions(&session_path, fs::Permissions::from_mode(0o600))
        .unwrap_or_else(|error| panic!("session chmod failed: {error}"));
    let base = format!("http://{address}");
    let endpoints = Endpoints {
        oauth: format!("{base}/oauth"),
        session: format!("{base}/session"),
        discovery: format!("{base}/devices"),
    };
    let client = RingReadOnlyClient::build(session_path.clone(), endpoints, false)
        .unwrap_or_else(|error| panic!("client setup failed: {error}"));
    TestHarness {
        client,
        session_path,
        task,
        _directory: directory,
    }
}

async fn oauth(
    State(state): State<Arc<MockState>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let call = state.oauth_calls.fetch_add(1, Ordering::SeqCst);
    let expected_refresh = if call == 0 { REFRESH_A } else { REFRESH_B };
    if state.reject_oauth {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if invalid_oauth(&headers, &body, expected_refresh) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let (access, refresh) = if call == 0 {
        (ACCESS_A, REFRESH_B)
    } else {
        (ACCESS_B, REFRESH_C)
    };
    Json(json!({
        "access_token": access,
        "expires_in": 3600,
        "refresh_token": refresh,
        "scope": "client",
        "token_type": "Bearer"
    }))
    .into_response()
}

fn invalid_oauth(headers: &HeaderMap, body: &Value, expected_refresh: &str) -> bool {
    headers
        .get("hardware_id")
        .and_then(|value| value.to_str().ok())
        != Some(HARDWARE_ID)
        || headers
            .get("user-agent")
            .and_then(|value| value.to_str().ok())
            != Some("android:com.ringapp")
        || body["grant_type"] != "refresh_token"
        || body["refresh_token"] != expected_refresh
        || body["client_id"] != "ring_official_android"
}

async fn register_session(
    State(state): State<Arc<MockState>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    state.session_calls.fetch_add(1, Ordering::SeqCst);
    if !valid_bearer(&headers)
        || body["device"]["hardware_id"] != HARDWARE_ID
        || body["device"]["metadata"]["api_version"] != 11
        || body["device"]["os"] != "android"
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    Json(json!({ "profile": { "id": 1 } })).into_response()
}

async fn discover(State(state): State<Arc<MockState>>, headers: HeaderMap) -> Response {
    let call = state.discovery_calls.fetch_add(1, Ordering::SeqCst);
    if state.first_discovery_unauthorized && call == 0 {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if state.rate_limit_discovery {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    if !valid_bearer(&headers) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    Json(json!({
        "other": [
            {"id": 42, "kind": "intercom_handset_audio", "description": "Synthetic Entrance Intercom"},
            {"id": 43, "kind": "third_party_garage_door_opener", "description": "Synthetic Other"}
        ]
    }))
    .into_response()
}

fn valid_bearer(headers: &HeaderMap) -> bool {
    matches!(
        headers.get("authorization").and_then(|value| value.to_str().ok()),
        Some(value) if value == format!("Bearer {ACCESS_A}") || value == format!("Bearer {ACCESS_B}")
    )
}

pub fn assert_session_token(path: &std::path::Path, expected: &str) {
    let value: Value = serde_json::from_slice(
        &fs::read(path).unwrap_or_else(|error| panic!("session read failed: {error}")),
    )
    .unwrap_or_else(|error| panic!("session JSON failed: {error}"));
    assert_eq!(value["refresh_token"], expected);
    #[cfg(unix)]
    assert_eq!(
        fs::metadata(path)
            .unwrap_or_else(|error| panic!("session metadata failed: {error}"))
            .permissions()
            .mode()
            & 0o077,
        0
    );
}

fn session_document(token: &str) -> String {
    format!(
        "{{\"schema_version\":1,\"hardware_id\":\"{HARDWARE_ID}\",\"refresh_token\":\"{token}\"}}"
    )
}
