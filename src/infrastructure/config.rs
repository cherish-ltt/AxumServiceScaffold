use std::{env, net::SocketAddr, str::FromStr};

use anyhow::{Context, Result, anyhow};
use tracing_appender::rolling::Rotation;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub app_name: String,
    pub app_env: String,
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub jwt: JwtConfig,
    pub logging: LoggingConfig,
    pub middleware: MiddlewareConfig,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

impl ServerConfig {
    pub fn socket_addr(&self) -> Result<SocketAddr> {
        let address = format!("{}:{}", self.host, self.port);
        address
            .parse::<SocketAddr>()
            .with_context(|| format!("无法解析服务监听地址: {address}"))
    }
}

#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    pub min_connections: u32,
    pub max_connections: u32,
    pub connect_timeout_secs: u64,
    pub idle_secs: u64,
    pub sqlx_logging: bool,
}

#[derive(Debug, Clone)]
pub struct JwtConfig {
    pub secret: String,
    pub issuer: String,
    pub audience: String,
    pub access_token_ttl_minutes: i64,
}

#[derive(Debug, Clone)]
pub struct LoggingConfig {
    pub filter: String,
    pub utc_offset_hour: i8,
    pub utc_offset_minute: i8,
    pub utc_offset_second: i8,
    pub filename_prefix: String,
    pub filename_suffix: String,
    pub rotation: Rotation,
    pub max_log_files: usize,
    pub out_dir: String,
}

/// HTTP 中间件容量参数，全部集中在 `.env`，业务代码不得内联这些数字。
#[derive(Debug, Clone)]
pub struct MiddlewareConfig {
    /// 单个请求从进入服务到响应完成的整体超时（秒）。
    pub request_timeout_secs: u64,
    /// 请求体最大字节数。
    pub max_body_bytes: usize,
    /// 同时进入 handler 的最大请求数。
    pub max_concurrency: usize,
    /// 并发已满时允许排队等待的请求数，超出部分快速拒绝。
    pub backpressure_queue: usize,
    /// 全局限流窗口内允许的请求数。
    pub rate_limit_requests: u64,
    /// 全局限流窗口长度（秒）。
    pub rate_limit_period_secs: u64,
    /// 是否下发 HSTS 响应头，默认跟随 `APP_ENV`。
    pub hsts_enabled: bool,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        let app_name = get_env_or("APP_NAME", "axum-service-scaffold");
        let app_env = get_env_or("APP_ENV", "development");

        let server = ServerConfig {
            host: get_env_or("SERVER_HOST", "127.0.0.1"),
            port: parse_env_or("SERVER_PORT", 8080u16)?,
        };

        let database = DatabaseConfig {
            url: get_env_or("DATABASE_URL", "sqlite://scaffold.db?mode=rwc"),
            min_connections: parse_env_or("DATABASE_MIN_CONNECTIONS", 1u32)?,
            max_connections: parse_env_or("DATABASE_MAX_CONNECTIONS", 10u32)?,
            connect_timeout_secs: parse_env_or("DATABASE_CONNECT_TIMEOUT_SECS", 8u64)?,
            idle_secs: parse_env_or("DATABASE_IDLE_SECS", 30u64)?,
            sqlx_logging: parse_env_or("DATABASE_SQLX_LOGGING", false)?,
        };

        let jwt = JwtConfig {
            secret: get_required_env("JWT_SECRET")?,
            issuer: get_env_or("JWT_ISSUER", "axum-service-scaffold"),
            audience: get_env_or("JWT_AUDIENCE", "axum-service-clients"),
            access_token_ttl_minutes: parse_env_or("JWT_ACCESS_TOKEN_TTL_MINUTES", 120i64)?,
        };

