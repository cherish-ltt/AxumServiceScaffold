use std::{
    collections::BTreeMap,
    future::Future,
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

tokio::task_local! {
    static CURRENT: Diagnostics;
}

#[derive(Clone, Default)]
struct Diagnostics(Arc<Mutex<Details>>);

#[derive(Default)]
struct Details {
    route: Option<String>,
    queue_ms: Option<u64>,
    stages: BTreeMap<&'static str, Stage>,
}

#[derive(Debug, Default)]
struct Stage {
    elapsed_ms: u64,
    calls: u64,
    interrupted: u64,
}

struct Timer {
    diagnostics: Diagnostics,
    name: &'static str,
    start: Instant,
    completed: bool,
}

impl Drop for Timer {
    fn drop(&mut self) {
        let mut details = self.diagnostics.0.lock().unwrap_or_else(|e| e.into_inner());
        let stage = details.stages.entry(self.name).or_default();
        stage.elapsed_ms += millis(self.start.elapsed());
        stage.calls += 1;
        stage.interrupted += u64::from(!self.completed);
    }
}

/// 测量阶段但不逐阶段打印日志；必须使用固定名称，不能带用户输入或 SQL。
pub(crate) async fn measure<F: Future>(name: &'static str, future: F) -> F::Output {
    let mut timer = CURRENT
        .try_with(|diagnostics| Timer {
            diagnostics: diagnostics.clone(),
            name,
            start: Instant::now(),
            completed: false,
        })
        .ok();
    let result = future.await;
    if let Some(timer) = timer.as_mut() {
        timer.completed = true;
    }
    result
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
        let mut response = match tokio::time::timeout(Duration::from_secs(config.request_timeout_secs), next.run(request)).await {
            Ok(response) => normalize(response),
            Err(_) => ApiResponse::<()>::error(StatusCode::REQUEST_TIMEOUT, "Request timed out").into_response(),
        };
        if is_head {
            *response.body_mut() = axum::body::Body::empty();
        }
        let elapsed_ms = millis(started.elapsed());
        let details = diagnostics.0.lock().unwrap_or_else(|e| e.into_inner());
        let status = response.status().as_u16();
        let route = details.route.as_deref().unwrap_or("unmatched");
        let queue_ms = details.queue_ms.unwrap_or(elapsed_ms);
        if elapsed_ms >= config.slow_request_ms || status == 408 || (status >= 500 && (status != 503 || details.route.is_some())) {
            tracing::warn!(status, elapsed_ms, route, queue_ms, stages = ?details.stages, "request diagnostic");
        } else {
            tracing::info!(status, elapsed_ms, route, "request completed");
        }
        response
    }.instrument(span).await
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
    CURRENT.scope(diagnostics, next.run(request)).await
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stages_aggregate_and_record_cancellation_without_extra_events() {
        let diagnostics = Diagnostics::default();
        CURRENT
            .scope(diagnostics.clone(), async {
                measure("db.query", async {}).await;
                measure("db.query", async {}).await;
                assert!(
                    tokio::time::timeout(
                        Duration::from_millis(5),
                        measure("db.wait", std::future::pending::<()>())
                    )
                    .await
                    .is_err()
                );
            })
            .await;
        let details = diagnostics.0.lock().unwrap();
        assert_eq!(details.stages["db.query"].calls, 2);
        assert_eq!(details.stages["db.query"].interrupted, 0);
        assert_eq!(details.stages["db.wait"].interrupted, 1);
        assert!(details.stages["db.wait"].elapsed_ms >= 5);
    }

    #[tokio::test]
    async fn measurements_work_without_request_context() {
        assert_eq!(measure("outside", async { 42 }).await, 42);
    }
}
