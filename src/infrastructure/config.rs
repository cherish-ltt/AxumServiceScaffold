use std::{
    env,
    net::{IpAddr, SocketAddr},
    str::FromStr,
};

use anyhow::{Context, Result, anyhow};
use axum::http::HeaderValue;
use ipnet::IpNet;
use tracing_appender::rolling::Rotation;

mod jwt;
pub use jwt::{JwtConfig, JwtKeyConfig, JwtKeySet};

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
        if self.host.starts_with('[') {
            return format!("{}:{}", self.host, self.port)
                .parse::<SocketAddr>()
                .with_context(|| format!("invalid SERVER_HOST: {}", self.host));
        }
        let ip = self
            .host
            .parse::<IpAddr>()
            .with_context(|| format!("invalid SERVER_HOST: {}", self.host))?;
        Ok(SocketAddr::new(ip, self.port))
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

impl DatabaseConfig {
    pub fn is_in_memory(&self) -> Result<bool> {
        let options = self.sqlite_options()?;
        Ok(self.memory_options().0
            || matches!(
                options.get_filename().to_str(),
                Some(":memory:" | "file::memory:")
            ))
    }

    pub fn validate(&self) -> Result<()> {
        let options = self.sqlite_options()?;
        let (configured_memory, shared) = self.memory_options();
        let filename = options.get_filename().to_string_lossy();
        let in_memory =
            configured_memory || matches!(filename.as_ref(), ":memory:" | "file::memory:");
        let supported_url = url::Url::parse(&self.url).is_ok_and(|url| url.scheme() == "sqlite");
        anyhow::ensure!(
            !in_memory || shared,
            "in-memory SQLite requires shared cache"
        );
        if in_memory {
            anyhow::ensure!(
                filename.starts_with("file:") && filename != "file:",
                "in-memory SQLite requires a shared file: URI; use sqlite::memory: for an isolated pool"
            );
            anyhow::ensure!(
                supported_url,
                "invalid in-memory DATABASE_URL; use sqlite::memory: or sqlite:file:name?mode=memory&cache=shared"
            );
        } else {
            anyhow::ensure!(
                supported_url,
                "invalid DATABASE_URL: expected a SQLite URL supported by the database driver"
            );
        }
        Ok(())
    }

    fn sqlite_options(&self) -> Result<sqlx::sqlite::SqliteConnectOptions> {
        self.url
            .parse::<sqlx::sqlite::SqliteConnectOptions>()
            .map_err(|_| anyhow!("invalid DATABASE_URL: expected valid SQLite options"))
    }

    fn memory_options(&self) -> (bool, bool) {
        let location = self
            .url
            .trim_start_matches("sqlite://")
            .trim_start_matches("sqlite:");
        let (name, params) = location.split_once('?').unwrap_or((location, ""));
        let mut in_memory = name == ":memory:";
        let mut shared = in_memory;
        for (key, value) in url::form_urlencoded::parse(params.as_bytes()) {
            if key == "mode" && value == "memory" {
                in_memory = true;
                shared = true;
            } else if key == "cache" {
                shared = value == "shared";
            }
        }
        (in_memory, shared)
    }
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
    /// 文件日志批量落盘：缓冲满 N 条触发一次写入。
    pub batch_max_events: usize,
    /// 文件日志批量落盘：最长 N 秒强制写入一次。
    pub batch_flush_interval_secs: u64,
}

impl LoggingConfig {
    pub fn utc_offset(&self) -> Result<time::UtcOffset> {
        time::UtcOffset::from_hms(
            self.utc_offset_hour,
            self.utc_offset_minute,
            self.utc_offset_second,
        )
        .context("invalid log timezone offset configuration")
    }

    pub fn env_filter(&self) -> Result<tracing_subscriber::EnvFilter> {
        tracing_subscriber::EnvFilter::try_new(&self.filter)
            .map_err(|_| anyhow!("invalid LOG_FILTER expression"))
    }
}

/// HTTP 中间件容量参数，全部集中在 `.env`，业务代码不得内联这些数字。
#[derive(Debug, Clone)]
pub struct MiddlewareConfig {
    /// 从进入请求保护层到响应头生成（含排队）的超时，响应流发送不在范围内。
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
    /// 允许跨域访问的来源白名单（如 `https://a.example`）。
    ///
    /// 空列表表示不下发任何 CORS 放行头；单元素为 `*` 表示放行所有来源。
    /// 条目在 `AppConfig::from_env` 阶段校验为合法 header 值。
    pub cors_allowed_origins: Vec<String>,
    pub trusted_proxies: Vec<IpNet>,
    pub client_max_concurrency: usize,
    pub client_rate_limit_requests: u64,
    pub client_rate_limit_period_secs: u64,
    pub client_max_entries: usize,
    pub client_idle_secs: u64,
    pub slow_request_ms: u64,
}

