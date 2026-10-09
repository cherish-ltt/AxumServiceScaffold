use axum_service_scaffold::infrastructure::config::{
    AppConfig, DatabaseConfig, JwtConfig, LoggingConfig, MiddlewareConfig, ServerConfig,
};
use axum_service_scaffold::logging;
use tracing_appender::rolling::Rotation;
use uuid::Uuid;

const TEST_JWT_SECRET: &str = "logging-test-secret-that-is-long-enough";

fn test_config(out_dir: &str, utc_offset_hour: i8, utc_offset_minute: i8) -> AppConfig {
    AppConfig {
        app_name: "axum-service-scaffold".to_string(),
        app_env: "development".to_string(),
        server: ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 0,
        },
        database: DatabaseConfig {
            url: "sqlite://:memory:".to_string(),
            min_connections: 1,
            max_connections: 1,
            connect_timeout_secs: 5,
            idle_secs: 30,
            sqlx_logging: false,
        },
        jwt: JwtConfig {
            secret: TEST_JWT_SECRET.to_string(),
            issuer: "test-issuer".to_string(),
            audience: "test-audience".to_string(),
            access_token_ttl_minutes: 120,
        },
        logging: LoggingConfig {
            filter: "info".to_string(),
            utc_offset_hour,
            utc_offset_minute,
            utc_offset_second: 0,
            filename_prefix: "logging-test".to_string(),
            filename_suffix: "log".to_string(),
            rotation: Rotation::NEVER,
            max_log_files: 2,
            out_dir: out_dir.to_string(),
            batch_max_events: 50,
            batch_flush_interval_secs: 3,
        },
        middleware: MiddlewareConfig {
            request_timeout_secs: 10,
            max_body_bytes: 2 * 1024 * 1024,
            max_concurrency: 256,
            backpressure_queue: 256,
            rate_limit_requests: 1000,
            rate_limit_period_secs: 1,
            hsts_enabled: false,
        },
    }
}

#[test]
fn logging_init_creates_log_file_in_configured_dir() {
    let out_dir = std::env::temp_dir().join(format!("axum-scaffold-log-test-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&out_dir).expect("创建临时日志目录");

    let config = test_config(&out_dir.to_string_lossy(), 8, 0);
    let guard = match logging::init(&config) {
        Ok(guard) => guard,
        Err(e) => panic!("日志初始化应成功: {e}"),
    };

    tracing::info!("logging 初始化测试日志");
    drop(guard);

    let entries = std::fs::read_dir(&out_dir).expect("读取日志目录");
    assert!(
        entries.count() > 0,
        "日志目录中应已创建日志文件: {}",
        out_dir.display()
    );
}

#[test]
fn logging_init_rejects_invalid_utc_offset() {
    // time crate 的 UtcOffset 分量合法范围为 ±25 小时、±59 分秒，99 分钟必然非法。
    let config = test_config("/tmp/axum-scaffold-invalid-offset", 0, 99);

    let error = match logging::init(&config) {
        Ok(_) => panic!("非法时区偏移应被拒绝"),
        Err(e) => e,
    };
    assert!(error.to_string().contains("日志时区偏移配置无效"));
}
