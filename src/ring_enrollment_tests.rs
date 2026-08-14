use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::{net::TcpListener, task::JoinHandle};

use super::{EnrollmentStart, RingEnrollmentManager, VerifyEnrollment};
use crate::error::BridgeError;

const PASSWORD: &str = "synthetic-password";
const REFRESH: &str = "synthetic_refresh_token_1234567890abcdef";

struct Harness {
    manager: RingEnrollmentManager,
    directory: TempDir,
    calls: Arc<AtomicUsize>,
    server: JoinHandle<()>,
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn harness(reject_otp: bool) -> Harness {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route("/oauth/token", post(oauth))
        .with_state((Arc::clone(&calls), reject_otp));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("mock bind failed: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("mock address failed: {error}"));
    let server = tokio::spawn(async move {
        let _ignored = axum::serve(listener, app).await;
    });
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("test directory failed: {error}"));
    let manager = RingEnrollmentManager::build(
        directory.path().join("session.json"),
        format!("http://{address}/oauth/token"),
        false,
    )
    .unwrap_or_else(|error| panic!("manager setup failed: {error}"));
    Harness {
        manager,
        directory,
        calls,
        server,
    }
}

async fn oauth(
    State((calls, reject_otp)): State<(Arc<AtomicUsize>, bool)>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    calls.fetch_add(1, Ordering::SeqCst);
    assert_eq!(body["grant_type"], "password");
    assert_eq!(body["username"], "owner@example.com");
    assert_eq!(body["password"], PASSWORD);
    assert!(headers.get("hardware_id").is_some());
    let otp = headers
        .get("2fa-code")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if otp.is_empty() {
        return (
            StatusCode::PRECONDITION_FAILED,
            Json(json!({"tsv_state":"totp"})),
        );
    }
    if reject_otp {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error":"rejected"})));
    }
    assert_eq!(otp, "123456");
    (
        StatusCode::OK,
        Json(json!({
            "access_token":"synthetic_access_token_1234567890abcdef",
            "expires_in":3600,
            "refresh_token":REFRESH,
            "scope":"client",
            "token_type":"Bearer"
        })),
    )
}

fn start() -> EnrollmentStart {
    serde_json::from_value(json!({
        "email":"owner@example.com",
        "password":PASSWORD
    }))
    .unwrap_or_else(|error| panic!("start input failed: {error}"))
}

fn otp() -> VerifyEnrollment {
    serde_json::from_value(json!({"code":"123456"}))
        .unwrap_or_else(|error| panic!("OTP input failed: {error}"))
}

#[tokio::test]
async fn two_step_enrollment_persists_only_the_rotating_session() {
    let harness = harness(false).await;
    let started = harness
        .manager
        .start(start())
        .await
        .unwrap_or_else(|error| panic!("start failed: {error}"));
    assert_eq!(started.next_step, "otp");
    let verified = harness
        .manager
        .verify(&started.enrollment_id, otp())
        .await
        .unwrap_or_else(|error| panic!("verify failed: {error}"));
    assert_eq!(verified.status, "complete");
    assert_eq!(harness.calls.load(Ordering::SeqCst), 2);
    let stored = std::fs::read_to_string(harness.directory.path().join("session.json"))
        .unwrap_or_else(|error| panic!("session read failed: {error}"));
    assert!(stored.contains(REFRESH));
    assert!(!stored.contains(PASSWORD));
    assert!(!stored.contains("owner@example.com"));
}

#[tokio::test]
async fn rejected_otp_consumes_the_challenge_without_retry() {
    let harness = harness(true).await;
    let started = harness
        .manager
        .start(start())
        .await
        .unwrap_or_else(|error| panic!("start failed: {error}"));
    assert!(matches!(
        harness.manager.verify(&started.enrollment_id, otp()).await,
        Err(BridgeError::InvalidOtp)
    ));
    assert!(matches!(
        harness.manager.verify(&started.enrollment_id, otp()).await,
        Err(BridgeError::EnrollmentExpired)
    ));
    assert_eq!(harness.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn cancellation_is_idempotent_and_consumes_no_vendor_attempt() {
    let harness = harness(false).await;
    let started = harness
        .manager
        .start(start())
        .await
        .unwrap_or_else(|error| panic!("start failed: {error}"));
    harness.manager.cancel(&started.enrollment_id).await;
    harness.manager.cancel(&started.enrollment_id).await;
    assert!(matches!(
        harness.manager.verify(&started.enrollment_id, otp()).await,
        Err(BridgeError::EnrollmentExpired)
    ));
    assert_eq!(harness.calls.load(Ordering::SeqCst), 1);
}
