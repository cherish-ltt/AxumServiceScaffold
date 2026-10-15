use std::env;
use std::sync::{Mutex, MutexGuard, OnceLock};

use axum_service_scaffold::infrastructure::config::{
    AppConfig, LogFormat, LogRotation, MIN_HTTP1_BUF_SIZE, ServerConfig,
};

/// 环境变量是进程级全局状态，所有依赖环境变量的测试必须串行执行，避免相互污染。
static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn set_var(key: &str, value: &str) {
    unsafe { env::set_var(key, value) };
}

fn remove_var(key: &str) {
    unsafe { env::remove_var(key) };
}

/// 断言结束（含 panic）时恢复环境变量的守卫：panic 展开也会执行 Drop。
struct EnvRestore {
    saved: Vec<(String, Option<String>)>,
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (key, value) in &self.saved {
            match value {
                Some(value) => set_var(key, value),
                None => remove_var(key),
            }
        }
    }
}

const EXAMPLE_SECRET: &str = "change-me-to-a-random-string-with-at-least-32-characters";
const CUSTOM_SECRET: &str = "unit-test-secret-that-is-definitely-long-enough";

/// 每个用例的最小必填基线：JWT 密钥与指标采集令牌。
///
/// 用例可以再用同名的环境变量覆盖它们（例如验证「缺失即拒绝」）。
fn baseline_env() -> Vec<(&'static str, Option<&'static str>)> {
    vec![
        ("JWT_SECRET", Some(CUSTOM_SECRET)),
        ("METRICS_TOKEN", Some(CUSTOM_SECRET)),
    ]
}

/// 临时设置环境变量后执行断言，结束时（含断言 panic）恢复原始值。
fn with_env<F: FnOnce()>(vars: &[(&str, Option<&str>)], assertion: F) {
    let _lock = env_lock();
    let mut all = baseline_env();
    all.extend_from_slice(vars);

    let saved: Vec<(String, Option<String>)> = all
        .iter()
        .map(|(key, _)| (key.to_string(), env::var(key).ok()))
        .collect();

    // 先挂上恢复守卫，再改环境变量：无论 assertion 是否 panic，都会恢复。
    let _restore = EnvRestore { saved };

    for (key, value) in &all {
        match value {
            Some(value) => set_var(key, value),
            None => remove_var(key),
        }
    }

    assertion();
}

/// `LogRotation` 的 Debug 输出为 PascalCase（如 Daily），统一小写后比较。
fn rotation_debug(rotation: LogRotation) -> String {
    format!("{rotation:?}").to_lowercase()
}

