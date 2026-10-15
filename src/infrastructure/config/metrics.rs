use anyhow::{Result, anyhow};

use super::env::{get_env_or, parse_env_or};

/// Prometheus 指标出口配置。
#[derive(Debug, Clone)]
pub struct MetricsConfig {
    /// 是否挂载 `/metrics` 采集端点。
    pub enabled: bool,
    /// 采集端点要求的 Bearer 令牌；`/metrics` 与业务接口同端口暴露，
    /// 因此必须显式配置令牌，不能匿名可读。
    pub token: String,
}

impl MetricsConfig {
    /// 采集令牌的最小长度，避免写成 `1` 这类可猜值。
    pub const MIN_TOKEN_LEN: usize = 16;
    const EXAMPLE_TOKEN: &str = "change-me-to-a-random-metrics-token";

    pub fn from_env(app_env: &str) -> Result<Self> {
        let config = Self {
            enabled: parse_env_or("METRICS_ENABLED", true)?,
            token: get_env_or("METRICS_TOKEN", ""),
        };
        config.validate(app_env)?;

        Ok(config)
    }

    fn validate(&self, app_env: &str) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        if self.token.len() < Self::MIN_TOKEN_LEN {
            return Err(anyhow!(
                "METRICS_TOKEN must be at least {} characters long when METRICS_ENABLED=true",
                Self::MIN_TOKEN_LEN
            ));
        }
        // 示例令牌只在生产环境拒绝：开发环境保留 `.env-example` 可直接启动的体验。
        if app_env.eq_ignore_ascii_case("production") && self.token == Self::EXAMPLE_TOKEN {
            return Err(anyhow!(
                "example METRICS_TOKEN is not allowed in production environment"
            ));
        }

        Ok(())
    }
}
