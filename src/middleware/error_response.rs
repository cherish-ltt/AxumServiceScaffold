use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use tower::{BoxError, load_shed::error::Overloaded};

use crate::response::ApiResponse;

/// 并发或排队已超出容量（`LoadShed` 触发）时快速返回 503。
///
/// 限流/背压触发时是批量拒绝，逐条告警无信息量，这里不打日志；
/// 访问日志（TraceLayer）已记录每次请求与状态码。
pub async fn overload(err: BoxError) -> Response {
    if err.is::<Overloaded>() {
        return ApiResponse::<()>::error(StatusCode::SERVICE_UNAVAILABLE, "服务繁忙，请稍后重试")
            .into_response();
    }
    internal(err)
}

/// 限流窗口内额度已用尽（`LoadShed` 触发）时快速返回 429。
///
/// 限流/背压触发时是批量拒绝，逐条告警无信息量，这里不打日志；
/// 访问日志（TraceLayer）已记录每次请求与状态码。
pub async fn rate_limited(err: BoxError) -> Response {
    if err.is::<Overloaded>() {
        return ApiResponse::<()>::error(StatusCode::TOO_MANY_REQUESTS, "请求过于频繁，请稍后重试")
            .into_response();
    }
    internal(err)
}

fn internal(err: BoxError) -> Response {
    tracing::error!("中间件栈内部错误: {err}");
    ApiResponse::<()>::error(StatusCode::INTERNAL_SERVER_ERROR, "服务器内部错误").into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn overloaded_is_mapped_by_caller() {
        let overloaded = || -> BoxError { Box::new(Overloaded::new()) };

        assert_eq!(
            overload(overloaded()).await.status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            rate_limited(overloaded()).await.status(),
            StatusCode::TOO_MANY_REQUESTS
        );
    }

    #[tokio::test]
    async fn unexpected_error_is_internal() {
        let unexpected = || -> BoxError { Box::new(std::io::Error::other("boom")) };

        assert_eq!(
            overload(unexpected()).await.status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            rate_limited(unexpected()).await.status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
