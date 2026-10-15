use anyhow::{Result, anyhow};

mod database;
mod env;
mod jwt;
mod logging;
mod metrics;
mod middleware;
mod server;

pub use database::DatabaseConfig;
pub use jwt::JwtConfig;
pub use logging::{LogFormat, LogRotation, LoggingConfig};
pub use metrics::MetricsConfig;
pub use middleware::{HstsConfig, MiddlewareConfig, MiddlewareSwitches, TrustedProxy};
pub use server::{MIN_HTTP1_BUF_SIZE, ServerConfig};

use env::{get_env_or, parse_env_or};

/// 应用级配置聚合。
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub app_name: String,
    pub app_env: String,
    /// 显式豁免「生产环境构建画像约束」（`APP_ALLOW_INSECURE_PRODUCTION`）。
    ///
    /// 放开「生产环境禁止 debug 构建 / `docs` feature」这一条限制；调试路由与文档路由
    /// 仍然不会注册，只是不再拒绝启动。这是一条危险通路，只在受控部署时使用。
    pub allow_insecure_build_profile: bool,
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub jwt: JwtConfig,
    pub logging: LoggingConfig,
    pub middleware: MiddlewareConfig,
    pub metrics: MetricsConfig,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        let app_name = get_env_or("APP_NAME", "axum-service-scaffold");
        let app_env = get_env_or("APP_ENV", "development");
        let allow_insecure_build_profile = parse_env_or("APP_ALLOW_INSECURE_PRODUCTION", false)?;
        let allow_insecure_middleware =
            parse_env_or("MIDDLEWARE_ALLOW_INSECURE_PRODUCTION", false)?;

        validate_build_profile(&app_env, allow_insecure_build_profile)?;

        Ok(Self {
            server: ServerConfig::from_env()?,
            database: DatabaseConfig::from_env()?,
            jwt: JwtConfig::from_env(&app_env)?,
            logging: LoggingConfig::from_env()?,
            middleware: MiddlewareConfig::from_env(&app_env, allow_insecure_middleware)?,
            metrics: MetricsConfig::from_env(&app_env)?,
            app_name,
            app_env,
            allow_insecure_build_profile,
        })
    }

    /// 当前是否为生产环境。
    pub fn is_production(&self) -> bool {
        is_production_env(&self.app_env)
    }
}

pub fn is_production_env(app_env: &str) -> bool {
    app_env.eq_ignore_ascii_case("production")
}

/// 生产环境禁止携带调试构建或文档 feature。
///
/// 仅靠编译期 `cfg` 隔离是不够的：debug 构建下的 `dev-login` 可以伪造任意身份，
/// `docs` feature 会暴露无鉴权的 Swagger UI。这里在启动阶段直接拒绝，
/// 只有显式设置 `APP_ALLOW_INSECURE_PRODUCTION=true` 才降级为「不注册这些路由」。
fn validate_build_profile(app_env: &str, allow_insecure_build_profile: bool) -> Result<()> {
    if !is_production_env(app_env) || allow_insecure_build_profile {
        return Ok(());
    }

    let mut offenders = Vec::new();
    if cfg!(debug_assertions) {
        offenders.push("a debug build");
    }
    if cfg!(feature = "docs") {
        offenders.push("the `docs` feature (Swagger UI / OpenAPI)");
    }
    if offenders.is_empty() {
        return Ok(());
    }

    Err(anyhow!(
        "refusing to start: APP_ENV=production with {}. Build with `cargo build --release` without the `docs` feature, or set APP_ALLOW_INSECURE_PRODUCTION=true to run anyway (debug-only routes stay unregistered)",
        offenders.join(" and ")
    ))
}
