use std::{net::SocketAddr, str::FromStr};

use anyhow::{Context, Result, anyhow};

use super::env::{get_env_or, parse_env_or};

/// hyper 的 `max_buf_size` 下限（`hyper::proto::h1::io::MINIMUM_MAX_BUFFER_SIZE`）。
///
/// 低于该值 hyper 会在运行时断言失败，因此在配置阶段就拒绝。
pub const MIN_HTTP1_BUF_SIZE: usize = 8 * 1024;

/// 监听地址与连接层参数。
///
/// 这些参数作用于 TCP 连接本身（而非单个请求），由 `src/server` 的 accept 循环使用。
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    /// 同时打开的 TCP 连接上限，超限的新连接被立即关闭。
    pub max_connections: usize,
    /// HTTP/1 请求头读取超时（秒）。
    ///
    /// 同时约束两类连接：慢速发头（slowloris）与 keep-alive 等待下一个请求头。
    /// hyper 在等待下一个请求头时会重新武装该定时器，因此空闲连接也会被关闭。
    pub http1_header_read_timeout_secs: u64,
    /// HTTP/1 请求头数量上限，超限返回 431。
    pub http1_max_headers: usize,
    /// HTTP/1 读缓冲上限（字节），即请求头总大小的实际上限，超限返回 431。
    pub http1_max_buf_size: usize,
    /// 收到终止信号后等待在途连接结束的最长时间（秒），到期强制中止。
    pub shutdown_timeout_secs: u64,
}

impl ServerConfig {
    pub fn from_env() -> Result<Self> {
        let config = Self {
            host: get_env_or("SERVER_HOST", "127.0.0.1"),
            port: parse_env_or("SERVER_PORT", 8080u16)?,
            max_connections: parse_env_or("SERVER_MAX_CONNECTIONS", 4096_usize)?,
            http1_header_read_timeout_secs: parse_env_or(
                "SERVER_HTTP1_HEADER_READ_TIMEOUT_SECS",
                30_u64,
            )?,
            http1_max_headers: parse_env_or("SERVER_HTTP1_MAX_HEADERS", 64_usize)?,
            http1_max_buf_size: parse_env_or("SERVER_HTTP1_MAX_BUF_SIZE", 16 * 1024_usize)?,
            shutdown_timeout_secs: parse_env_or("SHUTDOWN_TIMEOUT_SECS", 20_u64)?,
        };
        config.validate()?;

        Ok(config)
    }

    pub fn socket_addr(&self) -> Result<SocketAddr> {
        let address = format!("{}:{}", self.host, self.port);
        address
            .parse::<SocketAddr>()
            .with_context(|| format!("failed to parse server listen address: {address}"))
    }

    fn validate(&self) -> Result<()> {
        if self.max_connections == 0 {
            return Err(anyhow!("SERVER_MAX_CONNECTIONS must be greater than 0"));
        }
        if self.http1_header_read_timeout_secs == 0 {
            return Err(anyhow!(
                "SERVER_HTTP1_HEADER_READ_TIMEOUT_SECS must be greater than 0"
            ));
        }
        if self.http1_max_headers == 0 {
            return Err(anyhow!("SERVER_HTTP1_MAX_HEADERS must be greater than 0"));
        }
        if self.http1_max_buf_size < MIN_HTTP1_BUF_SIZE {
            return Err(anyhow!(
                "SERVER_HTTP1_MAX_BUF_SIZE must be at least {MIN_HTTP1_BUF_SIZE} bytes"
            ));
        }
        if self.shutdown_timeout_secs == 0 {
            return Err(anyhow!("SHUTDOWN_TIMEOUT_SECS must be greater than 0"));
        }
        // 先做地址解析校验，避免只在校验端口号时报错信息不完整。
        SocketAddr::from_str(&format!("{}:{}", self.host, self.port)).with_context(|| {
            format!(
                "invalid SERVER_HOST/SERVER_PORT: {}:{}",
                self.host, self.port
            )
        })?;

        Ok(())
    }
}