impl AppConfig {
    pub fn validate_static(&self) -> Result<()> {
        self.validate_runtime()?;
        self.database.validate()?;
        self.logging.utc_offset()?;
        self.logging.env_filter()?;
        Ok(())
    }

    pub fn validate_runtime(&self) -> Result<()> {
        if self.app_env.eq_ignore_ascii_case("production") && cfg!(debug_assertions) {
            return Err(anyhow!(
                "production requires a release build: debug authentication routes are unsafe"
            ));
        }
        Ok(())
    }

    pub fn from_env() -> Result<Self> {
        let app_name = get_env_or("APP_NAME", "axum-service-scaffold");
        let app_env = get_env_or("APP_ENV", "development");

        let server = ServerConfig {
            host: get_env_or("SERVER_HOST", "127.0.0.1"),
            port: parse_env_or("SERVER_PORT", 8080u16)?,
        };
        server.socket_addr()?;

        let database = DatabaseConfig {
            url: get_env_or("DATABASE_URL", "sqlite://scaffold.db?mode=rwc"),
            min_connections: parse_env_or("DATABASE_MIN_CONNECTIONS", 1u32)?,
            max_connections: parse_env_or("DATABASE_MAX_CONNECTIONS", 10u32)?,
            connect_timeout_secs: parse_env_or("DATABASE_CONNECT_TIMEOUT_SECS", 8u64)?,
            idle_secs: parse_env_or("DATABASE_IDLE_SECS", 60u64)?,
            sqlx_logging: parse_env_or("DATABASE_SQLX_LOGGING", false)?,
        };

        let raw_keys = get_optional_env("JWT_KEY_SET")?;
        let jwt = JwtConfig {
            secret: get_optional_env("JWT_SECRET")?,
            key_set: if raw_keys.is_empty() {
                None
            } else {
                Some(
                    serde_json::from_str(&raw_keys)
                        .map_err(|_| anyhow!("invalid JWT_KEY_SET JSON"))?,
                )
            },
            issuer: get_env_or("JWT_ISSUER", "axum-service-scaffold"),
            audience: get_env_or("JWT_AUDIENCE", "axum-service-clients"),
            access_token_ttl_minutes: parse_env_or("JWT_ACCESS_TOKEN_TTL_MINUTES", 120i64)?,
        };

        jwt.validate_keys(app_env.eq_ignore_ascii_case("production"))?;
        if jwt.access_token_ttl_minutes <= 0 {
            return Err(anyhow!(
                "JWT_ACCESS_TOKEN_TTL_MINUTES must be greater than 0"
            ));
        }
        if database.max_connections == 0
            || database.min_connections == 0
            || database.min_connections > database.max_connections
        {
            return Err(anyhow!(
                "invalid database pool parameters: expected 0 < min_connections <= max_connections"
            ));
        }
        // 迁移文件与补列逻辑都依赖 SQLite 语法（pragma_table_info / sqlite 后端），
        // 其他后端会在启动时以难以定位的方式失败，所以在这里直接拒绝。
        let scheme = database.url.split(':').next().unwrap_or_default();
        if scheme != "sqlite" {
            return Err(anyhow!(
                "unsupported DATABASE_URL scheme: this scaffold only supports sqlite:// URLs"
            ));
        }
        if database.connect_timeout_secs == 0 || database.idle_secs == 0 {
            return Err(anyhow!(
                "database connect timeout and idle timeout must be greater than 0"
            ));
        }

        let max_log_files = parse_env_or("LOG_MAX_LOG_FILES", 60_usize)?;
        if max_log_files == 0 {
            return Err(anyhow!("LOG_MAX_LOG_FILES must be greater than 0"));
        }

        let batch_max_events = parse_env_or("LOG_BATCH_MAX_EVENTS", 50_usize)?;
        let batch_flush_interval_secs = parse_env_or("LOG_BATCH_FLUSH_INTERVAL_SECS", 3_u64)?;
        if batch_max_events == 0 || batch_flush_interval_secs == 0 {
            return Err(anyhow!("log batch parameters must be greater than 0"));
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
                value => return Err(anyhow!("unsupported log rotation strategy: {value}")),
            },
            max_log_files,
            out_dir: get_env_or("LOG_OUT_DIR", "./log"),
            batch_max_events,
            batch_flush_interval_secs,
        };