/// 验证「默认值」用例需要清空的环境变量清单。
const DEFAULT_VARS: &[(&str, Option<&str>)] = &[
    ("APP_NAME", None),
    ("APP_ENV", None),
    ("APP_ALLOW_INSECURE_PRODUCTION", None),
    ("MIDDLEWARE_ALLOW_INSECURE_PRODUCTION", None),
    ("SERVER_HOST", None),
    ("SERVER_PORT", None),
    ("SERVER_MAX_CONNECTIONS", None),
    ("SERVER_HTTP1_HEADER_READ_TIMEOUT_SECS", None),
    ("SERVER_HTTP1_MAX_HEADERS", None),
    ("SERVER_HTTP1_MAX_BUF_SIZE", None),
    ("SHUTDOWN_TIMEOUT_SECS", None),
    ("DATABASE_URL", None),
    ("DATABASE_MIN_CONNECTIONS", None),
    ("DATABASE_MAX_CONNECTIONS", None),
    ("DATABASE_CONNECT_TIMEOUT_SECS", None),
    ("DATABASE_ACQUIRE_TIMEOUT_SECS", None),
    ("DATABASE_IDLE_SECS", None),
    ("DATABASE_READY_PROBE_TIMEOUT_SECS", None),
    ("DATABASE_SQLX_LOGGING", None),
    ("JWT_ISSUER", None),
    ("JWT_AUDIENCE", None),
    ("JWT_ACCESS_TOKEN_TTL_MINUTES", None),
    ("LOG_FILTER", None),
    ("LOG_FORMAT", None),
    ("LOG_UTC_OFFSET_HOUR", None),
    ("LOG_UTC_OFFSET_MINUTE", None),
    ("LOG_UTC_OFFSET_SECOND", None),
    ("LOG_FILENAME_PREFIX", None),
    ("LOG_FILENAME_SUFFIX", None),
    ("LOG_ROTATION", None),
    ("LOG_MAX_LOG_FILES", None),
    ("LOG_MAX_FILE_BYTES", None),
    ("LOG_MAX_EVENTS_PER_SEC", None),
    ("LOG_OUT_DIR", None),
    ("LOG_BATCH_MAX_EVENTS", None),
    ("LOG_BATCH_FLUSH_INTERVAL_SECS", None),
    ("MIDDLEWARE_REQUEST_TIMEOUT_SECS", None),
    ("MIDDLEWARE_QUEUE_WAIT_TIMEOUT_SECS", None),
    ("MIDDLEWARE_MAX_BODY_BYTES", None),
    ("MIDDLEWARE_MAX_URI_BYTES", None),
    ("MIDDLEWARE_MAX_CONCURRENCY", None),
    ("MIDDLEWARE_BACKPRESSURE_QUEUE", None),
    ("MIDDLEWARE_RATE_LIMIT_REQUESTS", None),
    ("MIDDLEWARE_RATE_LIMIT_PERIOD_SECS", None),
    ("MIDDLEWARE_HSTS_ENABLED", None),
    ("MIDDLEWARE_HSTS_MAX_AGE_SECS", None),
    ("MIDDLEWARE_HSTS_INCLUDE_SUBDOMAINS", None),
    ("MIDDLEWARE_HSTS_PRELOAD", None),
    ("MIDDLEWARE_CORS_ALLOWED_ORIGINS", None),
    ("MIDDLEWARE_TRUSTED_PROXIES", None),
    ("HTTP_ALLOWED_HOSTS", None),
    ("METRICS_ENABLED", None),
];

#[test]
fn valid_config_loads_with_defaults() {
    with_env(DEFAULT_VARS, || {
        let config = AppConfig::from_env().expect("默认配置应可加载");

        assert_eq!(config.app_name, "axum-service-scaffold");
        assert_eq!(config.app_env, "development");
        assert!(!config.allow_insecure_build_profile);
        assert!(!config.is_production());
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.server.max_connections, 4096);
        assert_eq!(config.server.http1_header_read_timeout_secs, 30);
        assert_eq!(config.server.http1_max_headers, 64);
        assert_eq!(config.server.http1_max_buf_size, 16 * 1024);
        assert_eq!(config.server.shutdown_timeout_secs, 20);
        assert_eq!(config.database.url, "sqlite://scaffold.db?mode=rwc");
        assert_eq!(config.database.min_connections, 1);
        assert_eq!(config.database.max_connections, 10);
        assert_eq!(config.database.connect_timeout_secs, 8);
        assert_eq!(config.database.acquire_timeout_secs, 3);
        assert_eq!(config.database.idle_secs, 60);
        assert_eq!(config.database.ready_probe_timeout_secs, 2);
        assert!(!config.database.sqlx_logging);
        assert_eq!(config.jwt.issuer, "axum-service-scaffold");
        assert_eq!(config.jwt.audience, "axum-service-clients");
        assert_eq!(config.jwt.access_token_ttl_minutes, 120);
        assert_eq!(config.logging.filter, "info,tower_http=info");
        assert_eq!(config.logging.format, LogFormat::Text);
        assert_eq!(config.logging.filename_prefix, "app");
        assert_eq!(config.logging.filename_suffix, "log");
        assert_eq!(config.logging.max_log_files, 60);
        assert_eq!(config.logging.max_file_bytes, 64 * 1024 * 1024);
        assert_eq!(config.logging.max_events_per_sec, 2000);
        assert_eq!(config.logging.out_dir, "./log");
        assert_eq!(config.logging.batch_max_events, 50);
        assert_eq!(config.logging.batch_flush_interval_secs, 3);
        assert!(rotation_debug(config.logging.rotation).contains("daily"));

        assert_eq!(config.middleware.request_timeout_secs, 10);
        assert_eq!(config.middleware.queue_wait_timeout_secs, 5);
        assert_eq!(config.middleware.max_body_bytes, 2 * 1024 * 1024);
        assert_eq!(config.middleware.max_uri_bytes, 8 * 1024);
        assert_eq!(config.middleware.max_concurrency, 256);
        assert_eq!(config.middleware.backpressure_queue, 256);
        assert_eq!(config.middleware.rate_limit_requests, 32768);
        assert_eq!(config.middleware.rate_limit_period_secs, 1);
        assert!(!config.middleware.hsts.enabled);
        assert_eq!(config.middleware.hsts.max_age_secs, 31_536_000);
        assert!(!config.middleware.hsts.include_subdomains);
        assert!(!config.middleware.hsts.preload);
        assert!(config.middleware.cors_allowed_origins.is_empty());
        assert!(config.middleware.allowed_hosts.is_empty());
        assert!(config.middleware.trusted_proxies.is_empty());
        // 安全关键中间件默认全部启用
        assert!(config.middleware.switches.request_id);
        assert!(config.middleware.switches.security_headers);
        assert!(config.middleware.switches.timeout);
        assert!(config.middleware.switches.body_limit);
        assert!(config.middleware.switches.concurrency_limit);
        assert!(config.middleware.switches.backpressure);
        assert!(config.middleware.switches.rate_limit);

        assert!(config.metrics.enabled);
    });
}

