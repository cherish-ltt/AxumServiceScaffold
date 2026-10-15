//! JSON 日志格式与敏感信息脱敏，单独成进程：`logging::init` 会安装全局订阅者，
//! 每进程只能初始化一次。

use std::sync::Arc;

mod common;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use axum_service_scaffold::{
    container::Container, create_app::create_app, infrastructure::config::AppConfig, logging,
};
use tower::ServiceExt;
use uuid::Uuid;

/// 读取日志目录下所有文件的拼接内容；尚未创建文件时返回空串。
fn read_logs(out_dir: &std::path::Path) -> String {
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

#[tokio::test]
async fn json_logs_are_parseable_and_never_leak_credentials() {
    let out_dir = std::env::temp_dir().join(format!("axum-scaffold-log-json-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&out_dir).expect("创建临时日志目录");

    let mut config: AppConfig = common::app_config(&out_dir.to_string_lossy());
    config.logging.format = axum_service_scaffold::infrastructure::config::LogFormat::Json;
    config.logging.filename_prefix = "json-test".to_string();
    config.database = common::database_config(common::temp_database_url("logging-json-test"));

    let guard = match logging::init(&config) {
        Ok(guard) => guard,
        Err(error) => panic!("JSON 日志初始化应成功: {error}"),
    };

    let container = Container::bootstrap(config).await.expect("容器装配成功");
    let app: Router = create_app(Arc::new(container));

    let secret_token = "super-secret-bearer-token-value";
    let session_cookie = "session=super-secret-cookie-value";
    let request = Request::builder()
        .uri("/api/v1/system/health")
        .header("x-request-id", "json-log-request-id")
        .header(header::AUTHORIZATION, format!("Bearer {secret_token}"))
        .header(header::COOKIE, session_cookie)
        .body(Body::empty())
        .expect("构造请求");
    let response = app.oneshot(request).await.expect("请求应成功返回");
    assert_eq!(response.status(), StatusCode::OK);

    // 落盘剩余缓冲
    drop(guard);

    let logs = read_logs(&out_dir);
    let mut access_line = None;
    for line in logs.lines().filter(|line| !line.trim().is_empty()) {
        let parsed: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|error| panic!("每行日志都必须是合法 JSON（{error}）: {line}"));
        if parsed["message"] == "request completed" {
            access_line = Some(parsed);
        }
    }

    let access = access_line.unwrap_or_else(|| panic!("缺少访问日志行:\n{logs}"));
    assert_eq!(access["status"], "200");
    assert_eq!(access["route"], "/api/v1/system/health");
    assert_eq!(access["method"], "GET");
    assert!(access["latency_ms"].is_number());
    assert_eq!(access["span"]["request_id"], "json-log-request-id");

    // 访问日志不记录请求头，凭据不得出现在任何一行里
    for secret in [secret_token, "super-secret-cookie-value"] {
        assert!(
            !logs.contains(secret),
            "日志中不得出现凭据 {secret}:\n{logs}"
        );
    }
}