        let trusted_proxies = parse_proxies(&get_env_or("MIDDLEWARE_TRUSTED_PROXIES", ""))?;
        // 信任所有对端等于把 X-Forwarded-For 交给客户端自报，按 IP 的额度随之失效；
        // 前缀长度为 0 的网段（`*` 与手写的 0.0.0.0/0）在启动时直接拒绝。
        if app_env.eq_ignore_ascii_case("production")
            && trusted_proxies.iter().any(|net| net.prefix_len() == 0)
        {
            return Err(anyhow!(
                "MIDDLEWARE_TRUSTED_PROXIES must not trust every peer in production"
            ));
        }

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
            cors_allowed_origins: parse_origins(&get_env_or(
                "MIDDLEWARE_CORS_ALLOWED_ORIGINS",
                "",
            ))?,
            trusted_proxies,
            client_max_concurrency: parse_env_or("MIDDLEWARE_CLIENT_MAX_CONCURRENCY", 32_usize)?,
            client_rate_limit_requests: parse_env_or(
                "MIDDLEWARE_CLIENT_RATE_LIMIT_REQUESTS",
                1024_u64,
            )?,
            client_rate_limit_period_secs: parse_env_or(
                "MIDDLEWARE_CLIENT_RATE_LIMIT_PERIOD_SECS",
                1_u64,
            )?,
            client_max_entries: parse_env_or("MIDDLEWARE_CLIENT_MAX_ENTRIES", 16384_usize)?,
            client_idle_secs: parse_env_or("MIDDLEWARE_CLIENT_IDLE_SECS", 60_u64)?,
            slow_request_ms: parse_env_or("MIDDLEWARE_SLOW_REQUEST_MS", 1000_u64)?,
        };

        if middleware.request_timeout_secs == 0
            || middleware.max_body_bytes == 0
            || middleware.max_concurrency == 0
            || middleware.backpressure_queue == 0
            || middleware.rate_limit_requests == 0
            || middleware.rate_limit_period_secs == 0
            || middleware.client_max_concurrency == 0
            || middleware.client_rate_limit_requests == 0
            || middleware.client_rate_limit_period_secs == 0
            || middleware.client_max_entries == 0
            || middleware.client_idle_secs == 0
            || middleware.slow_request_ms == 0
        {
            return Err(anyhow!(
                "MIDDLEWARE capacity parameters must be greater than 0"
            ));
        }

        if middleware.client_idle_secs < middleware.client_rate_limit_period_secs {
            return Err(anyhow!(
                "MIDDLEWARE_CLIENT_IDLE_SECS must be at least the client rate limit period"
            ));
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

fn get_optional_env(key: &str) -> Result<String> {
    match env::var(key) {
        Ok(value) => Ok(value),
        Err(env::VarError::NotPresent) => Ok(String::new()),
        Err(env::VarError::NotUnicode(_)) => Err(anyhow!("{key} must contain valid Unicode")),
    }
}

fn get_env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

/// 把逗号分隔的来源白名单解析成列表，非法条目直接拒绝启动。
fn parse_origins(raw: &str) -> Result<Vec<String>> {
    let mut origins = Vec::new();
    for origin in raw
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        HeaderValue::from_str(origin).map_err(|error| {
            anyhow!("invalid MIDDLEWARE_CORS_ALLOWED_ORIGINS entry {origin}: {error}")
        })?;
        origins.push(origin.to_string());
    }

    Ok(origins)
}

/// `*` 的展开形式：包含判断只看网段，信任全部 IPv4 与 IPv6 即等价于信任所有直连对端。
const TRUST_ALL_PROXIES: [&str; 2] = ["0.0.0.0/0", "::/0"];

/// 解析可信代理列表：接受 CIDR 或单个 IP，`*` 表示信任所有直连对端。
///
/// `*` 与具体条目混写会拒绝启动，避免「只多信任一个网段」被误读成放行全部。
fn parse_proxies(raw: &str) -> Result<Vec<IpNet>> {
    let entries: Vec<&str> = raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect();
    match entries.as_slice() {
        ["*"] => TRUST_ALL_PROXIES
            .iter()
            .map(|net| parse_proxy(net))
            .collect(),
        entries if entries.contains(&"*") => Err(anyhow!(
            "MIDDLEWARE_TRUSTED_PROXIES: `*` cannot be combined with explicit entries"
        )),
        entries => entries.iter().map(|value| parse_proxy(value)).collect(),
    }
}

fn parse_proxy(value: &str) -> Result<IpNet> {
    value
        .parse::<IpNet>()
        .or_else(|_| value.parse::<IpAddr>().map(IpNet::from))
        .map_err(|_| anyhow!("invalid MIDDLEWARE_TRUSTED_PROXIES entry: {value}"))
}

fn parse_env_or<T>(key: &str, default: T) -> Result<T>
where
    T: FromStr + Clone,
    <T as FromStr>::Err: std::fmt::Display,
{
    match env::var(key) {
        Ok(value) => value
            .parse::<T>()
            .map_err(|error| anyhow!("failed to parse environment variable {key}: {error}")),
        Err(_) => Ok(default),
    }
}
