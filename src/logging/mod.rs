use std::time::Duration;

use anyhow::{Context, Result};
use time::{UtcOffset, macros::format_description};
use tracing_subscriber::{
    EnvFilter,
    fmt::{self, time::OffsetTime},
    layer::SubscriberExt,
    util::SubscriberInitExt,
};

use crate::infrastructure::config::AppConfig;

mod batch;
mod file_writer;
mod format;

pub use batch::BatchGuard;
pub use file_writer::LogFileWriter;
pub use format::{ConsoleFieldsMarker, FileFieldsMarker, LineFormat, LogFields, LogFormatter};

/// 初始化全局日志。
///
/// 日志初始化应尽量早，这样启动阶段的配置错误与数据库错误也能被记录下来。
///
/// 文件层采用分批写入：事件先进入内存缓冲，满 `LOG_BATCH_MAX_EVENTS` 条或每
/// `LOG_BATCH_FLUSH_INTERVAL_SECS` 秒落盘一次，避免高频场景下逐条写文件浪费 CPU；
/// 控制台层保持实时输出。返回的 `BatchGuard` 在 drop 时停止刷盘线程并落盘剩余缓冲。
pub fn init(config: &AppConfig) -> Result<BatchGuard> {
    // 1. 配置时区和时间格式（默认东八区 UTC+8）
    let offset = UtcOffset::from_hms(
        config.logging.utc_offset_hour,
        config.logging.utc_offset_minute,
        config.logging.utc_offset_second,
    )
    .context("invalid log timezone offset configuration")?;
    let timer = OffsetTime::new(
        offset,
        // 自定义时间格式：年-月-日 时:分:秒.毫秒
        format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:5]"),
    );

    // 2. 配置日志过滤级别（来自 .env）。先于文件句柄校验，配置非法时不留下任何副作用。
    let env_filter = EnvFilter::try_new(config.logging.filter.clone())
        .with_context(|| format!("invalid LOG_FILTER: {}", config.logging.filter))?;

    // 3. 自建文件写入器：时间轮转 + 单文件大小上限 + 文件数量保留
    let file_writer = LogFileWriter::new(&config.logging)?;

    // 4. 配置格式化层（输出到文件，分批写入）
    let batch_writer = batch::BatchFileWriter::new(
        file_writer,
        config.logging.batch_max_events,
        Duration::from_secs(config.logging.batch_flush_interval_secs),
        config.logging.max_events_per_sec,
    );
    let fmt_layer = fmt::layer()
        // 先格式化整行再写入，保证每条日志恰好触发一次底层写入，分批计数才精确
        .event_format(LineFormat::new(LogFormatter::new(
            config.logging.format,
            timer.clone(),
        )))
        // 字段格式化器必须与事件格式化器配套，且不与控制台层共用缓存（见 format.rs）
        .fmt_fields(LogFields::<FileFieldsMarker>::new(config.logging.format))
        .with_ansi(false)
        .with_writer(batch_writer.clone());

    // 5. 控制台输出层（实时输出，不参与分批与限速）
    let console_layer = fmt::layer()
        .event_format(LineFormat::new(LogFormatter::new(
            config.logging.format,
            timer,
        )))
        .fmt_fields(LogFields::<ConsoleFieldsMarker>::new(config.logging.format))
        // JSON 里不能带 ANSI 转义，否则解析器会失败
        .with_ansi(config.logging.format == crate::infrastructure::config::LogFormat::Text)
        .with_writer(std::io::stdout);

    // 6. 初始化全局订阅者
    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .with(console_layer)
        .try_init()
        .context("failed to initialize global log subscriber")?;

    // 7. 启动定时刷盘线程
    let guard = batch_writer.start_flusher();

    Ok(guard)
}

