//! 定时刷盘路径验证（独立进程）：`logging::init` 每进程只能安装一次全局订阅者，
//! 与 `tests/logging_batch_tests.rs` 分开，各自独占进程观察落盘时机。

use std::path::Path;
use std::thread;
use std::time::Duration;

mod common;

use axum_service_scaffold::infrastructure::config::AppConfig;
use axum_service_scaffold::logging;
use uuid::Uuid;

fn test_config(out_dir: &str) -> AppConfig {
    let mut config = common::app_config(out_dir);
    config.logging.filename_prefix = "logging-interval".to_string();
    config.logging.max_log_files = 2;
    // 条数阈值拉高，确保本测试只由定时刷盘触发落盘
    config.logging.batch_max_events = 100;
    config.logging.batch_flush_interval_secs = 1;

    config
}

/// 读取日志目录下所有文件的拼接内容；尚未创建文件时返回空串。
fn read_logs(out_dir: &Path) -> String {
    let mut contents = String::new();
    if let Ok(entries) = std::fs::read_dir(out_dir) {
        for entry in entries.flatten() {
            if entry.path().is_file() {
                contents.push_str(&std::fs::read_to_string(entry.path()).unwrap_or_default());
            }
        }
    }

    contents
}

#[test]
fn logs_are_flushed_on_fixed_interval() {
    let out_dir =
        std::env::temp_dir().join(format!("axum-scaffold-log-interval-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&out_dir).expect("创建临时日志目录");

    let config = test_config(&out_dir.to_string_lossy());
    let guard = match logging::init(&config) {
        Ok(guard) => guard,
        Err(e) => panic!("日志初始化应成功: {e}"),
    };

    // 顺带验证启动配置日志输出了批量参数（log_startup_config 会写入文件）
    logging::log_startup_config(&config);

    for i in 0..3 {
        tracing::info!("interval flush entry {i}");
    }
    // 未满 100 条、间隔 1s：轮询等待定时线程落盘，避免固定 sleep 造成的时序抖动。
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut logs;
    loop {
        logs = read_logs(&out_dir);
        if logs.contains("interval flush entry 2") || std::time::Instant::now() > deadline {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert!(
        logs.contains("interval flush entry 0")
            && logs.contains("interval flush entry 1")
            && logs.contains("interval flush entry 2"),
        "定时刷盘线程应在间隔到期后落盘缓冲日志:\n{logs}"
    );
    assert!(
        logs.contains("batch_max_events=100") && logs.contains("batch_flush_interval_secs=1"),
        "启动配置日志应输出批量参数:\n{logs}"
    );
    // 版本号取自 Cargo.toml，断言用编译期同一来源比对，不硬编码字面量。
    assert!(
        logs.contains(&format!("version={}", env!("CARGO_PKG_VERSION"))),
        "启动配置日志应输出版本号:\n{logs}"
    );
    drop(guard);
}
