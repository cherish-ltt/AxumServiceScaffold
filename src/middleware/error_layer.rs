use std::convert::Infallible;

use axum::{
    body::Body,
    http::{Request, Response},
};
use tower::{BoxError, Layer, Service};

/// 把内层服务的 `Infallible` 错误转成 `BoxError`。
///
/// 中间件栈需要统一在 `BoxError` 上才能自由地按开关增减层：
/// `tower::load_shed::LoadShed` 与 `tower::limit::RateLimit` 的错误类型是 `BoxError`，
/// 而 axum Router 的错误类型是 `Infallible`；`Option<L>` 形式的可选层要求两侧错误类型
/// 一致，所以最内层先做一次显式转换，栈顶再由 `HandleErrorLayer` 收敛回 `Infallible`。
///
/// 本层位于请求体限制之内，因此对请求体类型（`Limited<Body>`）与响应体类型都保持泛型。
#[derive(Clone, Copy)]
pub struct EraseErrorLayer;

impl<S> Layer<S> for EraseErrorLayer {
    type Service = EraseError<S>;

    fn layer(&self, inner: S) -> Self::Service {
        EraseError { inner }
    }
}

/// 错误类型转换中间件。
#[derive(Clone)]
pub struct EraseError<S> {
    inner: S,
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for EraseError<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>, Error = Infallible>,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
    ResBody: Send + 'static,
{
    type Response = Response<ResBody>;
    type Error = BoxError;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, BoxError>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        match self.inner.poll_ready(cx) {
            std::task::Poll::Ready(result) => std::task::Poll::Ready(result.map_err(Into::into)),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let fut = self.inner.call(req);

        Box::pin(async move {
            match fut.await {
                Ok(response) => Ok(response),
                Err(never) => match never {},
            }
        })
    }
}

/// 把特定错误映射成响应，其余错误原样上传。
///
/// 与 `axum::error_handling::HandleErrorLayer` 的区别：本层保留 `BoxError` 错误类型，
/// 因此可以在它外面继续按开关增减中间件层；`HandleErrorLayer` 会把错误类型收敛为
/// `Infallible`，导致外层无法再统一类型。
#[derive(Clone, Copy)]
pub struct ErrorToResponseLayer {
    mapper: fn(&BoxError) -> Option<Response<Body>>,
}

impl ErrorToResponseLayer {
    pub fn new(mapper: fn(&BoxError) -> Option<Response<Body>>) -> Self {
        Self { mapper }
    }
}

impl<S> Layer<S> for ErrorToResponseLayer {
    type Service = ErrorToResponse<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ErrorToResponse {
            inner,
            mapper: self.mapper,
        }
    }
}

/// 错误映射中间件。
#[derive(Clone)]
pub struct ErrorToResponse<S> {
    inner: S,
    mapper: fn(&BoxError) -> Option<Response<Body>>,
}

impl<S> Service<Request<Body>> for ErrorToResponse<S>
where
    S: Service<Request<Body>, Response = Response<Body>, Error = BoxError>,
    S::Future: Send + 'static,
{
    type Response = Response<Body>;
    type Error = BoxError;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, BoxError>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let mapper = self.mapper;
        let fut = self.inner.call(req);

        Box::pin(async move {
            match fut.await {
                Ok(response) => Ok(response),
                Err(error) => match mapper(&error) {
                    Some(response) => Ok(response),
                    None => Err(error),
                },
            }
        })
    }
}
