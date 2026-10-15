//! 分批写入验证单独成进程：`logging::init` 会安装全局日志订阅者，同一进程只能
//! 初始化一次；该测试要观察「未满批次不落盘」的中间状态，必须独占一个进程。

use std::path::Path;

mod common;

use axum_service_scaffold::infrastructure::config::AppConfig;
use axum_service_scaffold::logging;
use uuid::Uuid;

fn test_config(out_dir: &str) -> AppConfig {
    let mut config = common::app_config(out_dir);
    config.logging.filename_prefix = "logging-batch".to_string();
    config.logging.max_log_files = 2;
    config.logging.batch_max_events = 5;
    // 定时刷盘间隔拉到 1 小时，确保本测试只由「条数」触发落盘
    config.logging.batch_flush_interval_secs = 3600;

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
fn file_logs_are_flushed_in_batches() {
    let out_dir = std::env::temp_dir().join(format!("axum-scaffold-log-batch-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&out_dir).expect("创建临时日志目录");

    let config = test_config(&out_dir.to_string_lossy());
    let guard = match logging::init(&config) {
        Ok(guard) => guard,
        Err(e) => panic!("日志初始化应成功: {e}"),
    };

    // 未满批次阈值：事件只进内存缓冲，不应落盘
    for i in 0..4 {
        tracing::info!("batch log entry {i}");
    }
    assert_eq!(
        read_logs(&out_dir),
        "",
        "未满 LOG_BATCH_MAX_EVENTS=5 条时不应写入文件"
    );

    // 满 5 条：同步触发一次落盘
    tracing::info!("batch log entry 5");
    assert_eq!(
        read_logs(&out_dir).lines().count(),
        5,
        "满批次后应落盘 5 条日志"
    );

    // 继续写入，剩余 3 条在 guard drop 时落盘
    for i in 6..9 {
        tracing::info!("batch log entry {i}");
    }
    drop(guard);

    let logs = read_logs(&out_dir);
    assert_eq!(
        logs.lines().count(),
        8,
        "drop 后全部 8 条日志应已落盘:\n{logs}"
    );
}