#[test]
fn custom_overrides_are_applied() {
    with_env(
        &[
            ("APP_NAME", Some("custom-app")),
            ("APP_ENV", Some("staging")),
            ("SERVER_PORT", Some("9000")),
            ("SERVER_MAX_CONNECTIONS", Some("32")),
            ("SERVER_HTTP1_HEADER_READ_TIMEOUT_SECS", Some("5")),
            ("SERVER_HTTP1_MAX_HEADERS", Some("32")),
            ("SERVER_HTTP1_MAX_BUF_SIZE", Some("16384")),
            ("SHUTDOWN_TIMEOUT_SECS", Some("7")),
            ("JWT_ACCESS_TOKEN_TTL_MINUTES", Some("30")),
            ("DATABASE_SQLX_LOGGING", Some("true")),
            ("DATABASE_ACQUIRE_TIMEOUT_SECS", Some("2")),
            ("DATABASE_READY_PROBE_TIMEOUT_SECS", Some("1")),
            ("LOG_ROTATION", Some("HOURLY")),
            ("LOG_FORMAT", Some("json")),
            ("LOG_MAX_LOG_FILES", Some("7")),
            ("LOG_MAX_FILE_BYTES", Some("1024")),
            ("LOG_MAX_EVENTS_PER_SEC", Some("100")),
            ("LOG_BATCH_MAX_EVENTS", Some("100")),
            ("LOG_BATCH_FLUSH_INTERVAL_SECS", Some("5")),
        ],
        || {
            let config = AppConfig::from_env().expect("自定义配置应可加载");

            assert_eq!(config.app_name, "custom-app");
            assert_eq!(config.app_env, "staging");
            assert_eq!(config.server.port, 9000);
            assert_eq!(config.server.max_connections, 32);
            assert_eq!(config.server.http1_header_read_timeout_secs, 5);
            assert_eq!(config.server.http1_max_headers, 32);
            assert_eq!(config.server.http1_max_buf_size, 16 * 1024);
            assert_eq!(config.server.shutdown_timeout_secs, 7);
            assert_eq!(config.jwt.access_token_ttl_minutes, 30);
            assert!(config.database.sqlx_logging);
            assert_eq!(config.database.acquire_timeout_secs, 2);
            assert_eq!(config.database.ready_probe_timeout_secs, 1);
            assert_eq!(config.logging.format, LogFormat::Json);
            assert_eq!(config.logging.max_log_files, 7);
            assert_eq!(config.logging.max_file_bytes, 1024);
            assert_eq!(config.logging.max_events_per_sec, 100);
            assert_eq!(config.logging.batch_max_events, 100);
            assert_eq!(config.logging.batch_flush_interval_secs, 5);
            assert!(rotation_debug(config.logging.rotation).contains("hourly"));
        },
    );
}

