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

        let logging = LoggingConfig {
            filter: get_env_or("LOG_FILTER", "info,tower_http=info"),
            utc_offset_hour: parse_env_or("LOG_UTC_OFFSET_HOUR", 0_i8)?,
            utc_offset_minute: parse_env_or("LOG_UTC_OFFSET_MINUTE", 0_i8)?,
            utc_offset_second: parse_env_or("LOG_UTC_OFFSET_SECOND", 0_i8)?,
            filename_prefix: get_env_or("LOG_FILENAME_PREFIX", "app"),
            filename_suffix: get_env_or("LOG_FILENAME_SUFFIX", "log"),
            rotation: match get_env_or("LOG_FILTER", "Rotation::DAILY").as_str() {
                "Rotation::DAILY" => Rotation::DAILY,
                "Rotation::HOURLY" => Rotation::HOURLY,
                "Rotation::MINUTELY" => Rotation::MINUTELY,
                "Rotation::NEVER" => Rotation::NEVER,
                "Rotation::WEEKLY" => Rotation::WEEKLY,
                _ => Rotation::DAILY,
            },
            max_log_files: parse_env_or("LOG_MAX_LOG_FILES", 30_usize)?,
            out_dir: get_env_or("LOG_OUT_DIR", "/var/log/axum-app"),
        };

        Ok(Self {
            app_name,
            app_env,
            server,
            database,
            jwt,
            logging,
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
