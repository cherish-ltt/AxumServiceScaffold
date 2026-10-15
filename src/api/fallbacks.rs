use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::response::ApiResponse;

/// 未匹配任何路由的 404 响应。
///
/// 与业务错误共用 `ApiResponse` 结构，避免同一服务出现两种响应体格式；
/// 文案固定，不反射请求路径，避免把用户输入带进响应体。
pub async fn not_found() -> Response {
    ApiResponse::<()>::error(StatusCode::NOT_FOUND, "Route not found").into_response()
}

/// 路径匹配但方法不允许的 405 响应。
///
/// axum 会在调用本处理器前补上 `Allow` 头，因此状态码语义不变，
/// 只是把空的响应体换成统一结构。
pub async fn method_not_allowed(method: axum::http::Method, path: String) -> Response {
    tracing::debug!(%method, %path, "method not allowed");

    ApiResponse::<()>::error(StatusCode::METHOD_NOT_ALLOWED, "Method not allowed").into_response()
}
