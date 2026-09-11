//! Enrollment refreshes every scoped provider client and its push subscription.
use crate::{
    BridgeError,
    api::Runtime,
    auth::require_bearer,
    ring_enrollment::{EnrollmentStart, EnrollmentStarted, EnrollmentVerified, VerifyEnrollment},
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use std::sync::Arc;

pub async fn start_enrollment(
    State(runtime): State<Arc<Runtime>>,
    headers: HeaderMap,
    Json(input): Json<EnrollmentStart>,
) -> Result<Json<EnrollmentStarted>, BridgeError> {
    require_bearer(&headers, &runtime.config.api_token)?;
    let guard = runtime.provider.enrollment_guard().await;
    let result = runtime.enrollment.start(input).await?;
    if result.next_step == "complete" {
        runtime.provider.reset().await;
        runtime.push.reload();
    }
    drop(guard);
    if result.next_step == "complete" && runtime.refresh_intercoms().await.is_err() {
        tracing::warn!("Ring enrolled; intercom discovery pending inventory retry");
        runtime.start_discovery();
    }
    Ok(Json(result))
}

pub async fn verify_enrollment(
    State(runtime): State<Arc<Runtime>>,
    Path(enrollment): Path<String>,
    headers: HeaderMap,
    Json(input): Json<VerifyEnrollment>,
) -> Result<Json<EnrollmentVerified>, BridgeError> {
    require_bearer(&headers, &runtime.config.api_token)?;
    let guard = runtime.provider.enrollment_guard().await;
    let result = runtime.enrollment.verify(&enrollment, input).await?;
    runtime.provider.reset().await;
    runtime.push.reload();
    drop(guard);
    if runtime.refresh_intercoms().await.is_err() {
        tracing::warn!("Ring enrolled; intercom discovery pending inventory retry");
        runtime.start_discovery();
    }
    Ok(Json(result))
}

pub async fn cancel_enrollment(
    State(runtime): State<Arc<Runtime>>,
    Path(enrollment): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, BridgeError> {
    require_bearer(&headers, &runtime.config.api_token)?;
    runtime.enrollment.cancel(&enrollment).await;
    Ok(StatusCode::NO_CONTENT)
}
