use std::{sync::Arc, time::Duration};

use axum::{
    body::Body,
    http::{Request, Response, StatusCode},
};
use tokio::sync::Semaphore;
use tower::{Layer, Service, ServiceExt as _};

use crate::middleware::error_response::Rejection;

/// 并发许可门：限制同时在途的请求数，并对「等待许可」的时间设上限。
///
/// 替代 `tower::ConcurrencyLimitLayer` 的原因：后者在许可耗尽时通过 `poll_ready`
/// 返回 Pending，等待发生在中间件栈之外（axum 先 await readiness 再 `call`），
/// 因此任何超时层都覆盖不到排队时间。这里把等待放进自己持有的 future，
/// 等待超过 `wait_timeout` 即返回 503，排队时长因此有确定上界。
///
/// 许可用 `OwnedSemaphorePermit` 持有，请求完成、超时或被取消（future 被丢弃）时
/// 随 drop 释放，不需要显式归还。
#[derive(Clone)]
pub struct ConcurrencyGateLayer {
    semaphore: Arc<Semaphore>,
    wait_timeout: Duration,
}

impl ConcurrencyGateLayer {
    pub fn new(max_concurrency: usize, wait_timeout: Duration) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrency)),
            wait_timeout,
        }
    }

    /// 当前可用许可数，供测试与运行期观测使用。
    pub fn available_permits(&self) -> usize {
        self.semaphore.available_permits()
    }
}

impl<S> Layer<S> for ConcurrencyGateLayer {
    type Service = ConcurrencyGate<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ConcurrencyGate {
            inner,
            semaphore: Arc::clone(&self.semaphore),
            wait_timeout: self.wait_timeout,
        }
    }
}

/// 并发许可门中间件。
#[derive(Clone)]
pub struct ConcurrencyGate<S> {
    inner: S,
    semaphore: Arc<Semaphore>,
    wait_timeout: Duration,
}

impl<S> Service<Request<Body>> for ConcurrencyGate<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    /// 始终报告就绪：等待许可的语义在 `call` 里实现，才能被等待超时约束。
    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let semaphore = Arc::clone(&self.semaphore);
        let wait_timeout = self.wait_timeout;
        let mut inner = self.inner.clone();

        Box::pin(async move {
            // 先取到许可再调用内层：否则请求在拿到许可前就已经开始处理，
            // 「并发上限」会退化成只限制响应返回的速度。
            match tokio::time::timeout(wait_timeout, semaphore.acquire_owned()).await {
                Ok(Ok(permit)) => {
                    // `poll_ready` 被刻意实现为「始终就绪」（否则等待会跑到栈外、失去等待超时），
                    // 因此这里必须自己先把内层服务推到就绪：内层可能是有状态的 `Buffer`，
                    // 未 poll_reserve 直接 call 会 panic。
                    inner.ready().await?;
                    let result = inner.call(req).await;
                    drop(permit);
                    result
                },
                Ok(Err(_closed)) => Ok(Rejection::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Service is busy, please retry later",
                    "concurrency_closed",
                )
                .into_response()),
                Err(_elapsed) => Ok(Rejection::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Service is busy, please retry later",
                    "queue_wait_timeout",
                )
                .into_response()),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    #[tokio::test]
    async fn permit_is_released_when_the_inner_future_is_dropped() {
        let layer = ConcurrencyGateLayer::new(1, Duration::from_secs(30));
        let mut service = layer.layer(tower::service_fn(|_req: Request<Body>| async {
            // 永不返回，用于模拟被取消的在途请求
            std::future::pending::<Result<Response<Body>, std::convert::Infallible>>().await
        }));

        {
            let call = service.call(Request::new(Body::empty()));
            tokio::pin!(call);
            // 50ms 内不会完成：首个请求已拿到许可并卡在内层 future 上
            assert!(
                tokio::time::timeout(Duration::from_millis(50), &mut call)
                    .await
                    .is_err()
            );
            assert_eq!(layer.available_permits(), 0);
        }

        // 取消首个请求（future 被丢弃）后许可必须归还
        assert_eq!(layer.available_permits(), 1);
    }

    #[tokio::test]
    async fn waiting_longer_than_the_budget_is_rejected_with_503() {
        let layer = ConcurrencyGateLayer::new(1, Duration::from_millis(50));
        let mut gate = layer.layer(tower::service_fn(|_req: Request<Body>| async {
            tokio::time::sleep(Duration::from_millis(500)).await;
            Ok::<_, std::convert::Infallible>(Response::new(Body::empty()))
        }));

        let holder = tokio::spawn({
            let mut holder = gate.clone();
            async move {
                holder
                    .ready()
                    .await
                    .expect("就绪")
                    .call(Request::new(Body::empty()))
                    .await
            }
        });
        // 等第一个请求占住唯一许可
        tokio::time::sleep(Duration::from_millis(50)).await;

        let response = gate
            .ready()
            .await
            .expect("就绪")
            .call(Request::new(Body::empty()))
            .await
            .expect("等待超时应返回响应而非错误");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let _ = holder.await;
    }
}
