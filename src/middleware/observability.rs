use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, Response, header},
};
use tower::{Layer, Service};
use tracing::{Level, Span};

use crate::{
    metrics,
    middleware::{client_ip::ClientIp, route_slot::RouteSlot},
};

/// 未匹配任何路由时的固定路由标签。
///
/// 用固定值而不是原始 URI，避免扫描流量把指标基数打爆。
pub const UNMATCHED_ROUTE: &str = "unmatched";

/// 请求被取消（future 在应答前被丢弃，例如连接中断或进程关闭）时的状态标签。
const CANCELLED_STATUS: &str = "cancelled";

/// 访问日志与指标的配置。
#[derive(Clone, Copy)]
pub struct ObservabilityConfig {
    pub metrics_enabled: bool,
    pub access_log: bool,
}

/// 一次请求的观测中间件：写入结构化访问日志并记录指标。
///
/// 它是栈里最外层的请求级中间件之一，因此限流、背压、超时等「中间件自身产生的拒绝」
/// 也会经过这里，状态码与拒绝原因都能被统计到。
#[derive(Clone, Copy)]
pub struct ObservabilityLayer {
    config: ObservabilityConfig,
}

impl ObservabilityLayer {
    pub fn new(config: ObservabilityConfig) -> Self {
        Self { config }
    }
}

impl<S> Layer<S> for ObservabilityLayer {
    type Service = Observability<S>;

    fn layer(&self, inner: S) -> Self::Service {
        Observability {
            inner,
            config: self.config,
        }
    }
}

/// 观测中间件。
#[derive(Clone)]
pub struct Observability<S> {
    inner: S,
    config: ObservabilityConfig,
}

impl<S> Service<Request<Body>> for Observability<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = S::Response;
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

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        let slot = RouteSlot::new();
        req.extensions_mut().insert(slot.clone());

        let method = req.method().to_string();
        let path = req.uri().path().to_string();
        let request_id = header_value(req.headers(), "x-request-id");
        // 直连对端与解析后的来源 IP 都记下来：前者用于核对可信代理链路。
        let peer = req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0.ip().to_string());
        let client_ip = req
            .extensions()
            .get::<ClientIp>()
            .map(|ip| ip.0.to_string());
        let span = request_span(
            &method,
            &path,
            request_id.as_deref(),
            client_ip.as_deref(),
            peer.as_deref(),
        );
        let config = self.config;
        let fut = self.inner.call(req);
        let started = Instant::now();

        if config.metrics_enabled {
            metrics::inflight_started();
        }

        Box::pin(async move {
            let mut guard = RequestGuard {
                config,
                started,
                method,
                path,
                slot,
                span,
                recorded: false,
            };

            let result = fut.await;
            guard.finish(
                result
                    .as_ref()
                    .map(|response| response.status().as_u16())
                    .ok(),
            );

            result
        })
    }
}

/// 请求级记录的收尾者。
///
/// 之所以用一个带 `Drop` 的守卫：如果外层把本 future 丢弃（连接断开、进程关闭），
/// `finish` 不会被执行，在途计数就会永久偏大。守卫在 drop 时补记一次取消事件。
struct RequestGuard {
    config: ObservabilityConfig,
    started: Instant,
    method: String,
    path: String,
    slot: RouteSlot,
    span: Span,
    recorded: bool,
}

impl RequestGuard {
    fn finish(&mut self, status: Option<u16>) {
        self.recorded = true;
        let elapsed = self.started.elapsed();
        let route = self
            .slot
            .get()
            .unwrap_or_else(|| UNMATCHED_ROUTE.to_string());

        if self.config.metrics_enabled {
            metrics::inflight_finished();
            match status {
                Some(status) => {
                    metrics::record_request(&self.method, &route, status, elapsed.as_secs_f64())
                },
                // 取消的请求没有状态码，不记入请求总数（也没法算状态码分布）
                None => metrics::record_rejection("cancelled"),
            }
        }

        if self.config.access_log {
            emit_access_log(
                &self.span,
                LogRecord {
                    method: &self.method,
                    path: &self.path,
                    route: &route,
                    status,
                    elapsed,
                },
            );
        }
    }
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        if self.recorded {
            return;
        }
        self.finish(None);
    }
}

/// 一次访问日志事件需要携带的字段。
struct LogRecord<'a> {
    method: &'a str,
    path: &'a str,
    route: &'a str,
    status: Option<u16>,
    elapsed: Duration,
}

/// 状态码 → （日志级别，日志文案）。
///
/// 429 / 503 是预期内的批量拒绝：逐条 ERROR 会在过载时产出 GB 级日志，因此降到 DEBUG
/// （默认 `LOG_FILTER=info` 下不输出），由指标统计次数。真正的 5xx 仍为 ERROR。
/// 注意 503 还可能是业务侧 `AppError::unavailable`，那条路径在 `src/error.rs`
/// 里已经单独打了 ERROR，不会因为这里降级而丢失内部故障信息。
fn access_log_outcome(status: Option<u16>) -> (Level, &'static str) {
    match status {
        Some(status) if status >= 500 && status != 503 => (Level::ERROR, "request failed"),
        Some(503 | 429) => (Level::DEBUG, "request shed"),
        Some(status) if status >= 400 => (Level::WARN, "request rejected"),
        Some(_) => (Level::INFO, "request completed"),
        None => (Level::DEBUG, "request cancelled"),
    }
}

fn emit_access_log(span: &Span, record: LogRecord<'_>) {
    let (level, message) = access_log_outcome(record.status);
    let status = record
        .status
        .map(|status| status.to_string())
        .unwrap_or_else(|| CANCELLED_STATUS.to_string());
    let _entered = span.enter();

    // 事件宏的级别必须是编译期常量，无法用运行时 `level` 变量驱动，因此按级别分派。
    match level {
        Level::ERROR => log_error(&status, &record, message),
        Level::WARN => log_warn(&status, &record, message),
        Level::DEBUG => log_debug(&status, &record, message),
        Level::INFO | Level::TRACE => log_info(&status, &record, message),
    }
}

fn log_error(status: &str, record: &LogRecord<'_>, message: &'static str) {
    tracing::error!(
        status = %status,
        route = record.route,
        path = record.path,
        method = record.method,
        latency_ms = record.elapsed.as_millis() as u64,
        message,
    );
}

fn log_warn(status: &str, record: &LogRecord<'_>, message: &'static str) {
    tracing::warn!(
        status = %status,
        route = record.route,
        path = record.path,
        method = record.method,
        latency_ms = record.elapsed.as_millis() as u64,
        message,
    );
}

fn log_debug(status: &str, record: &LogRecord<'_>, message: &'static str) {
    tracing::debug!(
        status = %status,
        route = record.route,
        path = record.path,
        method = record.method,
        latency_ms = record.elapsed.as_millis() as u64,
        message,
    );
}

fn log_info(status: &str, record: &LogRecord<'_>, message: &'static str) {
    tracing::info!(
        status = %status,
        route = record.route,
        path = record.path,
        method = record.method,
        latency_ms = record.elapsed.as_millis() as u64,
        message,
    );
}

fn request_span(
    method: &str,
    path: &str,
    request_id: Option<&str>,
    client_ip: Option<&str>,
    peer: Option<&str>,
) -> Span {
    tracing::info_span!(
        "http_request",
        method = method,
        path = path,
        request_id = request_id.unwrap_or_default(),
        client_ip = client_ip.unwrap_or_default(),
        peer_ip = peer.unwrap_or_default(),
    )
}

fn header_value(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::HeaderName::from_bytes(name.as_bytes()).expect("静态头名称合法"))
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string())
}
