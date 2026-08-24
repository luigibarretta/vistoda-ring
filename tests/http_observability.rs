use std::{
    collections::BTreeMap,
    io::Write,
    sync::{Arc, Mutex},
};

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use ring_intercom_bridge::{
    BridgeConfig, Runtime,
    model::{DeviceConfig, DeviceKind},
    router,
};
use serde_json::Value;
use tower::ServiceExt;
use tracing_subscriber::fmt::MakeWriter;

const TOKEN: &str = "01234567890123456789012345678901";
const CLIENT_ID: &str = "never-trust-client-correlation";
const QUERY_SECRET: &str = "never-log-query-values";

#[derive(Clone, Default)]
struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

struct CapturedWriter(Arc<Mutex<Vec<u8>>>);

impl Write for CapturedWriter {
    fn write(&mut self, value: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend_from_slice(value);
        Ok(value.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'writer> MakeWriter<'writer> for CapturedLogs {
    type Writer = CapturedWriter;

    fn make_writer(&'writer self) -> Self::Writer {
        CapturedWriter(Arc::clone(&self.0))
    }
}

impl CapturedLogs {
    fn text(&self) -> String {
        let value = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        String::from_utf8(value).unwrap_or_else(|error| panic!("log was not UTF-8: {error}"))
    }
}

#[tokio::test]
async fn server_failures_are_correlated_without_sensitive_request_data() {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let recordings = directory.path().join("recordings");
    let application = app(recordings.clone());
    std::fs::write(&recordings, b"not a directory")
        .unwrap_or_else(|error| panic!("recording fixture failed: {error}"));

    let captured = CapturedLogs::default();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_ansi(false)
        .with_writer(captured.clone())
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);
    let response = application
        .oneshot(
            Request::get(format!(
                "/v1/devices/entrance/recordings?value={QUERY_SECRET}"
            ))
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header("x-request-id", CLIENT_ID)
            .body(Body::empty())
            .unwrap_or_else(|error| panic!("request failed: {error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("router failed: {error}"));
    drop(guard);

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let response_id = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_else(|| panic!("response request ID missing"));
    assert!(uuid::Uuid::parse_str(response_id).is_ok());
    assert_ne!(response_id, CLIENT_ID);

    let logs = captured.text();
    assert!(!logs.contains(TOKEN));
    assert!(!logs.contains(CLIENT_ID));
    assert!(!logs.contains(QUERY_SECRET));
    assert!(!logs.contains("entrance"));
    let event = logs
        .lines()
        .find(|line| line.contains("HTTP request failed"))
        .unwrap_or_else(|| panic!("structured failure event missing: {logs}"));
    let value: Value = serde_json::from_str(event)
        .unwrap_or_else(|error| panic!("failure event was not JSON: {error}"));
    let fields = &value["fields"];
    assert_eq!(fields["request_id"], response_id);
    assert_eq!(fields["method"], "GET");
    assert_eq!(fields["route"], "/v1/devices/{device}/recordings");
    assert_eq!(fields["status"], 500);
    assert_eq!(fields["error_code"], "internal");
    assert_eq!(fields["error_class"], "configuration");
}

#[tokio::test]
async fn unmatched_routes_receive_server_correlation_ids() {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
    let response = app(directory.path().join("recordings"))
        .oneshot(
            Request::get("/not-a-real-route?value=private")
                .header("x-request-id", CLIENT_ID)
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("request failed: {error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("router failed: {error}"));
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response_id = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_else(|| panic!("response request ID missing"));
    assert!(uuid::Uuid::parse_str(response_id).is_ok());
    assert_ne!(response_id, CLIENT_ID);
}

fn app(recording_dir: std::path::PathBuf) -> axum::Router {
    let devices = BTreeMap::from([(
        "entrance".into(),
        DeviceConfig {
            kind: DeviceKind::RingIntercomAudio,
        },
    )]);
    let config = BridgeConfig::new("127.0.0.1".into(), 8775, TOKEN.into(), devices)
        .unwrap_or_else(|error| panic!("test configuration failed: {error}"))
        .with_recording_dir(recording_dir);
    router(Arc::new(Runtime::new(config).unwrap_or_else(|error| {
        panic!("runtime setup failed: {error}")
    })))
}
