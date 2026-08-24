use std::time::Instant;

use axum::{
    extract::{MatchedPath, Request},
    http::{HeaderName, HeaderValue, Method, StatusCode},
    middleware::Next,
    response::Response,
};
use uuid::Uuid;

use crate::error::HttpErrorContext;

const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

pub async fn observe_request(request: Request, next: Next) -> Response {
    let request_id = Uuid::new_v4();
    let method = request.method().clone();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or("unmatched", MatchedPath::as_str)
        .to_owned();
    let started = Instant::now();
    let mut response = next.run(request).await;
    let status = response.status();
    if status.is_client_error() || status.is_server_error() {
        log_failure(
            request_id,
            &method,
            &route,
            status,
            started.elapsed().as_secs_f64() * 1_000.0,
            response.extensions().get::<HttpErrorContext>().copied(),
        );
    }
    if let Ok(value) = HeaderValue::from_str(&request_id.to_string()) {
        response.headers_mut().insert(REQUEST_ID_HEADER, value);
    }
    response
}

fn log_failure(
    request_id: Uuid,
    method: &Method,
    route: &str,
    status: StatusCode,
    latency_ms: f64,
    context: Option<HttpErrorContext>,
) {
    let code = context.map_or("framework_response", |value| value.code);
    let class = context.map_or_else(
        || {
            if status.is_server_error() {
                "server_response"
            } else {
                "client_response"
            }
        },
        |value| value.class,
    );
    if status.is_server_error() {
        tracing::error!(
            %request_id,
            method = %method,
            route,
            status = status.as_u16(),
            latency_ms,
            error_code = code,
            error_class = class,
            "HTTP request failed"
        );
    } else {
        tracing::warn!(
            %request_id,
            method = %method,
            route,
            status = status.as_u16(),
            latency_ms,
            error_code = code,
            error_class = class,
            "HTTP request rejected"
        );
    }
}
