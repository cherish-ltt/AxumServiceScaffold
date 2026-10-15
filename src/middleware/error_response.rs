use axum::{
    body::Body,
    http::{Response, StatusCode},
    response::IntoResponse,
};
use tower::{BoxError, load_shed::error::Overloaded};

use crate::{metrics, response::ApiResponse};

/// 中间件层的统一拒绝描述。
///
/// 拒绝响应的正文必须复用 `ApiResponse::error`，并给指标一个固定 `reason` 标签，
/// 便于在 PromQL 里按拒绝原因分开统计。
#[derive(Debug, Clone, Copy)]
pub struct Rejection {
    pub status: StatusCode,
    pub message: &'static str,
    pub reason: &'static str,
}

impl Rejection {
    pub const fn new(status: StatusCode, message: &'static str, reason: &'static str) -> Self {
        Self {
            status,
            message,
            reason,
        }
    }

    /// 记录拒绝指标并生成响应。
    ///
    /// 批量拒绝（限流、背压、队列等待、连接上限）不逐条打日志：在高并发拒绝下逐条告警
    /// 没有信息量，还会撑爆日志。访问日志与指标已覆盖到每次拒绝。
    pub fn into_response(self) -> Response<Body> {
        metrics::record_rejection(self.reason);

        ApiResponse::<()>::error(self.status, self.message).into_response()
    }
}

/// 并发上限或背压队列已满（`LoadShed` 触发）→ 503。
pub fn overload_mapper(err: &BoxError) -> Option<Response<Body>> {
    err.is::<Overloaded>().then(|| {
        Rejection::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Service is busy, please retry later",
            "overload",
        )
        .into_response()
    })
}

/// 限流窗口内额度用尽（`LoadShed` 触发）→ 429。
pub fn rate_limited_mapper(err: &BoxError) -> Option<Response<Body>> {
    err.is::<Overloaded>().then(|| {
        Rejection::new(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many requests, please retry later",
            "rate_limited",
        )
        .into_response()
    })
}

/// 未被识别的中间件错误：留在栈顶收敛为 500，并记录内部错误日志。
///
/// 这是唯一一处对 5xx 打 `ERROR` 的中间件路径：预期内的 429/503 批量拒绝在各自的
/// 映射层就变成了响应，不会走到这里。
pub async fn internal(err: BoxError) -> Response<Body> {
    tracing::error!("middleware stack internal error: {err}");
    ApiResponse::<()>::error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn overloaded_error_is_mapped_to_specific_responses() {
        let overloaded = || -> BoxError { Box::new(Overloaded::new()) };

        assert_eq!(
            overload_mapper(&overloaded())
                .expect("应被识别为过载")
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            rate_limited_mapper(&overloaded())
                .expect("应被识别为限流")
                .status(),
            StatusCode::TOO_MANY_REQUESTS
        );
    }

    #[tokio::test]
    async fn unexpected_error_is_not_mapped() {
        let unexpected = || -> BoxError { Box::new(std::io::Error::other("boom")) };

        assert!(overload_mapper(&unexpected()).is_none());
        assert!(rate_limited_mapper(&unexpected()).is_none());
        assert_eq!(
            internal(unexpected()).await.status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[tokio::test]
    async fn rejection_uses_unified_envelope() {
        let response = Rejection::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Service is busy, please retry later",
            "queue_wait_timeout",
        )
        .into_response();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("读取响应体");
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("解析响应体");

        assert_eq!(body["code"], 503);
        assert_eq!(body["message"], "Service is busy, please retry later");
    }
}
