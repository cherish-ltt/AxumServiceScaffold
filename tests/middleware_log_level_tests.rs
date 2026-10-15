//! 日志级别策略与日志量的验收，单独成进程：`tracing` 的全局订阅者每进程只能安装一次。
//!
//! 审计发现的「15 秒 46 万条 ERROR」来自把 429/503 这类批量拒绝也打成 ERROR。
//! 本文件锁定策略：批量拒绝降到 DEBUG（默认 `LOG_FILTER=info` 下不输出），
//! 真正的 5xx 仍为 ERROR。

use std::sync::{Arc, Mutex};

mod common;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    routing::get,
};
use axum_service_scaffold::{infrastructure::config::AppConfig, middleware};
use tower::ServiceExt;

#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl LogBuffer {
    fn contents(&self) -> String {
        String::from_utf8(self.0.lock().expect("日志缓冲可读").clone()).expect("日志为 UTF-8")
    }
}

impl std::io::Write for &LogBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("日志缓冲可写").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn capture_logs() -> Arc<LogBuffer> {
    let buffer = Arc::new(LogBuffer::default());
    let subscriber = tracing_subscriber::fmt()
        .with_writer(Arc::clone(&buffer))
        .with_ansi(false)
        .without_time()
        .with_max_level(tracing::Level::DEBUG)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("安装全局日志订阅者");
    tracing::callsite::rebuild_interest_cache();

    buffer
}

fn test_config() -> AppConfig {
    let mut config = common::app_config(&std::env::temp_dir().to_string_lossy());
    config.middleware.rate_limit_requests = 2;
    config.middleware.rate_limit_period_secs = 60;

    config
}

/// 取出访问日志里某个状态码对应的那一行。
fn access_line(logs: &str, status: u16) -> String {
    logs.lines()
        .find(|line| {
            line.contains(&format!("status={status} "))
                || line.ends_with(&format!("status={status}"))
        })
        .unwrap_or_else(|| panic!("缺少 status={status} 的访问日志行:\n{logs}"))
        .to_string()
}

#[tokio::test]
async fn bulk_rejections_are_debug_while_real_5xx_are_error() {
    let logs = capture_logs();
    let app = middleware::apply(
        Router::new()
            .route("/ok", get(|| async { "ok" }))
            .route(
                "/error",
                get(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
            )
            .route(
                "/unavailable",
                get(|| async { StatusCode::SERVICE_UNAVAILABLE }),
            ),
        &test_config(),
    );

    // 限流额度是 2，且窗口为 60 秒：先取走正常响应（500/503 由业务返回），
    // 再发第三个请求必然被限流。顺序不能反，否则 5xx 会被限流挡住。
    for uri in ["/error", "/unavailable"] {
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("构造请求"),
            )
            .await
            .expect("请求应成功返回");
    }
    let rejected = app
        .oneshot(
            Request::builder()
                .uri("/ok")
                .body(Body::empty())
                .expect("构造请求"),
        )
        .await
        .expect("请求应成功返回");
    assert_eq!(rejected.status(), StatusCode::TOO_MANY_REQUESTS);

    let captured = logs.contents();
    let rate_limited = access_line(&captured, 429);
    assert!(
        rate_limited.contains("DEBUG"),
        "批量限流拒绝必须以 DEBUG 记录，避免过载时日志爆炸: {rate_limited}"
    );
    let unavailable = access_line(&captured, 503);
    assert!(
        unavailable.contains("DEBUG"),
        "背压/不可用必须与 429 同等对待（不逐条告警）: {unavailable}"
    );
    let internal = access_line(&captured, 500);
    assert!(
        internal.contains("ERROR"),
        "真正的 5xx 仍必须记录为 ERROR: {internal}"
    );
    assert_eq!(
        captured.matches("status=500").count(),
        1,
        "单次 5xx 只应产生一条 ERROR 访问日志:\n{captured}"
    );
}
