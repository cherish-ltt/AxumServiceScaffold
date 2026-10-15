//! Prometheus 指标：全局 recorder 安装、采集端点渲染与记录辅助函数。
//!
//! 指标与日志、追踪分层：日志负责单请求审计（含请求 ID），指标负责聚合（QPS、
//! 状态码分布、延迟分位数、在途请求数、拒绝原因），两者不互相替代。
//! 本模块不引入 OpenTelemetry：单体服务 + 进程内 SQLite，没有跨服务调用链。

use std::sync::OnceLock;

use anyhow::{Result, anyhow};
use axum::http::{HeaderMap, header};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};

use crate::infrastructure::config::MetricsConfig;

/// 采集端点路径。与业务接口同端口暴露，靠令牌鉴权隔离。
pub const METRICS_PATH: &str = "/metrics";

/// 采集端点响应的内容类型（Prometheus 文本格式 0.0.4）。
pub const METRICS_CONTENT_TYPE: &str = "text/plain; version=0.0.4";

/// 延迟直方图名称，分桶配置与记录代码共用同一个常量。
pub const LATENCY_METRIC: &str = "http_request_duration_seconds";

/// 延迟直方图分桶（秒）：覆盖 1ms 到 10s，便于用 PromQL 算 P50/P95/P99。
const LATENCY_BUCKETS: &[f64] = &[
    0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// 进程内唯一 recorder 的句柄缓存。
///
/// `metrics` 的全局 recorder 每进程只能安装一次；同一进程内多次装配容器
/// （集成测试逐个用例 bootstrap）必须复用同一个 recorder 与句柄。
static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// 串行化「检查并安装」：`OnceLock` 只能保证 `set` 一次，但两个线程可能同时发现它为空，
/// 随后第二次 `set_global_recorder` 会失败。集成测试并发装配容器会真实触发这条竞态。
static INSTALL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 已安装的指标出口：渲染句柄 + 采集令牌。
#[derive(Clone)]
pub struct MetricsHandle {
    handle: PrometheusHandle,
    token: String,
}

impl MetricsHandle {
    /// 安装全局 recorder 并返回采集句柄；`METRICS_ENABLED=false` 时返回 `None`。
    pub fn install(config: &MetricsConfig) -> Result<Option<Self>> {
        if !config.enabled {
            return Ok(None);
        }

        let _guard = INSTALL_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(handle) = HANDLE.get() {
            return Ok(Some(Self {
                handle: handle.clone(),
                token: config.token.clone(),
            }));
        }

        let recorder = PrometheusBuilder::new()
            .set_buckets_for_metric(Matcher::Full(LATENCY_METRIC.to_string()), LATENCY_BUCKETS)
            .map_err(|error| anyhow!("invalid latency histogram buckets: {error}"))?
            .build_recorder();
        let handle = recorder.handle();
        metrics::set_global_recorder(recorder)
            .map_err(|error| anyhow!("failed to install global metrics recorder: {error}"))?;
        let _ = HANDLE.set(handle.clone());

        Ok(Some(Self {
            handle,
            token: config.token.clone(),
        }))
    }

    /// 校验采集令牌并渲染指标文本；令牌不匹配时返回 `None`。
    pub fn render(&self, headers: &HeaderMap) -> Option<String> {
        self.token_matches(headers).then(|| self.handle.render())
    }

    fn token_matches(&self, headers: &HeaderMap) -> bool {
        let Some(value) = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
        else {
            return false;
        };
        let Some(provided) = value.strip_prefix("Bearer ") else {
            return false;
        };

        constant_time_eq(provided.as_bytes(), self.token.as_bytes())
    }
}

/// 定长比较，避免令牌比较泄露前缀信息。
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (left, right) in left.iter().zip(right.iter()) {
        diff |= left ^ right;
    }

    diff == 0
}

/// 记录一次请求：总数、状态码分布、延迟直方图。
///
/// 标签只用路由模板（如 `/api/v1/examples/{id}`），不含任何路径参数、查询串或请求 ID，
/// 避免指标基数随流量形态爆炸。未匹配路由统一为 `unmatched`。
pub fn record_request(method: &str, route: &str, status: u16, latency_seconds: f64) {
    metrics::counter!(
        "http_requests_total",
        "method" => method.to_string(),
        "route" => route.to_string(),
        "status" => status.to_string(),
    )
    .increment(1);
    metrics::histogram!(
        LATENCY_METRIC,
        "method" => method.to_string(),
        "route" => route.to_string(),
        "status" => status.to_string(),
    )
    .record(latency_seconds);
}

/// 记录一次拒绝，`reason` 为固定枚举值（见 `docs/MIDDLEWARE.md`）。
pub fn record_rejection(reason: &str) {
    metrics::counter!("http_rejections_total", "reason" => reason.to_string()).increment(1);
}

/// 在途请求数：进入可观测性层时 +1，离开时 -1。
pub fn inflight_started() {
    metrics::gauge!("http_requests_inflight").increment(1.0);
}

pub fn inflight_finished() {
    metrics::gauge!("http_requests_inflight").decrement(1.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_comparison_is_length_safe() {
        assert!(constant_time_eq(b"token-abcdefgh", b"token-abcdefgh"));
        assert!(!constant_time_eq(b"token-abcdefgh", b"token-abcdefgi"));
        assert!(!constant_time_eq(b"short", b"token-abcdefgh"));
    }
}
