use std::time::Duration;

use anyhow::{Result, anyhow};

use super::env::{get_env_or, parse_env_or};

/// SeaORM / SQLite 连接池参数。
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    pub min_connections: u32,
    pub max_connections: u32,
    pub connect_timeout_secs: u64,
    /// 从池中获取连接的最长等待时间（秒）。
    ///
    /// 必须短于 `MIDDLEWARE_REQUEST_TIMEOUT_SECS`：否则池耗尽时请求会先被整体超时
    /// 掐断，拿不到明确的「池等待超时」错误，也无法快速释放排队资源。
    pub acquire_timeout_secs: u64,
    pub idle_secs: u64,
    /// `/api/v1/system/ready` 探测数据库的等待上限（秒）。
    pub ready_probe_timeout_secs: u64,
    pub sqlx_logging: bool,
}

impl DatabaseConfig {
    pub fn from_env() -> Result<Self> {
        let config = Self {
            url: get_env_or("DATABASE_URL", "sqlite://scaffold.db?mode=rwc"),
            min_connections: parse_env_or("DATABASE_MIN_CONNECTIONS", 1_u32)?,
            max_connections: parse_env_or("DATABASE_MAX_CONNECTIONS", 10_u32)?,
            connect_timeout_secs: parse_env_or("DATABASE_CONNECT_TIMEOUT_SECS", 8_u64)?,
            acquire_timeout_secs: parse_env_or("DATABASE_ACQUIRE_TIMEOUT_SECS", 3_u64)?,
            idle_secs: parse_env_or("DATABASE_IDLE_SECS", 60_u64)?,
            ready_probe_timeout_secs: parse_env_or("DATABASE_READY_PROBE_TIMEOUT_SECS", 2_u64)?,
            sqlx_logging: parse_env_or("DATABASE_SQLX_LOGGING", false)?,
        };
        config.validate()?;

        Ok(config)
    }

    pub fn acquire_timeout(&self) -> Duration {
        Duration::from_secs(self.acquire_timeout_secs)
    }

    fn validate(&self) -> Result<()> {
        if self.max_connections == 0
            || self.min_connections == 0
            || self.min_connections > self.max_connections
        {
            return Err(anyhow!(
                "invalid database pool parameters: expected 0 < min_connections <= max_connections"
            ));
        }
        // 迁移文件与补列逻辑都依赖 SQLite 语法（pragma_table_info / sqlite 后端），
        // 其他后端会在启动时以难以定位的方式失败，所以在这里直接拒绝。
        let scheme = self.url.split(':').next().unwrap_or_default();
        if scheme != "sqlite" {
            return Err(anyhow!(
                "unsupported DATABASE_URL scheme `{scheme}`: this scaffold only supports sqlite:// URLs"
            ));
        }
        if self.connect_timeout_secs == 0 || self.idle_secs == 0 {
            return Err(anyhow!(
                "database connect timeout and idle timeout must be greater than 0"
            ));
        }
        if self.acquire_timeout_secs == 0 {
            return Err(anyhow!(
                "DATABASE_ACQUIRE_TIMEOUT_SECS must be greater than 0"
            ));
        }
        if self.ready_probe_timeout_secs == 0 {
            return Err(anyhow!(
                "DATABASE_READY_PROBE_TIMEOUT_SECS must be greater than 0"
            ));
        }

        Ok(())
    }
}