#[test]
fn supported_log_rotation_variants_parse() {
    with_env(&[("LOG_ROTATION", Some("DAILY"))], || {
        for variant in ["daily", "hourly", "minutely", "weekly", "never"] {
            set_var("LOG_ROTATION", &variant.to_uppercase());
            let config = AppConfig::from_env().expect("支持的轮转策略应可解析");
            assert!(rotation_debug(config.logging.rotation).contains(variant));
        }

        for variant in ["Rotation::DAILY", "Rotation::WEEKLY"] {
            set_var("LOG_ROTATION", variant);
            let config = AppConfig::from_env().expect("兼容写法应可解析");
            let expected = variant.rsplit("::").next().unwrap_or("").to_lowercase();
            assert!(rotation_debug(config.logging.rotation).contains(&expected));
        }
    });
}

#[test]
fn unsupported_log_format_is_rejected() {
    with_env(&[("LOG_FORMAT", Some("xml"))], || {
        let error = AppConfig::from_env().expect_err("不支持的日志格式应被拒绝");
        assert!(error.to_string().contains("unsupported LOG_FORMAT"));
    });
}

#[test]
fn production_accepts_custom_secret_with_explicit_override() {
    with_env(
        &[
            ("APP_ENV", Some("production")),
            ("APP_ALLOW_INSECURE_PRODUCTION", Some("true")),
            ("HTTP_ALLOWED_HOSTS", Some("api.example.com")),
        ],
        || {
            let config = AppConfig::from_env().expect("显式豁免后生产配置应可加载");
            assert!(config.is_production());
            assert!(config.allow_insecure_build_profile);
            // 生产环境 HSTS 默认开启
            assert!(config.middleware.hsts.enabled);
        },
    );
}

/// 生产环境 + debug 构建（或 `docs` feature）默认拒绝启动。
///
/// 只靠编译期 `cfg` 隔离不够：debug 构建下的 `dev-login` 能伪造任意身份。
#[test]
fn production_with_debug_build_is_rejected_without_override() {
    with_env(
        &[
            ("APP_ENV", Some("production")),
            ("APP_ALLOW_INSECURE_PRODUCTION", None),
            ("MIDDLEWARE_ALLOW_INSECURE_PRODUCTION", None),
            ("HTTP_ALLOWED_HOSTS", Some("api.example.com")),
        ],
        || {
            let error = AppConfig::from_env().expect_err("生产环境使用 debug 构建必须拒绝启动");
            assert!(
                error.to_string().contains("refusing to start"),
                "报错信息不符合预期: {error}"
            );
        },
    );
}

#[test]
fn production_requires_allowed_hosts() {
    with_env(
        &[
            ("APP_ENV", Some("production")),
            ("APP_ALLOW_INSECURE_PRODUCTION", Some("true")),
            ("HTTP_ALLOWED_HOSTS", None),
        ],
        || {
            let error = AppConfig::from_env().expect_err("生产环境缺少 Host 白名单应拒绝启动");
            assert!(
                error.to_string().contains("HTTP_ALLOWED_HOSTS"),
                "报错信息不符合预期: {error}"
            );
        },
    );
}

/// 生产环境不允许通过普通开关关掉安全关键中间件，除非显式豁免。
#[test]
fn production_rejects_disabled_required_middleware() {
    with_env(
        &[
            ("APP_ENV", Some("production")),
            ("APP_ALLOW_INSECURE_PRODUCTION", Some("true")),
            ("MIDDLEWARE_ALLOW_INSECURE_PRODUCTION", Some("true")),
            ("HTTP_ALLOWED_HOSTS", Some("api.example.com")),
            ("MIDDLEWARE_BODY_LIMIT_ENABLED", Some("false")),
        ],
        || {
            let config = AppConfig::from_env().expect("中间件层显式豁免后允许关闭安全关键中间件");
            assert!(!config.middleware.switches.body_limit);
        },
    );

    with_env(
        &[
            ("APP_ENV", Some("production")),
            ("APP_ALLOW_INSECURE_PRODUCTION", Some("true")),
            ("MIDDLEWARE_ALLOW_INSECURE_PRODUCTION", None),
            ("HTTP_ALLOWED_HOSTS", Some("api.example.com")),
            ("MIDDLEWARE_BODY_LIMIT_ENABLED", Some("false")),
        ],
        || {
            let error = AppConfig::from_env().expect_err("生产环境关闭请求体限制应拒绝启动");
            assert!(
                error
                    .to_string()
                    .contains("required middleware disabled in production"),
                "报错信息不符合预期: {error}"
            );
        },
    );
}

