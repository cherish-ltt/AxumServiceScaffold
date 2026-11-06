use std::env;
use std::ffi::{OsStr, OsString};
use std::net::IpAddr;
use std::sync::{Mutex, MutexGuard, OnceLock};

use axum_service_scaffold::infrastructure::config::{AppConfig, ServerConfig};

/// 环境变量是进程级全局状态，所有依赖环境变量的测试必须串行执行，避免相互污染。
static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn set_var(key: &str, value: impl AsRef<OsStr>) {
    unsafe { env::set_var(key, value) };
}

fn remove_var(key: &str) {
    unsafe { env::remove_var(key) };
}

/// 断言结束（含 panic）时恢复环境变量的守卫：panic 展开也会执行 Drop。
struct EnvRestore {
    saved: Vec<(String, Option<OsString>)>,
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

/// 临时设置环境变量后执行断言，结束时（含断言 panic）恢复原始值。
fn with_env<F: FnOnce()>(vars: &[(&str, Option<&str>)], assertion: F) {
    let _lock = env_lock();
    let mut vars = vars.to_vec();
    if !vars.iter().any(|(key, _)| *key == "JWT_KEY_SET") {
        vars.push(("JWT_KEY_SET", None));
    }
    let saved: Vec<(String, Option<OsString>)> = vars
        .iter()
        .map(|(key, _)| (key.to_string(), env::var_os(key)))
        .collect();

    // 先挂上恢复守卫，再改环境变量：无论 assertion 是否 panic，都会恢复。
    let _restore = EnvRestore { saved };

    for (key, value) in &vars {
        match value {
            Some(value) => set_var(key, value),
            None => remove_var(key),
        }
    }

    assertion();
}

const EXAMPLE_SECRET: &str = "change-me-to-a-random-string-with-at-least-32-characters";
const CUSTOM_SECRET: &str = "unit-test-secret-that-is-definitely-long-enough";

#[test]
fn jwt_key_set_example_survives_dotenv_parsing() {
    let line = include_str!("../.env-example")
        .lines()
        .find_map(|line| line.trim().strip_prefix("# JWT_KEY_SET="))
        .expect("模板应提供轮换示例");
    with_env(
        &[
            ("APP_ENV", Some("development")),
            ("JWT_SECRET", None),
            ("JWT_KEY_SET", None),
        ],
        || {
            let path = env::temp_dir().join(format!("jwt-template-{}.env", uuid::Uuid::now_v7()));
            std::fs::write(&path, format!("JWT_KEY_SET={line}\n")).unwrap();
            let loaded = dotenv::from_path(&path);
            std::fs::remove_file(&path).unwrap();
            loaded.expect("示例应遵守 dotenv 语法");
            let config = AppConfig::from_env().expect("dotenv 读取后仍须为有效轮换配置");
            assert!(config.jwt.key_set.is_some());

            set_var("APP_ENV", "production");
            let error = AppConfig::from_env().expect_err("生产环境必须拒绝模板中的轮换示例密钥");
            assert_eq!(
                error.to_string(),
                "example JWT_KEY_SET secret is not allowed in production environment"
            );
            assert!(!format!("{error:#?}").contains(EXAMPLE_SECRET));
        },
    );
}

#[test]
fn jwt_key_set_replaces_the_single_secret() {
    let keys = serde_json::json!({
        "current_kid": "new",
        "keys": [{"kid": "new", "secret": CUSTOM_SECRET}]
    })
    .to_string();
    with_env(
        &[("JWT_SECRET", None), ("JWT_KEY_SET", Some(&keys))],
        || {
            AppConfig::from_env().expect("密钥集合应能独立提供签发与验证配置");
        },
    );
}

#[test]
fn jwt_key_set_conflicts_with_single_secret() {
    let keys = serde_json::json!({
        "current_kid": "new",
        "keys": [{"kid": "new", "secret": CUSTOM_SECRET}]
    })
    .to_string();
    with_env(
        &[
            ("JWT_SECRET", Some(CUSTOM_SECRET)),
            ("JWT_KEY_SET", Some(&keys)),
        ],
        || {
            let error = AppConfig::from_env().expect_err("禁止并存两个密钥真值来源");
            assert!(error.to_string().contains("JWT_SECRET"));
            assert!(!format!("{error:#}").contains(CUSTOM_SECRET));
        },
    );
}

#[test]
fn jwt_rotation_rejects_invalid_configurations_without_disclosing_secrets() {
    use serde_json::json;
    let current = json!({"kid": "B", "secret": CUSTOM_SECRET});
    let old = json!({"kid": "A", "secret": CUSTOM_SECRET, "accept_until": 1893456000});
    let base = json!({"current_kid": "B", "keys": [current.clone(), old.clone()]});
    let mut cases = vec![
        ("empty set", json!({"current_kid": "B", "keys": []})),
        (
            "too many keys",
            json!({"current_kid": "B", "keys": [current.clone(), old.clone(), old.clone(), old.clone(), old]}),
        ),
        (
            "duplicate kid",
            json!({"current_kid": "B", "keys": [current.clone(), current]}),
        ),
    ];
    for (label, field, value) in [
        ("missing current key", "current_kid", json!("missing")),
        ("wrong current type", "current_kid", json!(12)),
        ("missing legacy key", "legacy_kid", json!("missing")),
        ("empty legacy key", "legacy_kid", json!("")),
        ("legacy requires deadline", "legacy_kid", json!("B")),
        ("unknown root field", CUSTOM_SECRET, json!(true)),
    ] {
        let mut value_config = base.clone();
        value_config[field] = value;
        cases.push((label, value_config));
    }
    for (label, field, value) in [
        ("empty kid", "kid", json!("")),
        ("long kid", "kid", json!("a".repeat(65))),
        ("invalid kid", "kid", json!("../key")),
        (
            "short current secret",
            "secret",
            json!("short-current-secret"),
        ),
        ("zero cutoff", "accept_until", json!(0)),
        ("negative cutoff", "accept_until", json!(-1)),
        ("unrepresentable cutoff", "accept_until", json!(i64::MAX)),
        ("fractional cutoff", "accept_until", json!(1.5)),
        ("string cutoff", "accept_until", json!(CUSTOM_SECRET)),
        ("unknown key field", CUSTOM_SECRET, json!(true)),
    ] {
        let mut value_config = base.clone();
        value_config["keys"][0][field] = value;
        cases.push((label, value_config));
    }
    let mut missing_current = base.clone();
    missing_current
        .as_object_mut()
        .unwrap()
        .remove("current_kid");
    cases.push(("missing current field", missing_current));
    let mut missing_cutoff = base.clone();
    missing_cutoff["keys"][1]
        .as_object_mut()
        .unwrap()
        .remove("accept_until");
    cases.push(("missing extra key cutoff", missing_cutoff));
    let mut short_extra = base;
    short_extra["keys"][1]["secret"] = json!("short-extra-secret");
    cases.push(("short extra secret", short_extra));

    for (label, value) in cases {
        let raw = value.to_string();
        with_env(
            &[
                ("APP_ENV", Some("development")),
                ("JWT_SECRET", None),
                ("JWT_KEY_SET", Some(&raw)),
            ],
            || {
                let error = AppConfig::from_env().expect_err(label);
                let rendered = format!("{error:#?}");
                assert!(rendered.contains("JWT_KEY_SET"), "{label}: {rendered}");
                assert!(!rendered.contains(CUSTOM_SECRET), "{label}");
                assert!(
                    !rendered.contains("short-current-secret")
                        && !rendered.contains("short-extra-secret"),
                    "{label}"
                );
            },
        );
    }
}

#[test]
fn jwt_rotation_parse_errors_hide_raw_json() {
    for raw in [
        CUSTOM_SECRET.to_string(),
        format!(r#"{{"current_kid":"{CUSTOM_SECRET}","keys":{{}}}}"#),
    ] {
        with_env(&[("JWT_SECRET", None), ("JWT_KEY_SET", Some(&raw))], || {
            let error = AppConfig::from_env().expect_err("非法 JSON 必须拒绝");
            assert!(error.to_string().contains("JWT_KEY_SET"));
            assert!(!format!("{error:#?}").contains(CUSTOM_SECRET));
        });
    }
}

#[cfg(unix)]
#[test]
fn jwt_rotation_rejects_non_unicode_config_without_disclosing_secrets() {
    use std::os::unix::ffi::OsStringExt;

    for key in ["JWT_KEY_SET", "JWT_SECRET"] {
        with_env(
            &[("JWT_SECRET", Some(CUSTOM_SECRET)), ("JWT_KEY_SET", None)],
            || {
                let mut raw = CUSTOM_SECRET.as_bytes().to_vec();
                raw.push(0xff);
                unsafe { env::set_var(key, OsString::from_vec(raw)) };
                let error = AppConfig::from_env().expect_err("非法编码不能静默忽略");
                assert!(error.to_string().contains(key));
                assert!(error.to_string().contains("valid Unicode"));
                assert!(!format!("{error:#?}").contains(CUSTOM_SECRET));
            },
        );
    }
}

#[test]
fn jwt_rotation_applies_production_secret_rules_to_every_key() {
    for example_index in 0..2 {
        let mut raw = serde_json::json!({"current_kid": "B", "keys": [
            {"kid": "B", "secret": CUSTOM_SECRET},
            {"kid": "A", "secret": CUSTOM_SECRET, "accept_until": 1}
        ]});
        raw["keys"][example_index]["secret"] = serde_json::json!(EXAMPLE_SECRET);
        let raw = raw.to_string();
        with_env(
            &[
                ("APP_ENV", Some("production")),
                ("JWT_SECRET", None),
                ("JWT_KEY_SET", Some(&raw)),
            ],
            || {
                let error = AppConfig::from_env().expect_err("所有项均须遵守生产密钥约束");
                assert!(error.to_string().contains("not allowed in production"));
                assert!(!format!("{error:#?}").contains(EXAMPLE_SECRET));
            },
        );
    }
}

#[test]
fn jwt_rotation_accepts_four_keys_and_past_cutoffs() {
    let keys: Vec<_> = ["A", "B", "C", "D"]
        .into_iter()
        .map(|kid| serde_json::json!({"kid": kid, "secret": CUSTOM_SECRET, "accept_until": 1}))
        .collect();
    let raw = serde_json::json!({"current_kid": "B", "legacy_kid": "A", "keys": keys}).to_string();
    with_env(
        &[
            ("APP_ENV", Some("production")),
            ("JWT_SECRET", Some("")),
            ("JWT_KEY_SET", Some(&raw)),
        ],
        || {
            AppConfig::from_env().expect("已截止配置可加载，但令牌使用时必须拒绝");
        },
    );
}

#[test]
fn jwt_rotation_empty_value_keeps_single_secret_mode() {
    with_env(
        &[
            ("JWT_SECRET", Some(CUSTOM_SECRET)),
            ("JWT_KEY_SET", Some("")),
        ],
        || {
            AppConfig::from_env().expect("空集合配置保持原单密钥模式");
        },
    );
}

/// Rotation 的 Debug 输出为 PascalCase（如 Daily），统一小写后比较。
fn rotation_debug(rotation: &tracing_appender::rolling::Rotation) -> String {
    format!("{rotation:?}").to_lowercase()
}

#[test]
fn valid_config_loads_with_defaults() {
    with_env(
        &[
            ("APP_NAME", None),
            ("APP_ENV", None),
            ("SERVER_HOST", None),
            ("SERVER_PORT", None),
            ("DATABASE_URL", None),
            ("DATABASE_MIN_CONNECTIONS", None),
            ("DATABASE_MAX_CONNECTIONS", None),
            ("DATABASE_CONNECT_TIMEOUT_SECS", None),
            ("DATABASE_IDLE_SECS", None),
            ("DATABASE_SQLX_LOGGING", None),
            ("JWT_SECRET", Some(CUSTOM_SECRET)),
            ("JWT_ISSUER", None),
            ("JWT_AUDIENCE", None),
            ("JWT_ACCESS_TOKEN_TTL_MINUTES", None),
            ("LOG_FILTER", None),
            ("LOG_UTC_OFFSET_HOUR", None),
            ("LOG_UTC_OFFSET_MINUTE", None),
            ("LOG_UTC_OFFSET_SECOND", None),
            ("LOG_FILENAME_PREFIX", None),
            ("LOG_FILENAME_SUFFIX", None),
            ("LOG_ROTATION", None),
            ("LOG_MAX_LOG_FILES", None),
            ("LOG_OUT_DIR", None),
            ("LOG_BATCH_MAX_EVENTS", None),
            ("LOG_BATCH_FLUSH_INTERVAL_SECS", None),
            ("MIDDLEWARE_REQUEST_TIMEOUT_SECS", None),
            ("MIDDLEWARE_MAX_BODY_BYTES", None),
            ("MIDDLEWARE_MAX_CONCURRENCY", None),
            ("MIDDLEWARE_BACKPRESSURE_QUEUE", None),
            ("MIDDLEWARE_RATE_LIMIT_REQUESTS", None),
            ("MIDDLEWARE_RATE_LIMIT_PERIOD_SECS", None),
            ("MIDDLEWARE_HSTS_ENABLED", None),
            ("MIDDLEWARE_CORS_ALLOWED_ORIGINS", None),
            ("MIDDLEWARE_TRUSTED_PROXIES", None),
            ("MIDDLEWARE_CLIENT_MAX_CONCURRENCY", None),
            ("MIDDLEWARE_CLIENT_RATE_LIMIT_REQUESTS", None),
            ("MIDDLEWARE_CLIENT_RATE_LIMIT_PERIOD_SECS", None),
            ("MIDDLEWARE_CLIENT_MAX_ENTRIES", None),
            ("MIDDLEWARE_CLIENT_IDLE_SECS", None),
            ("MIDDLEWARE_SLOW_REQUEST_MS", None),
        ],
        || {
            let config = AppConfig::from_env().expect("默认配置应可加载");

            assert_eq!(config.app_name, "axum-service-scaffold");
            assert_eq!(config.app_env, "development");
            assert_eq!(config.server.host, "127.0.0.1");
            assert_eq!(config.server.port, 8080);
            assert_eq!(config.database.url, "sqlite://scaffold.db?mode=rwc");
            assert_eq!(config.database.min_connections, 1);
            assert_eq!(config.database.max_connections, 10);
            assert_eq!(config.database.connect_timeout_secs, 8);
            assert_eq!(config.database.idle_secs, 60);
            assert!(!config.database.sqlx_logging);
            assert_eq!(config.jwt.issuer, "axum-service-scaffold");
            assert_eq!(config.jwt.audience, "axum-service-clients");
            assert_eq!(config.jwt.access_token_ttl_minutes, 120);
            assert_eq!(config.logging.filter, "info,tower_http=info");
            assert_eq!(config.logging.filename_prefix, "app");
            assert_eq!(config.logging.filename_suffix, "log");
            assert_eq!(config.logging.max_log_files, 60);
            assert_eq!(config.logging.out_dir, "./log");
            assert_eq!(config.logging.batch_max_events, 50);
            assert_eq!(config.logging.batch_flush_interval_secs, 3);
            assert!(rotation_debug(&config.logging.rotation).contains("daily"));

            assert_eq!(config.middleware.request_timeout_secs, 10);
            assert_eq!(config.middleware.max_body_bytes, 2 * 1024 * 1024);
            assert_eq!(config.middleware.max_concurrency, 256);
            assert_eq!(config.middleware.backpressure_queue, 256);
            assert_eq!(config.middleware.rate_limit_requests, 32768);
            assert_eq!(config.middleware.rate_limit_period_secs, 1);
            assert!(!config.middleware.hsts_enabled);
            assert!(config.middleware.cors_allowed_origins.is_empty());
            assert!(config.middleware.trusted_proxies.is_empty());
            assert_eq!(config.middleware.client_max_concurrency, 32);
            assert_eq!(config.middleware.client_rate_limit_requests, 1024);
            assert_eq!(config.middleware.client_rate_limit_period_secs, 1);
            assert_eq!(config.middleware.client_max_entries, 16384);
            assert_eq!(config.middleware.client_idle_secs, 60);
            assert_eq!(config.middleware.slow_request_ms, 1000);
        },
    );
}

#[test]
fn custom_overrides_are_applied() {
    with_env(
        &[
            ("APP_NAME", Some("custom-app")),
            ("APP_ENV", Some("staging")),
            ("SERVER_PORT", Some("9000")),
            ("JWT_SECRET", Some(CUSTOM_SECRET)),
            ("JWT_ACCESS_TOKEN_TTL_MINUTES", Some("30")),
            ("DATABASE_SQLX_LOGGING", Some("true")),
            ("LOG_ROTATION", Some("HOURLY")),
            ("LOG_MAX_LOG_FILES", Some("7")),
            ("LOG_BATCH_MAX_EVENTS", Some("100")),
            ("LOG_BATCH_FLUSH_INTERVAL_SECS", Some("5")),
        ],
        || {
            let config = AppConfig::from_env().expect("自定义配置应可加载");

            assert_eq!(config.app_name, "custom-app");
            assert_eq!(config.app_env, "staging");
            assert_eq!(config.server.port, 9000);
            assert_eq!(config.jwt.access_token_ttl_minutes, 30);
            assert!(config.database.sqlx_logging);
            assert_eq!(config.logging.max_log_files, 7);
            assert_eq!(config.logging.batch_max_events, 100);
            assert_eq!(config.logging.batch_flush_interval_secs, 5);
            assert!(rotation_debug(&config.logging.rotation).contains("hourly"));
        },
    );
}

#[test]
fn supported_log_rotation_variants_parse() {
    with_env(
        &[
            ("JWT_SECRET", Some(CUSTOM_SECRET)),
            ("LOG_ROTATION", Some("DAILY")),
        ],
        || {
            for variant in ["daily", "hourly", "minutely", "weekly", "never"] {
                set_var("LOG_ROTATION", variant.to_uppercase());
                let config = AppConfig::from_env().expect("支持的轮转策略应可解析");
                assert!(rotation_debug(&config.logging.rotation).contains(variant));
            }

            for variant in ["Rotation::DAILY", "Rotation::WEEKLY"] {
                set_var("LOG_ROTATION", variant);
                let config = AppConfig::from_env().expect("兼容写法应可解析");
                let expected = variant.rsplit("::").next().unwrap_or("").to_lowercase();
                assert!(rotation_debug(&config.logging.rotation).contains(&expected));
            }
        },
    );
}

#[test]
fn production_accepts_custom_secret() {
    with_env(
        &[
            ("APP_ENV", Some("production")),
            ("JWT_SECRET", Some(CUSTOM_SECRET)),
        ],
        || {
            let result = AppConfig::from_env();
            assert!(result.is_ok());
        },
    );
}

#[test]
fn production_runtime_requires_release() {
    with_env(
        &[
            ("JWT_SECRET", Some(CUSTOM_SECRET)),
            ("APP_ENV", Some("production")),
        ],
        || {
            let config = AppConfig::from_env().unwrap();
            assert_eq!(config.validate_runtime().is_err(), cfg!(debug_assertions));
        },
    );
    with_env(
        &[
            ("JWT_SECRET", Some(CUSTOM_SECRET)),
            ("APP_ENV", Some("development")),
        ],
        || {
            assert!(AppConfig::from_env().unwrap().validate_runtime().is_ok());
        },
    );
}

#[test]
fn production_rejects_example_jwt_secret() {
    with_env(
        &[
            ("APP_ENV", Some("production")),
            ("JWT_SECRET", Some(EXAMPLE_SECRET)),
        ],
        || {
            let result = AppConfig::from_env();
            assert!(result.is_err());
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
    with_env(
        &[
            ("JWT_SECRET", Some(CUSTOM_SECRET)),
            ("JWT_ACCESS_TOKEN_TTL_MINUTES", Some("0")),
        ],
        || {
            let error = AppConfig::from_env().expect_err("非正数 TTL 应被拒绝");
            assert!(error.to_string().contains("must be greater than 0"));
        },
    );
}

#[test]
fn zero_min_connections_is_rejected() {
    with_env(
        &[
            ("JWT_SECRET", Some(CUSTOM_SECRET)),
            ("DATABASE_MIN_CONNECTIONS", Some("0")),
        ],
        || {
            let error = AppConfig::from_env().expect_err("0 最小连接数应被拒绝");
            assert!(
                error
                    .to_string()
                    .contains("invalid database pool parameters")
            );
        },
    );
}

#[test]
fn min_connections_above_max_is_rejected() {
    with_env(
        &[
            ("JWT_SECRET", Some(CUSTOM_SECRET)),
            ("DATABASE_MIN_CONNECTIONS", Some("11")),
        ],
        || {
            let error = AppConfig::from_env().expect_err("最小连接数大于最大连接数应被拒绝");
            assert!(
                error
                    .to_string()
                    .contains("invalid database pool parameters")
            );
        },
    );
}

#[test]
fn zero_connect_timeout_is_rejected() {
    with_env(
        &[
            ("JWT_SECRET", Some(CUSTOM_SECRET)),
            ("DATABASE_CONNECT_TIMEOUT_SECS", Some("0")),
        ],
        || {
            let error = AppConfig::from_env().expect_err("0 连接超时应被拒绝");
            assert!(error.to_string().contains("timeout must be greater than 0"));
        },
    );
}

#[test]
fn zero_idle_timeout_is_rejected() {
    with_env(
        &[
            ("JWT_SECRET", Some(CUSTOM_SECRET)),
            ("DATABASE_IDLE_SECS", Some("0")),
        ],
        || {
            let error = AppConfig::from_env().expect_err("0 空闲超时应被拒绝");
            assert!(error.to_string().contains("timeout must be greater than 0"));
        },
    );
}

#[test]
fn zero_batch_params_are_rejected() {
    for key in ["LOG_BATCH_MAX_EVENTS", "LOG_BATCH_FLUSH_INTERVAL_SECS"] {
        with_env(
            &[("JWT_SECRET", Some(CUSTOM_SECRET)), (key, Some("0"))],
            || {
                let error = AppConfig::from_env().expect_err("批量参数为 0 应被拒绝");
                assert!(
                    error
                        .to_string()
                        .contains("log batch parameters must be greater than 0"),
                    "{key} 的报错信息不符合预期: {error}"
                );
            },
        );
    }
}

#[test]
fn zero_max_log_files_is_rejected() {
    with_env(
        &[
            ("JWT_SECRET", Some(CUSTOM_SECRET)),
            ("LOG_MAX_LOG_FILES", Some("0")),
        ],
        || {
            let error = AppConfig::from_env().expect_err("0 日志文件数应被拒绝");
            assert!(
                error
                    .to_string()
                    .contains("LOG_MAX_LOG_FILES must be greater than 0")
            );
        },
    );
}

#[test]
fn unsupported_log_rotation_is_rejected() {
    with_env(
        &[
            ("JWT_SECRET", Some(CUSTOM_SECRET)),
            ("LOG_ROTATION", Some("YEARLY")),
        ],
        || {
            let error = AppConfig::from_env().expect_err("不支持的轮转策略应被拒绝");
            assert!(
                error
                    .to_string()
                    .contains("unsupported log rotation strategy")
            );
        },
    );
}

#[test]
fn invalid_server_port_is_rejected() {
    with_env(
        &[
            ("JWT_SECRET", Some(CUSTOM_SECRET)),
            ("SERVER_PORT", Some("not-a-number")),
        ],
        || {
            let error = AppConfig::from_env().expect_err("非法端口应被拒绝");
            assert!(error.to_string().contains("failed to parse"));
        },
    );
}

#[test]
fn socket_addr_parses_valid_host() {
    for (host, ip) in [
        ("127.0.0.1", "127.0.0.1"),
        ("0.0.0.0", "0.0.0.0"),
        ("::1", "::1"),
        ("::", "::"),
        ("[::1]", "::1"),
    ] {
        for port in [0, 8080] {
            let address = ServerConfig {
                host: host.to_string(),
                port,
            }
            .socket_addr()
            .unwrap();
            assert_eq!(address.ip(), ip.parse::<IpAddr>().unwrap());
            assert_eq!(address.port(), port);
        }
    }
}

#[test]
fn socket_addr_rejects_invalid_host() {
    for host in [
        "invalid host",
        "",
        "127.0.0.1:80",
        "[::1]:80",
        "localhost",
        "[127.0.0.1]",
    ] {
        let server = ServerConfig {
            host: host.to_string(),
            port: 8080,
        };
        assert!(server.socket_addr().is_err(), "{host}");
        with_env(
            &[
                ("JWT_SECRET", Some(CUSTOM_SECRET)),
                ("SERVER_HOST", Some(host)),
            ],
            || {
                assert!(
                    AppConfig::from_env()
                        .unwrap_err()
                        .to_string()
                        .contains("SERVER_HOST")
                );
            },
        );
    }
}

#[test]
fn middleware_overrides_are_applied() {
    with_env(
        &[
            ("JWT_SECRET", Some(CUSTOM_SECRET)),
            ("MIDDLEWARE_REQUEST_TIMEOUT_SECS", Some("3")),
            ("MIDDLEWARE_MAX_BODY_BYTES", Some("1024")),
            ("MIDDLEWARE_MAX_CONCURRENCY", Some("8")),
            ("MIDDLEWARE_BACKPRESSURE_QUEUE", Some("4")),
            ("MIDDLEWARE_RATE_LIMIT_REQUESTS", Some("50")),
            ("MIDDLEWARE_RATE_LIMIT_PERIOD_SECS", Some("2")),
            ("MIDDLEWARE_HSTS_ENABLED", Some("true")),
            (
                "MIDDLEWARE_TRUSTED_PROXIES",
                Some("10.0.0.0/8, ::1, 127.0.0.1"),
            ),
            ("MIDDLEWARE_CLIENT_MAX_CONCURRENCY", Some("4")),
            ("MIDDLEWARE_CLIENT_RATE_LIMIT_REQUESTS", Some("20")),
            ("MIDDLEWARE_CLIENT_RATE_LIMIT_PERIOD_SECS", Some("2")),
            ("MIDDLEWARE_CLIENT_MAX_ENTRIES", Some("100")),
            ("MIDDLEWARE_CLIENT_IDLE_SECS", Some("30")),
            ("MIDDLEWARE_SLOW_REQUEST_MS", Some("250")),
            (
                "MIDDLEWARE_CORS_ALLOWED_ORIGINS",
                Some("https://a.example, https://b.example ,"),
            ),
        ],
        || {
            let config = AppConfig::from_env().expect("中间件覆盖项应可加载");

            assert_eq!(config.middleware.request_timeout_secs, 3);
            assert_eq!(config.middleware.max_body_bytes, 1024);
            assert_eq!(config.middleware.max_concurrency, 8);
            assert_eq!(config.middleware.backpressure_queue, 4);
            assert_eq!(config.middleware.rate_limit_requests, 50);
            assert_eq!(config.middleware.rate_limit_period_secs, 2);
            assert_eq!(config.middleware.trusted_proxies.len(), 3);
            assert_eq!(config.middleware.client_max_concurrency, 4);
            assert_eq!(config.middleware.client_rate_limit_requests, 20);
            assert_eq!(config.middleware.client_rate_limit_period_secs, 2);
            assert_eq!(config.middleware.client_max_entries, 100);
            assert_eq!(config.middleware.client_idle_secs, 30);
            assert_eq!(config.middleware.slow_request_ms, 250);
            assert!(config.middleware.hsts_enabled);
            assert_eq!(
                config.middleware.cors_allowed_origins,
                vec![
                    "https://a.example".to_string(),
                    "https://b.example".to_string()
                ]
            );
        },
    );
}

#[test]
fn invalid_cors_origin_is_rejected() {
    with_env(
        &[
            ("JWT_SECRET", Some(CUSTOM_SECRET)),
            (
                "MIDDLEWARE_CORS_ALLOWED_ORIGINS",
                Some("https://a.example\nX-Evil: 1"),
            ),
        ],
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
fn middleware_hsts_defaults_to_production_only() {
    with_env(
        &[
            ("JWT_SECRET", Some(CUSTOM_SECRET)),
            ("APP_ENV", Some("production")),
            ("MIDDLEWARE_HSTS_ENABLED", None),
        ],
        || {
            let config = AppConfig::from_env().expect("生产配置应可加载");
            assert!(config.middleware.hsts_enabled);
        },
    );
}

#[test]
fn zero_middleware_capacity_is_rejected() {
    for key in [
        "MIDDLEWARE_REQUEST_TIMEOUT_SECS",
        "MIDDLEWARE_MAX_BODY_BYTES",
        "MIDDLEWARE_MAX_CONCURRENCY",
        "MIDDLEWARE_BACKPRESSURE_QUEUE",
        "MIDDLEWARE_RATE_LIMIT_REQUESTS",
        "MIDDLEWARE_RATE_LIMIT_PERIOD_SECS",
        "MIDDLEWARE_CLIENT_MAX_CONCURRENCY",
        "MIDDLEWARE_CLIENT_RATE_LIMIT_REQUESTS",
        "MIDDLEWARE_CLIENT_RATE_LIMIT_PERIOD_SECS",
        "MIDDLEWARE_CLIENT_MAX_ENTRIES",
        "MIDDLEWARE_CLIENT_IDLE_SECS",
        "MIDDLEWARE_SLOW_REQUEST_MS",
    ] {
        with_env(
            &[("JWT_SECRET", Some(CUSTOM_SECRET)), (key, Some("0"))],
            || {
                let error = AppConfig::from_env().expect_err("容量参数为 0 应被拒绝");
                assert!(
                    error
                        .to_string()
                        .contains("MIDDLEWARE capacity parameters must be greater than 0"),
                    "{key} 的报错信息不符合预期: {error}"
                );
            },
        );
    }
}

#[test]
fn wildcard_trusted_proxies_cover_every_peer() {
    with_env(
        &[
            ("JWT_SECRET", Some(CUSTOM_SECRET)),
            ("MIDDLEWARE_TRUSTED_PROXIES", Some("*")),
        ],
        || {
            let config = AppConfig::from_env().expect("`*` 应可加载");
            let nets = &config.middleware.trusted_proxies;
            assert_eq!(nets.len(), 2);
            for peer in ["203.0.113.7", "2001:db8::1"] {
                let peer: IpAddr = peer.parse().expect("测试用地址应合法");
                assert!(
                    nets.iter().any(|net| net.contains(&peer)),
                    "`*` 应覆盖 {peer}"
                );
            }
        },
    );
}

#[test]
fn production_rejects_trust_all_proxies() {
    for value in ["*", "0.0.0.0/0"] {
        with_env(
            &[
                ("APP_ENV", Some("production")),
                ("JWT_SECRET", Some(CUSTOM_SECRET)),
                ("MIDDLEWARE_TRUSTED_PROXIES", Some(value)),
            ],
            || {
                let error = AppConfig::from_env().expect_err("生产环境信任全部对端应被拒绝");
                assert!(
                    error
                        .to_string()
                        .contains("MIDDLEWARE_TRUSTED_PROXIES must not trust every peer"),
                    "{value} 的报错信息不符合预期: {error}"
                );
            },
        );
    }
    with_env(
        &[
            ("APP_ENV", Some("development")),
            ("JWT_SECRET", Some(CUSTOM_SECRET)),
            ("MIDDLEWARE_TRUSTED_PROXIES", Some("0.0.0.0/0")),
        ],
        || {
            assert!(AppConfig::from_env().is_ok());
        },
    );
}

#[test]
fn invalid_client_configuration_is_rejected() {
    for value in ["*,10.0.0.0/8", "10.0.0.1/99", "not-an-ip"] {
        with_env(
            &[
                ("JWT_SECRET", Some(CUSTOM_SECRET)),
                ("MIDDLEWARE_TRUSTED_PROXIES", Some(value)),
            ],
            || {
                assert!(
                    AppConfig::from_env()
                        .unwrap_err()
                        .to_string()
                        .contains("MIDDLEWARE_TRUSTED_PROXIES")
                );
            },
        );
    }
    with_env(
        &[
            ("JWT_SECRET", Some(CUSTOM_SECRET)),
            ("MIDDLEWARE_CLIENT_IDLE_SECS", Some("1")),
            ("MIDDLEWARE_CLIENT_RATE_LIMIT_PERIOD_SECS", Some("2")),
        ],
        || {
            assert!(
                AppConfig::from_env()
                    .unwrap_err()
                    .to_string()
                    .contains("MIDDLEWARE_CLIENT_IDLE_SECS")
            );
        },
    );
}

/// 脚手架只支持 SQLite：迁移文件与补列逻辑都依赖 SQLite 语法，其他后端必须启动即拒。
#[test]
fn non_sqlite_database_url_is_rejected() {
    for url in [
        "postgres://localhost/app",
        "mysql://localhost/app",
        "app.db",
    ] {
        with_env(
            &[
                ("JWT_SECRET", Some(CUSTOM_SECRET)),
                ("DATABASE_URL", Some(url)),
            ],
            || {
                let error = AppConfig::from_env().expect_err("非 SQLite 连接串应被拒绝");
                assert!(
                    error.to_string().contains("only supports sqlite:// URLs"),
                    "{url} 的报错信息不符合预期: {error}"
                );
            },
        );
    }
}
