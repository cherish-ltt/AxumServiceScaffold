use tower::Layer;
use tower_http::limit::RequestBodyLimit;

/// 请求体大小限制（可开关）。
///
/// 关闭时把上限置为 `usize::MAX`：`RequestBodyLimit` 会改变响应体类型
/// （`limit::ResponseBody<...>`），因此这一层必须始终存在，只能用「不限制」来表达关闭。
#[derive(Clone, Copy)]
pub struct BodyLimitLayer {
    limit: usize,
    enabled: bool,
}

impl BodyLimitLayer {
    pub fn new(limit: usize, enabled: bool) -> Self {
        Self { limit, enabled }
    }
}

impl<S> Layer<S> for BodyLimitLayer {
    type Service = RequestBodyLimit<S>;

    fn layer(&self, inner: S) -> Self::Service {
        let limit = if self.enabled { self.limit } else { usize::MAX };

        RequestBodyLimit::new(inner, limit)
    }
}