        if jwt.secret.len() < 32 {
            return Err(anyhow!("JWT_SECRET 长度至少需要 32 个字符"));
        }
        if app_env.eq_ignore_ascii_case("production")
            && jwt.secret == "change-me-to-a-random-string-with-at-least-32-characters"
        {
            return Err(anyhow!("生产环境不能使用示例 JWT_SECRET"));
        }
        if jwt.access_token_ttl_minutes <= 0 {
            return Err(anyhow!("JWT_ACCESS_TOKEN_TTL_MINUTES 必须大于 0"));
        }
        if database.max_connections == 0
            || database.min_connections == 0
            || database.min_connections > database.max_connections
        {
            return Err(anyhow!(
                "数据库连接池参数无效：需要 0 < min_connections <= max_connections"
            ));
        }
        if database.connect_timeout_secs == 0 || database.idle_secs == 0 {
            return Err(anyhow!("数据库连接超时和空闲超时必须大于 0"));
        }

        let max_log_files = parse_env_or("LOG_MAX_LOG_FILES", 30_usize)?;
        if max_log_files == 0 {
            return Err(anyhow!("LOG_MAX_LOG_FILES 必须大于 0"));
        }

        let logging = LoggingConfig {
            filter: get_env_or("LOG_FILTER", "info,tower_http=info"),
            utc_offset_hour: parse_env_or("LOG_UTC_OFFSET_HOUR", 0_i8)?,
            utc_offset_minute: parse_env_or("LOG_UTC_OFFSET_MINUTE", 0_i8)?,
            utc_offset_second: parse_env_or("LOG_UTC_OFFSET_SECOND", 0_i8)?,
            filename_prefix: get_env_or("LOG_FILENAME_PREFIX", "app"),
            filename_suffix: get_env_or("LOG_FILENAME_SUFFIX", "log"),
            rotation: match get_env_or("LOG_ROTATION", "DAILY").as_str() {
                "DAILY" | "Rotation::DAILY" => Rotation::DAILY,
                "HOURLY" | "Rotation::HOURLY" => Rotation::HOURLY,
                "MINUTELY" | "Rotation::MINUTELY" => Rotation::MINUTELY,
                "NEVER" | "Rotation::NEVER" => Rotation::NEVER,
                "WEEKLY" | "Rotation::WEEKLY" => Rotation::WEEKLY,
                value => return Err(anyhow!("不支持的日志轮转策略: {value}")),
            },
            max_log_files,
            out_dir: get_env_or("LOG_OUT_DIR", "/var/log/axum-app"),
        };

        let middleware = MiddlewareConfig {
            request_timeout_secs: parse_env_or("MIDDLEWARE_REQUEST_TIMEOUT_SECS", 10_u64)?,
            max_body_bytes: parse_env_or("MIDDLEWARE_MAX_BODY_BYTES", 2 * 1024 * 1024_usize)?,
            max_concurrency: parse_env_or("MIDDLEWARE_MAX_CONCURRENCY", 256_usize)?,
            backpressure_queue: parse_env_or("MIDDLEWARE_BACKPRESSURE_QUEUE", 256_usize)?,
            rate_limit_requests: parse_env_or("MIDDLEWARE_RATE_LIMIT_REQUESTS", 32768_u64)?,
            rate_limit_period_secs: parse_env_or("MIDDLEWARE_RATE_LIMIT_PERIOD_SECS", 1_u64)?,
            hsts_enabled: parse_env_or(
                "MIDDLEWARE_HSTS_ENABLED",
                app_env.eq_ignore_ascii_case("production"),
            )?,
        };

        if middleware.request_timeout_secs == 0
            || middleware.max_body_bytes == 0
            || middleware.max_concurrency == 0
            || middleware.backpressure_queue == 0
            || middleware.rate_limit_requests == 0
            || middleware.rate_limit_period_secs == 0
        {
            return Err(anyhow!("MIDDLEWARE 容量参数必须大于 0"));
        }

        Ok(Self {
            app_name,
            app_env,
            server,
            database,
            jwt,
            logging,
            middleware,
        })
    }
}

fn get_required_env(key: &str) -> Result<String> {
    env::var(key).with_context(|| format!("缺少必需环境变量: {key}"))
}

fn get_env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

fn parse_env_or<T>(key: &str, default: T) -> Result<T>
where
    T: FromStr + Clone,
    <T as FromStr>::Err: std::fmt::Display,
{
    match env::var(key) {
        Ok(value) => value
            .parse::<T>()
            .map_err(|error| anyhow!("环境变量 {key} 解析失败: {error}")),
        Err(_) => Ok(default),
    }
}
