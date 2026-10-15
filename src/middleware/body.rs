use axum::{
    body::{Body, Bytes, HttpBody},
    http::{Request, Response},
};
use tower::{BoxError, Layer, Service};

/// 把内层（压缩、请求体限制等）改变过的响应体类型收敛回 `axum::body::Body`。
///
/// `CompressionLayer` 与 `RequestBodyLimitLayer` 会把响应体包成
/// `CompressionBody<...>` / `limit::ResponseBody<...>`，而本仓库自己的中间件层
/// （指标、限流、错误映射）都按 `Response<Body>` 编写。这里在它们之外做一次类型收敛，
/// 让自定义层不需要为了兼容别人包装的 body 类型而泛型化。
#[derive(Clone, Copy)]
pub struct NormalizeBodyLayer;

impl<S> Layer<S> for NormalizeBodyLayer {
    type Service = NormalizeBody<S>;

    fn layer(&self, inner: S) -> Self::Service {
        NormalizeBody { inner }
    }
}

/// 响应体类型收敛中间件。
#[derive(Clone)]
pub struct NormalizeBody<S> {
    inner: S,
}

impl<S, B> Service<Request<Body>> for NormalizeBody<S>
where
    S: Service<Request<Body>, Response = Response<B>>,
    S::Future: Send + 'static,
    B: HttpBody<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
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

        Box::pin(async move { fut.await.map(|response| response.map(Body::new)) })
    }
}
