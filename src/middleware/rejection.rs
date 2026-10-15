use axum::{
    body::Body,
    http::{HeaderMap, Request, Response, StatusCode, header},
    response::IntoResponse,
};
use tower::{Layer, Service};

use crate::response::ApiResponse;

/// 把框架层产生的错误响应改写为统一响应结构。
///
/// `tower-http` 的 `TimeoutLayer` 返回**空体**的 `408`，`RequestBodyLimit` 返回
/// **纯文本** `length limit exceeded` 的 `413`，两者都不符合本服务的统一响应结构，
/// 会让客户端需要处理第二种错误格式。这一层把它们统一成 `ApiResponse`。
///
/// 改写判据是「状态码 >= 400 且响应不是 JSON」：业务错误与中间件拒绝都已经由
/// `ApiResponse` 渲染（带 `content-type: application/json`），不会被二次改写。
#[derive(Clone, Copy)]
pub struct RejectionBodyLayer;

impl<S> Layer<S> for RejectionBodyLayer {
    type Service = RejectionBody<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RejectionBody { inner }
    }
}

#[derive(Clone)]
pub struct RejectionBody<S> {
    inner: S,
}

impl<S> Service<Request<Body>> for RejectionBody<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let fut = self.inner.call(req);

        Box::pin(async move { fut.await.map(normalize) })
    }
}

fn normalize(response: Response<Body>) -> Response<Body> {
    let status = response.status();
    if !status.is_client_error() && !status.is_server_error() {
        return response;
    }
    if is_json(response.headers()) {
        return response;
    }

    ApiResponse::<()>::error(status, default_message(status)).into_response()
}

fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"))
}

/// 框架层错误的稳定文案；未列举的状态码退回标准原因短语，保证响应始终可读。
fn default_message(status: StatusCode) -> &'static str {
    match status {
        StatusCode::REQUEST_TIMEOUT => "Request timeout",
        StatusCode::PAYLOAD_TOO_LARGE => "Request body too large",
        _ => status.canonical_reason().unwrap_or("Request rejected"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_errors_are_kept_as_is() {
        let response =
            ApiResponse::<()>::error(StatusCode::NOT_FOUND, "Route not found").into_response();

        let normalized = normalize(response);
        assert_eq!(normalized.status(), StatusCode::NOT_FOUND);
        assert!(is_json(normalized.headers()));
    }

    #[test]
    fn non_json_errors_get_the_unified_envelope() {
        let response = Response::builder()
            .status(StatusCode::PAYLOAD_TOO_LARGE)
            .body(Body::from("length limit exceeded"))
            .expect("构造响应");

        let normalized = normalize(response);
        assert_eq!(normalized.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(
            normalized.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }

    #[test]
    fn successful_responses_are_untouched() {
        let response = Response::builder()
            .status(StatusCode::OK)
            .body(Body::from("plain text"))
            .expect("构造响应");

        let normalized = normalize(response);
        assert_eq!(normalized.headers().get(header::CONTENT_TYPE), None);
    }

    #[test]
    fn messages_cover_framework_rejections() {
        assert_eq!(
            default_message(StatusCode::REQUEST_TIMEOUT),
            "Request timeout"
        );
        assert_eq!(
            default_message(StatusCode::PAYLOAD_TOO_LARGE),
            "Request body too large"
        );
        assert_eq!(default_message(StatusCode::BAD_GATEWAY), "Bad Gateway");
    }
}
