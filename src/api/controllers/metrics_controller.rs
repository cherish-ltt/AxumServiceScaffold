use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};

use crate::{container::Container, metrics::METRICS_CONTENT_TYPE, response::ApiResponse};

/// Prometheus 采集端点。
///
/// 与业务接口同端口暴露，因此必须带 `Authorization: Bearer <METRICS_TOKEN>`；
/// 令牌不匹配时返回 404 而不是 401/403，避免向探测者暴露端点存在。
/// 仅在 `METRICS_ENABLED=true` 时注册（见 `create_app`）。
pub fn router(metrics_enabled: bool) -> Router<Arc<Container>> {
    if !metrics_enabled {
        return Router::new();
    }

    Router::new().route(crate::metrics::METRICS_PATH, get(render))
}

async fn render(State(container): State<Arc<Container>>, headers: HeaderMap) -> Response {
    let Some(metrics) = &container.metrics else {
        return ApiResponse::<()>::error(StatusCode::NOT_FOUND, "Not found").into_response();
    };
    let Some(body) = metrics.render(&headers) else {
        return ApiResponse::<()>::error(StatusCode::NOT_FOUND, "Not found").into_response();
    };

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, METRICS_CONTENT_TYPE)],
        body,
    )
        .into_response()
}