/// 打印本次启动实际生效的配置，便于事后追溯启动那一刻的参数。
///
/// 以手绘 ASCII 列表框输出，按配置分组；多行消息由 fmt 层逐行打印，
/// 控制台与文件层行为一致。数据库连接串中的口令、JWT 密钥与指标令牌只输出脱敏结果。
pub fn log_startup_config(config: &AppConfig) {
    const WIDTH: usize = 64;
    let equals = format!("+{}+", "=".repeat(WIDTH));
    let dashes = format!("+{}+", "-".repeat(WIDTH));
    let mut lines = vec![
        String::new(),
        equals.clone(),
        format!("|{:^width$}|", "", width = WIDTH),
        format!("|{:^width$}|", "startup configuration", width = WIDTH),
        format!("|{:^width$}|", "", width = WIDTH),
        equals,
    ];

    let app = config;
    let server = &config.server;
    let database = &config.database;
    let jwt = &config.jwt;
    let logger = &config.logging;
    let middleware = &config.middleware;
    let metrics = &config.metrics;

    push_section(
        &mut lines,
        &dashes,
        "application",
        vec![
            format!("app_name={}", app.app_name),
            format!("app_env={}", app.app_env),
            format!("version={}", env!("CARGO_PKG_VERSION")),
            format!("build_profile={}", build_profile()),
            format!(
                "allow_insecure_build_profile={}",
                app.allow_insecure_build_profile
            ),
        ],
    );
    push_section(
        &mut lines,
        &dashes,
        "server",
        vec![
            format!("host={}", server.host),
            format!("port={}", server.port),
            format!("max_connections={}", server.max_connections),
            format!(
                "http1_header_read_timeout_secs={}",
                server.http1_header_read_timeout_secs
            ),
            format!("http1_max_headers={}", server.http1_max_headers),
            format!("http1_max_buf_size={}", server.http1_max_buf_size),
            format!("shutdown_timeout_secs={}", server.shutdown_timeout_secs),
        ],
    );
    push_section(
        &mut lines,
        &dashes,
        "database",
        vec![
            format!("url={}", mask_url(&database.url)),
            format!("min_connections={}", database.min_connections),
            format!("max_connections={}", database.max_connections),
            format!("connect_timeout_secs={}", database.connect_timeout_secs),
            format!("acquire_timeout_secs={}", database.acquire_timeout_secs),
            format!("idle_secs={}", database.idle_secs),
            format!(
                "ready_probe_timeout_secs={}",
                database.ready_probe_timeout_secs
            ),
            format!("sqlx_logging={}", database.sqlx_logging),
        ],
    );
    push_section(
        &mut lines,
        &dashes,
        "jwt",
        vec![
            format!("secret={}", mask_secret(&jwt.secret)),
            format!("issuer={}", jwt.issuer),
            format!("audience={}", jwt.audience),
            format!("access_token_ttl_minutes={}", jwt.access_token_ttl_minutes),
        ],
    );
    push_section(
        &mut lines,
        &dashes,
        "logging",
        vec![
            format!("filter={}", logger.filter),
            format!("format={:?}", logger.format),
            format!("utc_offset_hour={}", logger.utc_offset_hour),
            format!("utc_offset_minute={}", logger.utc_offset_minute),
            format!("utc_offset_second={}", logger.utc_offset_second),
            format!("out_dir={}", logger.out_dir),
            format!("filename_prefix={}", logger.filename_prefix),
            format!("filename_suffix={}", logger.filename_suffix),
            format!("rotation={:?}", logger.rotation),
            format!("max_log_files={}", logger.max_log_files),
            format!("max_file_bytes={}", logger.max_file_bytes),
            format!("max_events_per_sec={}", logger.max_events_per_sec),
            format!("batch_max_events={}", logger.batch_max_events),
            format!(
                "batch_flush_interval_secs={}",
                logger.batch_flush_interval_secs
            ),
        ],
    );
    push_section(
        &mut lines,
        &dashes,
        "middleware",
        vec![
            format!("request_timeout_secs={}", middleware.request_timeout_secs),
            format!(
                "queue_wait_timeout_secs={}",
                middleware.queue_wait_timeout_secs
            ),
            format!("max_body_bytes={}", middleware.max_body_bytes),
            format!("max_uri_bytes={}", middleware.max_uri_bytes),
            format!("max_concurrency={}", middleware.max_concurrency),
            format!("backpressure_queue={}", middleware.backpressure_queue),
            format!("rate_limit_requests={}", middleware.rate_limit_requests),
            format!(
                "rate_limit_period_secs={}",
                middleware.rate_limit_period_secs
            ),
            format!("hsts_enabled={}", middleware.hsts.enabled),
            format!("hsts_max_age_secs={}", middleware.hsts.max_age_secs),
            format!(
                "hsts_include_subdomains={}",
                middleware.hsts.include_subdomains
            ),
            format!("hsts_preload={}", middleware.hsts.preload),
            format!("cors_allowed_origins={:?}", middleware.cors_allowed_origins),
            format!("allowed_hosts={:?}", middleware.allowed_hosts),
            format!("trusted_proxies={}", middleware.trusted_proxies.len()),
            format!("request_id_enabled={}", middleware.switches.request_id),
            format!("access_log_enabled={}", middleware.switches.access_log),
            format!("compression_enabled={}", middleware.switches.compression),
            format!(
                "security_headers_enabled={}",
                middleware.switches.security_headers
            ),
            format!("timeout_enabled={}", middleware.switches.timeout),
            format!("body_limit_enabled={}", middleware.switches.body_limit),
            format!(
                "concurrency_limit_enabled={}",
                middleware.switches.concurrency_limit
            ),
            format!("backpressure_enabled={}", middleware.switches.backpressure),
            format!("rate_limit_enabled={}", middleware.switches.rate_limit),
        ],
    );
    push_section(
        &mut lines,
        &dashes,
        "metrics",
        vec![
            format!("enabled={}", metrics.enabled),
            format!("token={}", mask_secret(&metrics.token)),
        ],
    );

    tracing::info!("{}", lines.join("\n"));
}

/// 当前构建画像，用于在日志中区分调试构建与 release 构建。
fn build_profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

/// 追加一个配置分节：标题行、键值行与分隔线。
fn push_section(lines: &mut Vec<String>, dashes: &str, title: &str, rows: Vec<String>) {
    lines.push(format!("| [{title}]"));
    for row in rows {
        lines.push(format!("|   {row}"));
    }
    lines.push(dashes.to_string());
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
