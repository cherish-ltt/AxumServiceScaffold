use time::{UtcOffset, macros::format_description};
use tracing_appender::rolling::RollingFileAppender;
use tracing_subscriber::{
    EnvFilter,
    fmt::{self, time::OffsetTime},
    layer::SubscriberExt,
    util::SubscriberInitExt,
};

use crate::infrastructure::config::AppConfig;

/// 初始化全局日志。
///
/// 日志初始化应尽量早，这样启动阶段的配置错误与数据库错误也能被记录下来。
pub fn init(config: &AppConfig) {
    // 1. 配置时区和时间格式（东八区 UTC+8）
    let timer = OffsetTime::new(
        UtcOffset::from_hms(
            config.logging.utc_offset_hour,
            config.logging.utc_offset_minute,
            config.logging.utc_offset_second,
        )
        .unwrap(), // 手动指定时区偏移
        // 自定义时间格式：年-月-日 时:分:秒.毫秒
        format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:5]"),
    );

    // 2. 配置日志滚动策略
    let file_appender = RollingFileAppender::builder()
        // 按天切割（同时支持按大小切割，见下文）
        .rotation(config.logging.rotation.clone())
        // 日志文件名前缀（生成的文件如：app.2026-04-22.log）
        .filename_prefix(config.logging.filename_prefix.clone())
        // 日志文件名后缀
        .filename_suffix(config.logging.filename_suffix.clone())
        // 最多保留 60 个日志文件（按时间倒序保留最近 60 天）
        .max_log_files(config.logging.max_log_files)
        // .latest_symlink("app.latest.log") // 需要对应平台权限
        .build(config.logging.out_dir.clone())
        .expect("Failed to create file appender");

    // 3. 配置格式化层（输出到文件）
    let fmt_layer = fmt::layer()
        .with_writer(file_appender)
        .with_ansi(false)
        .with_timer(timer.clone()); // 关闭文件中的 ANSI 颜色码（避免乱码）

    // 4.控制台输出层（带颜色）
    let console_layer = fmt::layer().with_writer(std::io::stdout).with_timer(timer);

    // 5.配置日志过滤级别（来自.env）
    let env_filter = EnvFilter::try_new(config.logging.filter.clone())
        .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info"));

    // 6. 初始化全局订阅者
    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .with(console_layer)
        .init();
}
