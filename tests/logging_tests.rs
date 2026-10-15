mod common;

use axum_service_scaffold::infrastructure::config::AppConfig;
use axum_service_scaffold::logging;
use uuid::Uuid;

fn test_config(out_dir: &str, utc_offset_hour: i8, utc_offset_minute: i8) -> AppConfig {
    let mut config = common::app_config(out_dir);
    config.logging.utc_offset_hour = utc_offset_hour;
    config.logging.utc_offset_minute = utc_offset_minute;
    config.logging.filename_prefix = "logging-test".to_string();
    config.logging.max_log_files = 2;

    config
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

    tracing::info!("logging init test message");
    drop(guard);

    // 不只断言文件存在，还要断言写入的内容确实落盘（BatchGuard drop 时落盘剩余缓冲）。
    let mut contents = String::new();
    for entry in std::fs::read_dir(&out_dir).expect("读取日志目录").flatten() {
        if entry.path().is_file() {
            contents.push_str(&std::fs::read_to_string(entry.path()).expect("读取日志文件"));
        }
    }
    assert!(
        contents.contains("logging init test message"),
        "日志文件应包含写入的日志行，实际内容:\n{contents}"
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
    assert!(
        error
            .to_string()
            .contains("invalid log timezone offset configuration")
    );
}

/// LOG_FILTER 是唯一真值来源：写错时必须拒绝启动，而不是静默换一套过滤规则。
#[test]
fn logging_init_rejects_invalid_filter() {
    let mut config = test_config("/tmp/axum-scaffold-invalid-filter", 8, 0);
    config.logging.filter = "info,missing=notalevel".to_string();

    match logging::init(&config) {
        Ok(_) => panic!("非法 LOG_FILTER 应被拒绝"),
        Err(error) => assert!(
            error.to_string().contains("invalid LOG_FILTER"),
            "报错信息不符合预期: {error}"
        ),
    }
}
