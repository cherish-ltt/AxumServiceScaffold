//! 真实 TCP 连接层验收：超时、连接上限、优雅关闭。
//!
//! 这些用例**不能**用 `oneshot` 覆盖：`oneshot` 直接调用服务，绕过监听套接字、
//! accept 循环与 hyper 连接构建器，而请求头读取超时、空闲连接回收、连接数上限、
//! 排空与强制中止都只发生在连接层。

use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

mod common;

use axum::{Router, routing::get};
use axum_service_scaffold::{
    container::Container,
    create_app::create_app,
    infrastructure::config::{AppConfig, ServerConfig},
    server::Server,
};
use tokio::sync::oneshot;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

fn server_config() -> ServerConfig {
    ServerConfig {
        // 连接层参数取小值，用例才能在秒级内观察到期行为
        http1_header_read_timeout_secs: 1,
        max_connections: 2,
        shutdown_timeout_secs: 1,
        ..common::server_config()
    }
}

fn app_config() -> AppConfig {
    let mut config = common::app_config(&std::env::temp_dir().to_string_lossy());
    config.server = server_config();

    config
}

/// 启动服务并返回监听地址与关闭信号发送端。
async fn start(app: Router, config: AppConfig) -> (SocketAddr, oneshot::Sender<()>, JoinHandle) {
    let server = Server::bind(Arc::new(config), app)
        .await
        .expect("绑定监听端口");
    let address = server.local_addr().expect("读取监听地址");
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        server
            .run(async {
                let _ = shutdown_rx.await;
            })
            .await
    });

    (address, shutdown_tx, handle)
}

type JoinHandle = tokio::task::JoinHandle<anyhow::Result<()>>;

fn ping_app() -> Router {
    Router::new().route("/ping", get(|| async { "pong" }))
}

/// 持续读取，直到对端关闭（返回 `true`）或超时（返回 `false`）。
async fn wait_for_eof(stream: &mut TcpStream, budget: Duration) -> bool {
    let deadline = Instant::now() + budget;
    let mut buffer = [0u8; 1024];

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        match tokio::time::timeout(remaining, stream.read(&mut buffer)).await {
            Ok(Ok(0)) | Ok(Err(_)) => return true,
            Ok(Ok(_)) => continue,
            Err(_) => return false,
        }
    }
}

/// 读取响应文本，直到对端关闭或超时。
async fn read_response(stream: &mut TcpStream, budget: Duration) -> String {
    let deadline = Instant::now() + budget;
    let mut buffer = [0u8; 4096];
    let mut collected = Vec::new();

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, stream.read(&mut buffer)).await {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break,
            Ok(Ok(read)) => collected.extend_from_slice(&buffer[..read]),
        }
        // 已读到完整应答头（GET 无请求体），无需等待对端关闭
        if collected.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }

    String::from_utf8_lossy(&collected).to_string()
}

/// 慢速发头（slowloris）：只发不完整的请求行，必须被请求头读取超时断开。
#[tokio::test]
async fn slow_header_sender_is_disconnected_after_header_read_timeout() {
    let (address, shutdown, handle) = start(ping_app(), app_config()).await;
    let mut stream = TcpStream::connect(address).await.expect("建立连接");

    // 故意留一个不完整的请求头，且不再发送剩余字节
    stream
        .write_all(b"GET /ping HTTP/1.1\r\nHost: localhost\r\nX-Partial:")
        .await
        .expect("写入部分请求头");

    let started = Instant::now();
    assert!(
        wait_for_eof(&mut stream, Duration::from_secs(5)).await,
        "超过 HTTP1_HEADER_READ_TIMEOUT_SECS 后服务端必须关闭连接"
    );
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "断开耗时 {elapsed:?} 超出预期"
    );

    drop(shutdown);
    handle.await.expect("服务任务结束").expect("服务正常退出");
}

