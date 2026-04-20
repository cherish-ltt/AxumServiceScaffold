use tracing_subscriber::{EnvFilter, fmt};

use crate::infrastructure::config::AppConfig;

/// 初始化全局日志。
///
/// 日志初始化应尽量早，这样启动阶段的配置错误与数据库错误也能被记录下来。
pub fn init(config: &AppConfig) {
    let env_filter = EnvFilter::try_new(config.logging.filter.clone())
        .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info"));

    let _ = fmt()
        .with_env_filter(env_filter)
        .with_target(true)
        .compact()
        .try_init();
}
