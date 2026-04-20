use std::{env, net::SocketAddr, str::FromStr};

use anyhow::{Context, Result, anyhow};

/// 应用总配置。
///
/// 后续新增 Redis、消息队列、对象存储等能力时，建议继续集中放在这里，
/// 避免配置读取逻辑散落在各个模块里。
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub app_name: String,
    pub app_env: String,
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub jwt: JwtConfig,
    pub logging: LoggingConfig,
}

/// HTTP 服务监听配置。
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

impl ServerConfig {
    /// 将监听地址转换成 `SocketAddr`。
    pub fn socket_addr(&self) -> Result<SocketAddr> {
        let address = format!("{}:{}", self.host, self.port);
        address
            .parse::<SocketAddr>()
            .with_context(|| format!("无法解析服务监听地址: {address}"))
    }
}

/// 数据库连接配置。
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    pub min_connections: u32,
    pub max_connections: u32,
    pub connect_timeout_secs: u64,
    pub sqlx_logging: bool,
}

/// JWT 配置。
#[derive(Debug, Clone)]
pub struct JwtConfig {
    pub secret: String,
    pub issuer: String,
    pub audience: String,
    pub access_token_ttl_minutes: i64,
}

/// 日志配置。
#[derive(Debug, Clone)]
pub struct LoggingConfig {
    pub filter: String,
}

impl AppConfig {
    /// 从环境变量构建应用配置。
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