/// 请求头数量超限：hyper 在解析阶段直接回 431。
#[tokio::test]
async fn too_many_request_headers_are_rejected_with_431() {
    let mut config = app_config();
    config.server.http1_max_headers = 4;
    let (address, shutdown, handle) = start(ping_app(), config).await;
    let mut stream = TcpStream::connect(address).await.expect("建立连接");

    let mut request = String::from("GET /ping HTTP/1.1\r\nHost: localhost\r\n");
    for index in 0..12 {
        request.push_str(&format!("X-Filler-{index}: value\r\n"));
    }
    request.push_str("\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("写入超量请求头");

    let response = read_response(&mut stream, Duration::from_secs(5)).await;
    assert!(
        response.starts_with("HTTP/1.1 431"),
        "请求头数量超限应回 431，实际响应:\n{response}"
    );

    drop(shutdown);
    handle.await.expect("服务任务结束").expect("服务正常退出");
}

/// keep-alive 空闲连接：应答完成后不再有请求头，连接必须被回收。
///
/// hyper 1.x 没有独立的 idle timeout，空闲等待复用 `header_read_timeout`，
/// 因此这一条同时验证「空闲连接不会永久占用文件描述符」。
#[tokio::test]
async fn idle_keep_alive_connection_is_recycled() {
    let (address, shutdown, handle) = start(ping_app(), app_config()).await;
    let mut stream = TcpStream::connect(address).await.expect("建立连接");

    stream
        .write_all(b"GET /ping HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .expect("写入请求");
    let response = read_response(&mut stream, Duration::from_secs(5)).await;
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "正常请求应返回 200，实际响应:\n{response}"
    );

    assert!(
        wait_for_eof(&mut stream, Duration::from_secs(5)).await,
        "空闲连接必须在 HTTP1_HEADER_READ_TIMEOUT_SECS 后被关闭"
    );

    drop(shutdown);
    handle.await.expect("服务任务结束").expect("服务正常退出");
}

/// 连接数上限：超过 `SERVER_MAX_CONNECTIONS` 的新连接被立即关闭；
/// 断开旧连接后许可必须归还（否则服务会在流量波动后退化为永久拒服）。
#[tokio::test]
async fn connection_limit_rejects_extra_sockets_and_releases_permits() {
    let (address, shutdown, handle) = start(ping_app(), app_config()).await;

    let mut first = TcpStream::connect(address).await.expect("建立连接 1");
    let mut second = TcpStream::connect(address).await.expect("建立连接 2");
    // 占用许可的连接先各发一个请求，确保 accept 循环确实处理过它们
    for stream in [&mut first, &mut second] {
        stream
            .write_all(b"GET /ping HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .expect("写入请求");
        let response = read_response(stream, Duration::from_secs(5)).await;
        assert!(
            response.starts_with("HTTP/1.1 200"),
            "连接应可用:\n{response}"
        );
    }

    // 第 3 个连接超出上限：TCP 握手可能仍会完成（还在 backlog 里），但服务端会立刻关闭
    let mut rejected = TcpStream::connect(address).await.expect("建立连接 3");
    assert!(
        wait_for_eof(&mut rejected, Duration::from_secs(5)).await,
        "超出连接上限的连接必须被立即关闭"
    );

    // 释放一个许可
    drop(first);
    tokio::time::sleep(Duration::from_millis(300)).await;

    let mut reused = TcpStream::connect(address).await.expect("建立连接 4");
    reused
        .write_all(b"GET /ping HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .expect("写入请求");
    let response = read_response(&mut reused, Duration::from_secs(5)).await;
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "断开连接后许可必须回收，新连接应可用:\n{response}"
    );

    drop(shutdown);
    handle.await.expect("服务任务结束").expect("服务正常退出");
}

/// 优雅关闭：收到信号后在途请求必须被排空，客户端拿到完整应答。
#[tokio::test]
async fn graceful_shutdown_drains_in_flight_requests() {
    let app = Router::new().route(
        "/slow",
        get(|| async {
            tokio::time::sleep(Duration::from_millis(400)).await;
            "done"
        }),
    );
    let (address, shutdown, handle) = start(app, app_config()).await;
    let mut stream = TcpStream::connect(address).await.expect("建立连接");

    stream
        .write_all(b"GET /slow HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .expect("写入请求");
    tokio::time::sleep(Duration::from_millis(100)).await;
    shutdown.send(()).expect("发送关闭信号");

    let response = read_response(&mut stream, Duration::from_secs(5)).await;
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "关闭信号后已接收的请求仍应完成:\n{response}"
    );
    handle.await.expect("服务任务结束").expect("服务正常退出");
}

/// 关闭宽限期：卡住的连接不能无限拖住进程退出。
#[tokio::test]
async fn shutdown_deadline_aborts_stuck_connections() {
    let app = Router::new().route(
        "/stuck",
        get(|| async {
            tokio::time::sleep(Duration::from_secs(30)).await;
            "never"
        }),
    );
    let (address, shutdown, handle) = start(app, app_config()).await;
    let mut stream = TcpStream::connect(address).await.expect("建立连接");
    stream
        .write_all(b"GET /stuck HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .expect("写入请求");
    tokio::time::sleep(Duration::from_millis(100)).await;

    let started = Instant::now();
    shutdown.send(()).expect("发送关闭信号");
    tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("SHUTDOWN_TIMEOUT_SECS 到期后进程必须退出，而不是等业务处理完")
        .expect("服务任务结束")
        .expect("服务正常退出");

    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "关闭耗时 {elapsed:?} 应受 SHUTDOWN_TIMEOUT_SECS=1 约束"
    );
    assert!(
        wait_for_eof(&mut stream, Duration::from_secs(5)).await,
        "被强制中止的连接应关闭"
    );
}

/// 分块传输（无 `Content-Length`）的请求体同样受上限约束：
/// 不能只看请求头就放行，必须按实际累计字节拒绝。
#[tokio::test]
async fn chunked_request_body_is_rejected_with_413() {
    let mut config = app_config();
    config.database = common::database_config(common::temp_database_url("chunked-body"));
    config.middleware.max_body_bytes = 1024;
    let container = Arc::new(Container::bootstrap(config).await.expect("容器装配成功"));
    let server = Server::bind(
        Arc::clone(&container.config),
        create_app(Arc::clone(&container)),
    )
    .await
    .expect("绑定监听端口");
    let address = server.local_addr().expect("读取监听地址");
    let handle = tokio::spawn(async move { server.run(std::future::pending()).await });
    let (address, shutdown) = (address, handle);

    let mut stream = TcpStream::connect(address).await.expect("建立连接");
    stream
        .write_all(
            b"POST /api/v1/examples/echo HTTP/1.1\r\nHost: localhost\r\n\
              Content-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n",
        )
        .await
        .expect("写入请求头");

    // 逐块发送远超 MIDDLEWARE_MAX_BODY_BYTES 的内容，服务端应在累计超限时立即拒绝
    let chunk = format!("{:x}\r\n{}\r\n", 4096, "a".repeat(4096));
    let mut response = String::new();
    for _ in 0..4 {
        if stream.write_all(chunk.as_bytes()).await.is_err() {
            break;
        }
        let _ = stream.flush().await;
        response = read_response(&mut stream, Duration::from_secs(2)).await;
        if !response.is_empty() {
            break;
        }
    }

    assert!(
        response.starts_with("HTTP/1.1 413"),
        "分块请求体超限应回 413，实际响应:\n{response}"
    );

    shutdown.abort();
}

/// 处理函数 panic 的爆炸半径：只炸掉该连接，进程与其余连接不受影响。
///
/// 这里不引入 `catch_unwind`：panics 由 hyper 连接任务承载，任务结束时通过
/// `JoinSet` 上报 `error!` 日志，连接被关闭，其他连接继续服务。
#[tokio::test]
async fn handler_panic_kills_only_its_own_connection() {
    async fn panicking_handler() -> &'static str {
        panic!("handler exploded")
    }

    let app = Router::new()
        .route("/panic", get(panicking_handler))
        .route("/ping", get(|| async { "pong" }));
    let (address, shutdown, handle) = start(app, app_config()).await;

    let mut doomed = TcpStream::connect(address).await.expect("建立连接 1");
    doomed
        .write_all(b"GET /panic HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .expect("写入请求");
    let response = read_response(&mut doomed, Duration::from_secs(5)).await;
    assert!(
        !response.contains("200 OK"),
        "panic 的请求不应返回成功响应:\n{response}"
    );

    let mut healthy = TcpStream::connect(address).await.expect("建立连接 2");
    healthy
        .write_all(b"GET /ping HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .expect("写入请求");
    let response = read_response(&mut healthy, Duration::from_secs(5)).await;
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "panic 不应影响其他连接，实际响应:\n{response}"
    );

    drop(shutdown);
    handle.await.expect("服务任务结束").expect("服务正常退出");
}
