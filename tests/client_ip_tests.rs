//! 可信客户端 IP 的端到端验收：真实 TCP 连接（只有真实连接才有 `ConnectInfo`）+ 真实日志。
//!
//! 单独成进程：`logging::init` 每进程只能安装一次全局订阅者。

use std::{net::SocketAddr, sync::Arc, time::Duration};

mod common;

use axum_service_scaffold::{
    container::Container, create_app::create_app, infrastructure::config::AppConfig, logging,
    server::Server,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use uuid::Uuid;

fn test_config(out_dir: &str, trusted_proxies: &[&str]) -> AppConfig {
    let mut config = common::app_config(out_dir);
    config.database = common::database_config(common::temp_database_url("client-ip-test"));
    config.logging.filename_prefix = "client-ip".to_string();
    config.middleware.trusted_proxies = trusted_proxies
        .iter()
        .map(|entry| entry.parse().expect("解析可信代理"))
        .collect();

    config
}

async fn start(config: AppConfig) -> (SocketAddr, tokio::task::JoinHandle<anyhow::Result<()>>) {
    let container = Arc::new(Container::bootstrap(config).await.expect("容器装配成功"));
    let server = Server::bind(
        Arc::clone(&container.config),
        create_app(Arc::clone(&container)),
    )
    .await
    .expect("绑定监听端口");
    let address = server.local_addr().expect("读取监听地址");
    let handle = tokio::spawn(async move { server.run(std::future::pending()).await });

    (address, handle)
}

async fn send_request(address: SocketAddr, extra_headers: &str) {
    let mut stream = TcpStream::connect(address).await.expect("建立连接");
    let request = format!(
        "GET /api/v1/system/health HTTP/1.1\r\nHost: localhost\r\n{extra_headers}Connection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("写入请求");

    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut response))
        .await
        .expect("读取响应不超时")
        .expect("读取响应");
    let text = String::from_utf8_lossy(&response);
    assert!(
        text.starts_with("HTTP/1.1 200"),
        "请求应成功，实际响应:\n{text}"
    );
}

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

/// 只有直连对端落在可信代理网段内才采信转发头；否则一律使用真实对端地址。
#[tokio::test]
async fn client_ip_follows_trusted_proxy_chain() {
    let out_dir = std::env::temp_dir().join(format!("axum-scaffold-client-ip-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&out_dir).expect("创建临时日志目录");
    let out_dir_label = out_dir.to_string_lossy().to_string();

    let guard = match logging::init(&test_config(&out_dir_label, &["127.0.0.0/8"])) {
        Ok(guard) => guard,
        Err(error) => panic!("日志初始化应成功: {error}"),
    };

    // 1) 对端 127.0.0.1 在可信网段内：采信 XFF，按信任链从右向左取第一个不可信地址
    let (trusted_address, trusted_handle) =
        start(test_config(&out_dir_label, &["127.0.0.0/8"])).await;
    send_request(
        trusted_address,
        "X-Forwarded-For: 203.0.113.7, 127.0.0.1\r\n",
    )
    .await;

    // 2) 未配置可信代理：同样的伪造头必须被忽略，日志里是对端地址
    let (untrusted_address, untrusted_handle) = start(test_config(&out_dir_label, &[])).await;
    send_request(untrusted_address, "X-Forwarded-For: 203.0.113.7\r\n").await;

    trusted_handle.abort();
    untrusted_handle.abort();
    drop(guard);

    let logs = read_logs(&out_dir);
    assert!(
        logs.contains("client_ip=\"203.0.113.7\"") && logs.contains("peer_ip=\"127.0.0.1\""),
        "可信代理链下应记录真实客户端 IP 与直连对端:\n{logs}"
    );
    // 未配置可信代理时，伪造头必须被忽略：此时 client_ip 与 peer_ip 都是 127.0.0.1，
    // 且这样的记录只能有 1 条（可信代理场景不得回退到代理地址）
    assert!(
        logs.matches("client_ip=\"127.0.0.1\" peer_ip=\"127.0.0.1\"")
            .count()
            == 1,
        "不可信对端的转发头必须被忽略:\n{logs}"
    );
}
