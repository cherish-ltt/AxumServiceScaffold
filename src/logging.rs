use anyhow::{Context, Result};
use time::{UtcOffset, macros::format_description};
use tracing_appender::{non_blocking::WorkerGuard, rolling::RollingFileAppender};
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
pub fn init(config: &AppConfig) -> Result<WorkerGuard> {
    // 1. 配置时区和时间格式（东八区 UTC+8）
    let offset = UtcOffset::from_hms(
        config.logging.utc_offset_hour,
        config.logging.utc_offset_minute,
        config.logging.utc_offset_second,
    )
    .context("日志时区偏移配置无效")?;
    let timer = OffsetTime::new(
        offset,
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
        .context("创建日志文件 appender 失败")?;
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);

    // 3. 配置格式化层（输出到文件）
    let fmt_layer = fmt::layer()
        .with_writer(file_writer)
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
        .try_init()
        .context("初始化全局日志订阅者失败")?;

    Ok(guard)
}

/// 打印本次启动实际生效的配置，便于事后追溯启动那一刻的参数。
///
/// 按配置分组结构化输出；数据库连接串中的口令与 JWT 密钥只输出脱敏结果。
pub fn log_startup_config(config: &AppConfig) {
    let server = &config.server;
    let database = &config.database;
    let jwt = &config.jwt;
    let logger = &config.logging;
    let middleware = &config.middleware;

    tracing::info!(app_name = %config.app_name, app_env = %config.app_env, "启动配置: 应用");
    tracing::info!(host = %server.host, port = server.port, "启动配置: 服务");
    tracing::info!(
        url = %mask_url(&database.url),
        min_connections = database.min_connections,
        max_connections = database.max_connections,
        connect_timeout_secs = database.connect_timeout_secs,
        idle_secs = database.idle_secs,
        sqlx_logging = database.sqlx_logging,
        "启动配置: 数据库"
    );
    tracing::info!(
        secret = %mask_secret(&jwt.secret),
        issuer = %jwt.issuer,
        audience = %jwt.audience,
        access_token_ttl_minutes = jwt.access_token_ttl_minutes,
        "启动配置: JWT"
    );
    tracing::info!(
        filter = %logger.filter,
        utc_offset_hour = logger.utc_offset_hour,
        utc_offset_minute = logger.utc_offset_minute,
        utc_offset_second = logger.utc_offset_second,
        out_dir = %logger.out_dir,
        filename_prefix = %logger.filename_prefix,
        filename_suffix = %logger.filename_suffix,
        rotation = ?logger.rotation,
        max_log_files = logger.max_log_files,
        "启动配置: 日志"
    );
    tracing::info!(
        request_timeout_secs = middleware.request_timeout_secs,
        max_body_bytes = middleware.max_body_bytes,
        max_concurrency = middleware.max_concurrency,
        backpressure_queue = middleware.backpressure_queue,
        rate_limit_requests = middleware.rate_limit_requests,
        rate_limit_period_secs = middleware.rate_limit_period_secs,
        hsts_enabled = middleware.hsts_enabled,
        "启动配置: 中间件"
    );
}

/// 隐藏连接串里的口令，保留其余结构以便核对目标库。
///
/// 同时处理 `scheme://user:password@host/db` 与 `scheme://host/db?password=xxx`
/// 两种写法：前者按最后一个 `@` 切分（口令里含 `@` 或 `:` 时也不会漏出），
/// 后者按参数名替换口令值。
fn mask_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_owned();
    };
    let end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);

    let authority = match authority.rsplit_once('@') {
        Some((userinfo, host)) => match userinfo.split_once(':') {
            Some((user, _)) => format!("{user}:***@{host}"),
            None => authority.to_owned(),
        },
        None => authority.to_owned(),
    };

    format!("{scheme}://{authority}{}", mask_query_password(tail))
}

/// 把查询参数里的口令值替换为 `***`。
fn mask_query_password(tail: &str) -> String {
    let Some((path, query)) = tail.split_once('?') else {
        return tail.to_owned();
    };
    let masked = query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((key, _)) if is_password_key(key) => format!("{key}=***"),
            _ => pair.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("&");

    format!("{path}?{masked}")
}

fn is_password_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "password" | "passwd" | "pwd"
    )
}

/// 密钥只输出长度，不泄露任何内容。
fn mask_secret(secret: &str) -> String {
    format!("***({} chars)", secret.chars().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_password_is_masked() {
        assert_eq!(
            mask_url("postgres://app:xxx@db:5432/orders"),
            "postgres://app:***@db:5432/orders"
        );
        assert_eq!(
            mask_url("mysql://root:xxx@db:3306/orders"),
            "mysql://root:***@db:3306/orders"
        );
        // 口令本身含 @ 或 : 时也不能漏出
        assert_eq!(
            mask_url("mysql://root:p@ss:word@db:3306/orders"),
            "mysql://root:***@db:3306/orders"
        );
        // 查询参数形式的口令
        assert_eq!(
            mask_url("postgres://db:5432/orders?user=app&password=xxx&sslmode=require"),
            "postgres://db:5432/orders?user=app&password=***&sslmode=require"
        );
        // 无口令的连接串保持原样
        assert_eq!(
            mask_url("mysql://root@db:3306/orders"),
            "mysql://root@db:3306/orders"
        );
        assert_eq!(
            mask_url("sqlite://scaffold.db?mode=rwc"),
            "sqlite://scaffold.db?mode=rwc"
        );
    }

    #[test]
    fn secret_content_is_not_printed() {
        let masked = mask_secret("change-me-change-me-change-me-32");

        assert_eq!(masked, "***(32 chars)");
        assert!(!masked.contains("change-me"));
    }
}