#[test]
fn production_rejects_example_jwt_secret() {
    with_env(
        &[
            ("APP_ENV", Some("production")),
            ("APP_ALLOW_INSECURE_PRODUCTION", Some("true")),
            ("HTTP_ALLOWED_HOSTS", Some("api.example.com")),
            ("JWT_SECRET", Some(EXAMPLE_SECRET)),
        ],
        || {
            let result = AppConfig::from_env();
            assert!(result.is_err());
        },
    );
}

#[test]
fn production_rejects_example_metrics_token() {
    with_env(
        &[
            ("APP_ENV", Some("production")),
            ("APP_ALLOW_INSECURE_PRODUCTION", Some("true")),
            ("HTTP_ALLOWED_HOSTS", Some("api.example.com")),
            ("METRICS_TOKEN", Some("change-me-to-a-random-metrics-token")),
        ],
        || {
            let error = AppConfig::from_env().expect_err("生产环境使用示例令牌应被拒绝");
            assert!(error.to_string().contains("example METRICS_TOKEN"));
        },
    );
}

#[test]
fn short_metrics_token_is_rejected() {
    with_env(&[("METRICS_TOKEN", Some("short"))], || {
        let error = AppConfig::from_env().expect_err("过短的采集令牌应被拒绝");
        assert!(error.to_string().contains("METRICS_TOKEN must be at least"));
    });
}

#[test]
fn metrics_can_be_disabled() {
    with_env(
        &[("METRICS_ENABLED", Some("false")), ("METRICS_TOKEN", None)],
        || {
            let config = AppConfig::from_env().expect("关闭指标后不再要求令牌");
            assert!(!config.metrics.enabled);
        },
    );
}

#[test]
fn missing_jwt_secret_is_rejected() {
    with_env(&[("JWT_SECRET", None)], || {
        let error = AppConfig::from_env().expect_err("缺少 JWT_SECRET 应被拒绝");
        assert!(
            error
                .to_string()
                .contains("missing required environment variable")
        );
    });
}

#[test]
fn short_jwt_secret_is_rejected() {
    with_env(&[("JWT_SECRET", Some("too-short"))], || {
        let error = AppConfig::from_env().expect_err("过短的 JWT_SECRET 应被拒绝");
        assert!(error.to_string().contains("at least 32 characters"));
    });
}

#[test]
fn non_positive_token_ttl_is_rejected() {
    with_env(&[("JWT_ACCESS_TOKEN_TTL_MINUTES", Some("0"))], || {
        let error = AppConfig::from_env().expect_err("非正数 TTL 应被拒绝");
        assert!(error.to_string().contains("must be greater than 0"));
    });
}

#[test]
fn zero_min_connections_is_rejected() {
    with_env(&[("DATABASE_MIN_CONNECTIONS", Some("0"))], || {
        let error = AppConfig::from_env().expect_err("0 最小连接数应被拒绝");
        assert!(
            error
                .to_string()
                .contains("invalid database pool parameters")
        );
    });
}

#[test]
fn min_connections_above_max_is_rejected() {
    with_env(&[("DATABASE_MIN_CONNECTIONS", Some("11"))], || {
        let error = AppConfig::from_env().expect_err("最小连接数大于最大连接数应被拒绝");
        assert!(
            error
                .to_string()
                .contains("invalid database pool parameters")
        );
    });
}

#[test]
fn zero_connect_timeout_is_rejected() {
    with_env(&[("DATABASE_CONNECT_TIMEOUT_SECS", Some("0"))], || {
        let error = AppConfig::from_env().expect_err("0 连接超时应被拒绝");
        assert!(error.to_string().contains("timeout must be greater than 0"));
    });
}

#[test]
fn zero_idle_timeout_is_rejected() {
    with_env(&[("DATABASE_IDLE_SECS", Some("0"))], || {
        let error = AppConfig::from_env().expect_err("0 空闲超时应被拒绝");
        assert!(error.to_string().contains("timeout must be greater than 0"));
    });
}

