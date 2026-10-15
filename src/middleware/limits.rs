use axum::{
    body::Body,
    http::{Request, Response, StatusCode},
};
use tower::{Layer, Service};

use crate::middleware::error_response::Rejection;

/// URI 长度上限。
///
/// 该限制发生在 HTTP 解析之后：超长 URI 已经被 hyper 解析并构造出 `http::Uri`
/// （hyper 自身的上限是 65534 字节，超过才返回 414）。这一层的作用是把上限收紧到
/// 与反向代理同一量级，减少路由匹配与日志的无效开销，**不能**当作连接层防护。
/// 真正的连接层防护由 `SERVER_*` 参数与反向代理承担。
#[derive(Clone, Copy)]
pub struct UriLimitLayer {
    max_bytes: usize,
}

impl UriLimitLayer {
    pub fn new(max_bytes: usize) -> Self {
        Self { max_bytes }
    }
}

impl<S> Layer<S> for UriLimitLayer {
    type Service = UriLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        UriLimitService {
            inner,
            max_bytes: self.max_bytes,
        }
    }
}

/// URI 长度校验中间件。
#[derive(Clone)]
pub struct UriLimitService<S> {
    inner: S,
    max_bytes: usize,
}

impl<S> Service<Request<Body>> for UriLimitService<S>
where
    S: Service<Request<Body>, Response = Response<Body>>,
    S::Future: Send + 'static,
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
        if req.uri().to_string().len() > self.max_bytes {
            let rejection = Rejection::new(
                StatusCode::URI_TOO_LONG,
                "request URI is too long",
                "uri_too_long",
            );

            return Box::pin(async move { Ok(rejection.into_response()) });
        }

        Box::pin(self.inner.call(req))
    }
}
