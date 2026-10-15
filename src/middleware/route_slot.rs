use std::sync::{Arc, OnceLock};

use axum::{
    Router,
    body::Body,
    extract::MatchedPath,
    http::{Request, Response},
};
use tower::{Layer, Service};

/// 请求所属路由模板的槽位。
///
/// 指标与访问日志都需要「路由模板」而不是原始 URI（`/api/v1/examples/{id}` 而非
/// `/api/v1/examples/123`），否则标签基数会随路径参数爆炸。
///
/// `MatchedPath` 只在路由匹配之后才写入请求扩展，而指标层必须包在整个栈的最外层
/// （才能统计到中间件自身的拒绝），所以两层之间靠这个槽位传递模板：
/// 外层插入空槽位，路由层的捕获中间件在匹配后填入。
#[derive(Clone, Default)]
pub struct RouteSlot(Arc<OnceLock<String>>);

impl RouteSlot {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&self, template: &str) {
        let _ = self.0.set(template.to_string());
    }

    pub fn get(&self) -> Option<String> {
        self.0.get().cloned()
    }
}

/// 在路由匹配之后把模板写进 `RouteSlot`。
///
/// 该层无状态（每次调用只写请求自带的槽位），因此经 `Router::layer` 逐路由复制是
/// 安全的；有状态层（并发限制、限流）仍然只能装配在 `stack.rs` 的最外层。
#[derive(Clone, Copy, Default)]
pub struct CaptureRouteLayer;

impl<S> Layer<S> for CaptureRouteLayer {
    type Service = CaptureRoute<S>;

    fn layer(&self, inner: S) -> Self::Service {
        CaptureRoute { inner }
    }
}

/// 捕获路由模板的中间件。
#[derive(Clone)]
pub struct CaptureRoute<S> {
    inner: S,
}

impl<S> Service<Request<Body>> for CaptureRoute<S>
where
    S: Service<Request<Body>, Response = Response<Body>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        if let (Some(slot), Some(matched)) = (
            req.extensions().get::<RouteSlot>(),
            req.extensions().get::<MatchedPath>(),
        ) {
            slot.set(matched.as_str());
        }

        self.inner.call(req)
    }
}

/// 把模板捕获层挂到已注册路由上。
///
/// 必须在所有 `route` / `nest` 之后调用；未匹配路由不会进入该层，指标侧统一落到
/// 固定标签 `unmatched`。
pub fn capture_route_templates(app: Router) -> Router {
    app.layer(CaptureRouteLayer)
}
