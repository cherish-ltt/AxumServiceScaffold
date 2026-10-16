use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    extract::{MatchedPath, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use tracing::Instrument;

use super::client_ip::{ClientIp, IdentityError};
use crate::{infrastructure::config::MiddlewareConfig, response::ApiResponse};

#[derive(Clone, Default)]
struct Diagnostics(Arc<Mutex<Details>>);

#[derive(Default)]
struct Details {
    route: Option<String>,
    queue_ms: Option<u64>,
}

#[derive(Clone)]
struct Arrival(Instant);

pub(crate) async fn observe(
    State(config): State<MiddlewareConfig>,
    mut request: Request,
    next: Next,
) -> Response {
    let started = Instant::now();
    let is_head = request.method() == axum::http::Method::HEAD;
    let diagnostics = Diagnostics::default();
    request.extensions_mut().insert(diagnostics.clone());
    request.extensions_mut().insert(Arrival(started));
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let client_ip = request
        .extensions()
        .get::<Result<ClientIp, IdentityError>>()
        .and_then(|value| value.as_ref().ok())
        .map(|ip| ip.0.to_string())
        .unwrap_or_default();
    let span = tracing::info_span!(target: "http_request", "http_request", method = %request.method(), path = %request.uri().path(), request_id = %request_id, client_ip = %client_ip);
    async move {
        let mut response = match tokio::time::timeout(
            Duration::from_secs(config.request_timeout_secs),
            next.run(request),
        )
        .await
        {
            Ok(response) => normalize(response),
            Err(_) => ApiResponse::<()>::error(StatusCode::REQUEST_TIMEOUT, "Request timed out")
                .into_response(),
        };
        if is_head {
            *response.body_mut() = axum::body::Body::empty();
        }
        let elapsed_ms = millis(started.elapsed());
        let details = diagnostics.0.lock().unwrap_or_else(|e| e.into_inner());
        let status = response.status().as_u16();
        let route = details.route.as_deref().unwrap_or("unmatched");
        let queue_ms = details.queue_ms.unwrap_or(elapsed_ms);
        if elapsed_ms >= config.slow_request_ms
            || status == 408
            || (status >= 500 && (status != 503 || details.route.is_some()))
        {
            tracing::warn!(status, elapsed_ms, route, queue_ms, "request diagnostic");
        } else {
            tracing::info!(status, elapsed_ms, route, "request completed");
        }
        response
    }
    .instrument(span)
    .await
}

pub(crate) async fn enter(mut request: Request, next: Next) -> Response {
    let diagnostics = request
        .extensions_mut()
        .get::<Diagnostics>()
        .cloned()
        .unwrap_or_default();
    if let Some(arrival) = request.extensions().get::<Arrival>() {
        diagnostics
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .queue_ms = Some(millis(arrival.0.elapsed()));
    }
    next.run(request).await
}

pub(crate) async fn route(request: Request, next: Next) -> Response {
    if let (Some(diagnostics), Some(path)) = (
        request.extensions().get::<Diagnostics>(),
        request.extensions().get::<MatchedPath>(),
    ) {
        diagnostics
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .route = Some(path.as_str().to_owned());
    }
    next.run(request).await
}

fn normalize(response: Response) -> Response {
    let message = match response.status() {
        StatusCode::BAD_REQUEST => "Invalid request",
        StatusCode::NOT_FOUND => "Not found",
        StatusCode::METHOD_NOT_ALLOWED => "Method not allowed",
        StatusCode::PAYLOAD_TOO_LARGE => "Request body too large",
        StatusCode::UNSUPPORTED_MEDIA_TYPE => "Expected application/json content type",
        StatusCode::UNPROCESSABLE_ENTITY => "Invalid request fields",
        _ => return response,
    };
    if response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"))
    {
        return response;
    }
    let mut normalized = ApiResponse::<()>::error(response.status(), message).into_response();
    for (name, value) in response.headers() {
        if name != axum::http::header::CONTENT_LENGTH
            && name != axum::http::header::CONTENT_TYPE
            && name != axum::http::header::CONTENT_ENCODING
            && name != axum::http::header::TRANSFER_ENCODING
        {
            normalized.headers_mut().insert(name.clone(), value.clone());
        }
    }
    normalized
}

fn millis(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}
