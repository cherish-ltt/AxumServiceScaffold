//! 集成测试共用的配置构造。
//!
//! 新增配置字段时只需改这里一处，各测试只覆盖自己关心的项。数值与 `.env-example`
//! 的开发默认值保持一致，避免测试与运行期默认值出现两套真值。

#![allow(dead_code)]

use axum_service_scaffold::infrastructure::config::{
    AppConfig, DatabaseConfig, HstsConfig, JwtConfig, LogFormat, LogRotation, LoggingConfig,
    MetricsConfig, MiddlewareConfig, MiddlewareSwitches, ServerConfig,
};

pub const TEST_JWT_SECRET: &str = "test-secret-that-is-long-enough-for-validation";
pub const TEST_METRICS_TOKEN: &str = "test-metrics-token-0123456789";

pub fn server_config() -> ServerConfig {
    ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        max_connections: 64,
        http1_header_read_timeout_secs: 30,
        http1_max_headers: 64,
        http1_max_buf_size: 16 * 1024,
        shutdown_timeout_secs: 5,
    }
}

pub fn database_config(url: impl Into<String>) -> DatabaseConfig {
    DatabaseConfig {
        url: url.into(),
        min_connections: 1,
        max_connections: 2,
        connect_timeout_secs: 5,
        acquire_timeout_secs: 3,
        idle_secs: 30,
        ready_probe_timeout_secs: 2,
        sqlx_logging: false,
    }
}

pub fn jwt_config() -> JwtConfig {
    JwtConfig {
        secret: TEST_JWT_SECRET.to_string(),
        issuer: "test-issuer".to_string(),
        audience: "test-audience".to_string(),
        access_token_ttl_minutes: 120,
    }
}

pub fn logging_config(out_dir: &str) -> LoggingConfig {
    LoggingConfig {
        filter: "info".to_string(),
        format: LogFormat::Text,
        utc_offset_hour: 8,
        utc_offset_minute: 0,
        utc_offset_second: 0,
        filename_prefix: "test".to_string(),
        filename_suffix: "log".to_string(),
        rotation: LogRotation::Never,
        max_log_files: 2,
        max_file_bytes: 64 * 1024 * 1024,
        // 测试里不限速，保证「条数」断言稳定
        max_events_per_sec: 0,
        out_dir: out_dir.to_string(),
        batch_max_events: 50,
        batch_flush_interval_secs: 3,
    }
}

pub fn middleware_config() -> MiddlewareConfig {
    MiddlewareConfig {
        request_timeout_secs: 10,
        queue_wait_timeout_secs: 5,
        max_body_bytes: 2 * 1024 * 1024,
        max_uri_bytes: 8 * 1024,
        max_concurrency: 256,
        backpressure_queue: 256,
        rate_limit_requests: 32768,
        rate_limit_period_secs: 1,
        hsts: HstsConfig {
            enabled: false,
            max_age_secs: 31_536_000,
            include_subdomains: false,
            preload: false,
        },
        cors_allowed_origins: Vec::new(),
        allowed_hosts: Vec::new(),
        trusted_proxies: Vec::new(),
        switches: MiddlewareSwitches {
            request_id: true,
            access_log: true,
            compression: true,
            security_headers: true,
            timeout: true,
            body_limit: true,
            concurrency_limit: true,
            backpressure: true,
            rate_limit: true,
        },
    }
}

pub fn metrics_config(enabled: bool) -> MetricsConfig {
    MetricsConfig {
        enabled,
        token: TEST_METRICS_TOKEN.to_string(),
    }
}

/// 测试用完整配置：默认不启用指标（`/metrics` 不挂载）。
pub fn app_config(out_dir: &str) -> AppConfig {
    AppConfig {
        app_name: "axum-service-scaffold".to_string(),
        app_env: "development".to_string(),
        allow_insecure_build_profile: false,
        server: server_config(),
        database: database_config("sqlite://:memory:"),
        jwt: jwt_config(),
        logging: logging_config(out_dir),
        middleware: middleware_config(),
        metrics: metrics_config(false),
    }
}

/// 每个用例独立的临时 SQLite 文件数据库 URL。
pub fn temp_database_url(prefix: &str) -> String {
    format!(
        "sqlite://{}?mode=rwc",
        std::env::temp_dir()
            .join(format!(
                "axum-scaffold-{prefix}-{}.db",
                uuid::Uuid::now_v7()
            ))
            .display()
    )
}
