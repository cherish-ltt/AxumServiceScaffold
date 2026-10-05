use std::time::Duration;

use axum::{
    Router,
    body::Body,
    error_handling::HandleErrorLayer,
    http::{HeaderName, HeaderValue, Request, StatusCode, header},
    response::Response,
};
use tower::{
    ServiceBuilder,
    buffer::BufferLayer,
    limit::{ConcurrencyLimitLayer, RateLimitLayer},
    load_shed::LoadShedLayer,
};
use tower_http::{
    LatencyUnit,
    compression::CompressionLayer,
    limit::RequestBodyLimitLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    set_header::{HeaderMetadata, response::SetMultipleResponseHeadersLayer},
    timeout::TimeoutLayer,
    trace::{DefaultOnFailure, DefaultOnResponse, TraceLayer},
};
use tracing::Level;

use crate::{infrastructure::config::MiddlewareConfig, middleware::error_response};

const REQUEST_ID_HEADER: &str = "x-request-id";

/// 用 `ServiceBuilder` 把中间件栈包在整个应用之外。
///
/// 必须包住整个 Router，不能用 `Router::layer`：后者会把有状态中间件逐路由复制一份，
/// 全局并发上限与全局限流会因此失效。
pub fn apply(app: Router, config: &MiddlewareConfig) -> Router {
    let stack = ServiceBuilder::new()
        .layer(SetRequestIdLayer::new(request_id_header(), MakeRequestUuid))
        .layer(PropagateRequestIdLayer::new(request_id_header()))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request<Body>| {
                    let request_id = request
                        .headers()
                        .get(REQUEST_ID_HEADER)
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or_default();
                    tracing::info_span!(
                        "http_request",
                        method = %request.method(),
                        uri = %request.uri(),
                        request_id = %request_id,
                    )
                })
                .on_response(
                    DefaultOnResponse::new()
                        .level(Level::INFO)
                        .latency_unit(LatencyUnit::Millis),
                )
                .on_failure(
                    DefaultOnFailure::new()
                        .level(Level::ERROR)
                        .latency_unit(LatencyUnit::Millis),
                ),
        )
        .layer(SetMultipleResponseHeadersLayer::overriding(
            security_headers(config),
        ))
        .layer(HandleErrorLayer::new(error_response::overload))
        .layer(LoadShedLayer::new())
        .layer(BufferLayer::new(config.backpressure_queue))
        .layer(ConcurrencyLimitLayer::new(config.max_concurrency))
        .layer(HandleErrorLayer::new(error_response::rate_limited))
        .layer(BufferLayer::new(config.backpressure_queue))
        .layer(LoadShedLayer::new())
        .layer(RateLimitLayer::new(
            config.rate_limit_requests,
            Duration::from_secs(config.rate_limit_period_secs),
        ))
        .layer(CompressionLayer::new())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(config.request_timeout_secs),
        ))
        .layer(RequestBodyLimitLayer::new(config.max_body_bytes))
        .service(app);

    Router::new().fallback_service(stack)
}

fn request_id_header() -> HeaderName {
    HeaderName::from_static(REQUEST_ID_HEADER)
}

fn security_headers(config: &MiddlewareConfig) -> Vec<HeaderMetadata<Response<Body>>> {
    let mut headers: Vec<HeaderMetadata<Response<Body>>> = vec![
        (
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        )
            .into(),
        (header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY")).into(),
        (
            header::REFERRER_POLICY,
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        )
            .into(),
    ];

    if config.hsts_enabled {
        headers.push(
            (
                header::STRICT_TRANSPORT_SECURITY,
                HeaderValue::from_static("max-age=31536000; includeSubDomains"),
            )
                .into(),
        );
    }

    headers
}
