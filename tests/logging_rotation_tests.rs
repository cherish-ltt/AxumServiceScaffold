//! 单文件大小轮转与文件数量保留，单独成进程：`logging::init` 每进程只能初始化一次。

mod common;

use std::path::Path;

use axum_service_scaffold::{infrastructure::config::AppConfig, logging};
use uuid::Uuid;

fn log_files(out_dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(out_dir)
        .expect("读取日志目录")
        .flatten()
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();

    names
}

/// 持续过载下单文件不能无限增长：超过 `LOG_MAX_FILE_BYTES` 就归档换新文件，
/// 并按 `LOG_MAX_LOG_FILES` 保留最近的文件。
#[test]
fn oversized_log_file_is_rotated_and_pruned() {
    let out_dir = std::env::temp_dir().join(format!("axum-scaffold-log-rotate-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&out_dir).expect("创建临时日志目录");

    let mut config: AppConfig = common::app_config(&out_dir.to_string_lossy());
    config.logging.filename_prefix = "rotate-test".to_string();
    config.logging.max_file_bytes = 512;
    config.logging.max_log_files = 1;
    config.logging.batch_max_events = 1;

    let guard = match logging::init(&config) {
        Ok(guard) => guard,
        Err(error) => panic!("日志初始化应成功: {error}"),
    };

    // 每条日志约 100 字节，写 30 条足以触发多次轮转
    for index in 0..30 {
        tracing::info!("rotation filler line {index} {}", "x".repeat(64));
    }
    drop(guard);

    let files = log_files(&out_dir);
    assert!(
        files.len() >= 2,
        "超过大小上限后应产生多个日志文件，实际: {files:?}"
    );
    assert!(
        files.len() <= 3,
        "保留文件数应受 LOG_MAX_LOG_FILES 约束，实际: {files:?}"
    );

    for (index, name) in files.iter().enumerate() {
        let path = out_dir.join(name);
        let size = std::fs::metadata(&path).expect("读取文件元数据").len();
        assert!(
            size <= 512 + 256,
            "第 {index} 个文件 {name} 大小 {size} 超出上限过多"
        );
    }
}
