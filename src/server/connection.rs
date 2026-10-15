use std::time::Duration;

use hyper_util::{
    rt::{TokioExecutor, TokioTimer},
    server::conn::auto,
};

use crate::infrastructure::config::ServerConfig;

/// 按配置构建 hyper 连接构建器。
///
/// 这里集中设置审计缺失的连接层参数：
/// - `timer`：**必须**显式设置。hyper 的 `header_read_timeout` 只有在设置了 `Timer`
///   之后才会生效（hyper 1.9 的默认值是 30 秒，但没有 timer 时该超时永远不会触发，
///   慢速发头可以无限占用连接）；`axum::serve` 从不设置它。
/// - `header_read_timeout`：既约束慢速发头，也约束 keep-alive 等待下一个请求头的时间，
///   因此空闲连接同样会在超时后被关闭（hyper 1.x 没有独立的 idle timeout）。
/// - `max_headers` / `max_buf_size`：显式收紧请求头数量与总大小，替代 hyper 默认的
///   100 个 / 约 400 KB。超限时 hyper 会直接回 431。
pub fn http1_builder(config: &ServerConfig) -> auto::Builder<TokioExecutor> {
    let mut builder = auto::Builder::new(TokioExecutor::new());
    builder
        .http1()
        .timer(TokioTimer::new())
        .header_read_timeout(Duration::from_secs(config.http1_header_read_timeout_secs))
        .max_headers(config.http1_max_headers)
        .max_buf_size(config.http1_max_buf_size);

    builder
}