#[test]
fn zero_database_acquire_and_probe_timeout_are_rejected() {
    for key in [
        "DATABASE_ACQUIRE_TIMEOUT_SECS",
        "DATABASE_READY_PROBE_TIMEOUT_SECS",
    ] {
        with_env(&[(key, Some("0"))], || {
            let error = AppConfig::from_env().expect_err("0 超时应被拒绝");
            assert!(
                error.to_string().contains("must be greater than 0"),
                "{key} 的报错信息不符合预期: {error}"
            );
        });
    }
}

#[test]
fn zero_batch_params_are_rejected() {
    for key in ["LOG_BATCH_MAX_EVENTS", "LOG_BATCH_FLUSH_INTERVAL_SECS"] {
        with_env(&[(key, Some("0"))], || {
            let error = AppConfig::from_env().expect_err("批量参数为 0 应被拒绝");
            assert!(
                error
                    .to_string()
                    .contains("log batch parameters must be greater than 0"),
                "{key} 的报错信息不符合预期: {error}"
            );
        });
    }
}

#[test]
fn zero_max_log_files_is_rejected() {
    with_env(&[("LOG_MAX_LOG_FILES", Some("0"))], || {
        let error = AppConfig::from_env().expect_err("0 日志文件数应被拒绝");
        assert!(
            error
                .to_string()
                .contains("LOG_MAX_LOG_FILES must be greater than 0")
        );
    });
}

#[test]
fn zero_log_file_size_is_rejected() {
    with_env(&[("LOG_MAX_FILE_BYTES", Some("0"))], || {
        let error = AppConfig::from_env().expect_err("0 单文件大小上限应被拒绝");
        assert!(
            error
                .to_string()
                .contains("LOG_MAX_FILE_BYTES must be greater than 0")
        );
    });
}

#[test]
fn unsupported_log_rotation_is_rejected() {
    with_env(&[("LOG_ROTATION", Some("YEARLY"))], || {
        let error = AppConfig::from_env().expect_err("不支持的轮转策略应被拒绝");
        assert!(
            error
                .to_string()
                .contains("unsupported log rotation strategy")
        );
    });
}

#[test]
fn invalid_server_port_is_rejected() {
    with_env(&[("SERVER_PORT", Some("not-a-number"))], || {
        let error = AppConfig::from_env().expect_err("非法端口应被拒绝");
        assert!(error.to_string().contains("failed to parse"));
    });
}

#[test]
fn zero_server_connection_parameters_are_rejected() {
    for key in [
        "SERVER_MAX_CONNECTIONS",
        "SERVER_HTTP1_HEADER_READ_TIMEOUT_SECS",
        "SERVER_HTTP1_MAX_HEADERS",
        "SHUTDOWN_TIMEOUT_SECS",
    ] {
        with_env(&[(key, Some("0"))], || {
            let error = AppConfig::from_env().expect_err("0 连接层参数应被拒绝");
            assert!(
                error.to_string().contains("must be greater than 0"),
                "{key} 的报错信息不符合预期: {error}"
            );
        });
    }
}

/// hyper 的 `max_buf_size` 低于内部下限会断言失败，必须在配置阶段拒绝。
#[test]
fn http1_buffer_below_hyper_minimum_is_rejected() {
    with_env(&[("SERVER_HTTP1_MAX_BUF_SIZE", Some("1024"))], || {
        let error = AppConfig::from_env().expect_err("过小的读缓冲上限应被拒绝");
        assert!(
            error.to_string().contains(&MIN_HTTP1_BUF_SIZE.to_string()),
            "报错信息不符合预期: {error}"
        );
    });
}

#[test]
fn socket_addr_parses_valid_host() {
    let server = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 8080,
        ..common_server()
    };
    let address = server.socket_addr().expect("合法地址应可解析");
    assert_eq!(address.to_string(), "127.0.0.1:8080");
}

#[test]
fn socket_addr_rejects_invalid_host() {
    let server = ServerConfig {
        host: "invalid host".to_string(),
        port: 8080,
        ..common_server()
    };
    assert!(server.socket_addr().is_err());
}

fn common_server() -> ServerConfig {
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

#[test]
fn middleware_overrides_are_applied() {
    with_env(
        &[
            ("MIDDLEWARE_REQUEST_TIMEOUT_SECS", Some("3")),
            ("MIDDLEWARE_QUEUE_WAIT_TIMEOUT_SECS", Some("2")),
            ("MIDDLEWARE_MAX_BODY_BYTES", Some("1024")),
            ("MIDDLEWARE_MAX_URI_BYTES", Some("2048")),
            ("MIDDLEWARE_MAX_CONCURRENCY", Some("8")),
            ("MIDDLEWARE_BACKPRESSURE_QUEUE", Some("4")),
            ("MIDDLEWARE_RATE_LIMIT_REQUESTS", Some("50")),
            ("MIDDLEWARE_RATE_LIMIT_PERIOD_SECS", Some("2")),
            ("MIDDLEWARE_HSTS_ENABLED", Some("true")),
            ("MIDDLEWARE_HSTS_MAX_AGE_SECS", Some("600")),
            ("MIDDLEWARE_HSTS_INCLUDE_SUBDOMAINS", Some("true")),
            (
                "MIDDLEWARE_CORS_ALLOWED_ORIGINS",
                Some("https://a.example, https://b.example ,"),
            ),
            (
                "HTTP_ALLOWED_HOSTS",
                Some("api.example.com:8443, [::1]:8080"),
            ),
            ("MIDDLEWARE_TRUSTED_PROXIES", Some("10.0.0.0/8, 127.0.0.1")),
            ("MIDDLEWARE_ACCESS_LOG_ENABLED", Some("false")),
            ("MIDDLEWARE_COMPRESSION_ENABLED", Some("false")),
        ],
        || {
            let config = AppConfig::from_env().expect("中间件覆盖项应可加载");

            assert_eq!(config.middleware.request_timeout_secs, 3);
            assert_eq!(config.middleware.queue_wait_timeout_secs, 2);
            assert_eq!(config.middleware.max_body_bytes, 1024);
            assert_eq!(config.middleware.max_uri_bytes, 2048);
            assert_eq!(config.middleware.max_concurrency, 8);
            assert_eq!(config.middleware.backpressure_queue, 4);
            assert_eq!(config.middleware.rate_limit_requests, 50);
            assert_eq!(config.middleware.rate_limit_period_secs, 2);
            assert!(config.middleware.hsts.enabled);
            assert_eq!(config.middleware.hsts.max_age_secs, 600);
            assert!(config.middleware.hsts.include_subdomains);
            assert_eq!(
                config.middleware.hsts.header_value(),
                "max-age=600; includeSubDomains"
            );
            assert_eq!(
                config.middleware.cors_allowed_origins,
                vec![
                    "https://a.example".to_string(),
                    "https://b.example".to_string()
                ]
            );
            assert_eq!(
                config.middleware.allowed_hosts,
                vec!["api.example.com:8443".to_string(), "[::1]:8080".to_string()]
            );
            assert_eq!(config.middleware.trusted_proxies.len(), 2);
            assert!(!config.middleware.switches.access_log);
            assert!(!config.middleware.switches.compression);
        },
    );
}

#[test]
fn invalid_cors_origin_is_rejected() {
    with_env(
        &[(
            "MIDDLEWARE_CORS_ALLOWED_ORIGINS",
            Some("https://a.example\nX-Evil: 1"),
        )],
        || {
            let error = AppConfig::from_env().expect_err("非法来源应被拒绝");
            assert!(
                error
                    .to_string()
                    .contains("invalid MIDDLEWARE_CORS_ALLOWED_ORIGINS entry"),
                "报错信息不符合预期: {error}"
            );
        },
    );
}

#[test]
fn invalid_trusted_proxy_is_rejected() {
    with_env(
        &[("MIDDLEWARE_TRUSTED_PROXIES", Some("10.0.0.0/33"))],
        || {
            let error = AppConfig::from_env().expect_err("非法 CIDR 应被拒绝");
            assert!(
                error
                    .to_string()
                    .contains("invalid MIDDLEWARE_TRUSTED_PROXIES entry"),
                "报错信息不符合预期: {error}"
            );
        },
    );
}

#[test]
fn wildcard_host_is_rejected() {
    with_env(&[("HTTP_ALLOWED_HOSTS", Some("*"))], || {
        let error = AppConfig::from_env().expect_err("通配 Host 应被拒绝");
        assert!(
            error.to_string().contains("wildcard host is not allowed"),
            "报错信息不符合预期: {error}"
        );
    });
}

#[test]
fn hsts_preload_requires_include_subdomains_and_long_max_age() {
    with_env(
        &[
            ("MIDDLEWARE_HSTS_ENABLED", Some("true")),
            ("MIDDLEWARE_HSTS_PRELOAD", Some("true")),
        ],
        || {
            let error = AppConfig::from_env().expect_err("preload 组合不完整应被拒绝");
            assert!(
                error
                    .to_string()
                    .contains("MIDDLEWARE_HSTS_PRELOAD requires"),
                "报错信息不符合预期: {error}"
            );
        },
    );

    with_env(
        &[
            ("MIDDLEWARE_HSTS_ENABLED", Some("true")),
            ("MIDDLEWARE_HSTS_PRELOAD", Some("true")),
            ("MIDDLEWARE_HSTS_INCLUDE_SUBDOMAINS", Some("true")),
            ("MIDDLEWARE_HSTS_MAX_AGE_SECS", Some("31536000")),
        ],
        || {
            let config = AppConfig::from_env().expect("完整 preload 组合应可加载");
            assert_eq!(
                config.middleware.hsts.header_value(),
                "max-age=31536000; includeSubDomains; preload"
            );
        },
    );
}

#[test]
fn hsts_subdomain_flags_require_hsts_enabled() {
    with_env(
        &[
            ("MIDDLEWARE_HSTS_ENABLED", Some("false")),
            ("MIDDLEWARE_HSTS_INCLUDE_SUBDOMAINS", Some("true")),
        ],
        || {
            let error = AppConfig::from_env().expect_err("HSTS 关闭时子域标记应被拒绝");
            assert!(
                error
                    .to_string()
                    .contains("require MIDDLEWARE_HSTS_ENABLED=true"),
                "报错信息不符合预期: {error}"
            );
        },
    );
}

#[test]
fn queue_wait_must_not_exceed_request_timeout() {
    with_env(
        &[
            ("MIDDLEWARE_REQUEST_TIMEOUT_SECS", Some("2")),
            ("MIDDLEWARE_QUEUE_WAIT_TIMEOUT_SECS", Some("5")),
        ],
        || {
            let error = AppConfig::from_env().expect_err("队列等待超过总超时应被拒绝");
            assert!(
                error
                    .to_string()
                    .contains("must not exceed MIDDLEWARE_REQUEST_TIMEOUT_SECS"),
                "报错信息不符合预期: {error}"
            );
        },
    );
}

#[test]
fn zero_middleware_capacity_is_rejected() {
    for key in [
        "MIDDLEWARE_REQUEST_TIMEOUT_SECS",
        "MIDDLEWARE_QUEUE_WAIT_TIMEOUT_SECS",
        "MIDDLEWARE_MAX_BODY_BYTES",
        "MIDDLEWARE_MAX_URI_BYTES",
        "MIDDLEWARE_MAX_CONCURRENCY",
        "MIDDLEWARE_BACKPRESSURE_QUEUE",
        "MIDDLEWARE_RATE_LIMIT_REQUESTS",
        "MIDDLEWARE_RATE_LIMIT_PERIOD_SECS",
    ] {
        with_env(&[(key, Some("0"))], || {
            let error = AppConfig::from_env().expect_err("容量参数为 0 应被拒绝");
            assert!(
                error.to_string().contains("must be greater than 0"),
                "{key} 的报错信息不符合预期: {error}"
            );
        });
    }
}

/// 脚手架只支持 SQLite：迁移文件与补列逻辑都依赖 SQLite 语法，其他后端必须启动即拒。
#[test]
fn non_sqlite_database_url_is_rejected() {
    for url in [
        "postgres://localhost/app",
        "mysql://localhost/app",
        "app.db",
    ] {
        with_env(&[("DATABASE_URL", Some(url))], || {
            let error = AppConfig::from_env().expect_err("非 SQLite 连接串应被拒绝");
            assert!(
                error.to_string().contains("only supports sqlite:// URLs"),
                "{url} 的报错信息不符合预期: {error}"
            );
        });
    }
}
