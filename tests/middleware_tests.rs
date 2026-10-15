use std::time::{Duration, Instant};

mod common;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
    routing::{get, post},
};
use axum_service_scaffold::{infrastructure::config::AppConfig, middleware};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

fn base_config() -> AppConfig {
    let mut config = common::app_config(&std::env::temp_dir().to_string_lossy());
    config.middleware.rate_limit_requests = 1000;
    config.middleware.cors_allowed_origins = vec!["https://example.com".to_string()];

    config
}

fn ping_app(config: &AppConfig) -> Router {
    middleware::apply(
        Router::new().route("/ping", get(|| async { "pong" })),
        config,
    )
}

fn get_request(uri: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .body(Body::empty())
        .expect("构造请求")
}

async fn send(app: Router, request: Request<Body>) -> axum::response::Response {
    app.oneshot(request).await.expect("请求应成功返回")
}

async fn slow_handler() -> &'static str {
    tokio::time::sleep(Duration::from_secs(5)).await;
    "slow"
}

async fn hold_handler() -> &'static str {
    tokio::time::sleep(Duration::from_millis(300)).await;
    "held"
}

#[tokio::test]
async fn request_over_timeout_is_rejected_with_408() {
    let mut config = base_config();
    config.middleware.request_timeout_secs = 1;

    let app = middleware::apply(Router::new().route("/slow", get(slow_handler)), &config);

    let started = Instant::now();
    let response = send(app, get_request("/slow")).await;

    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "超时应在 handler 完成前返回，实际耗时 {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn oversized_body_is_rejected_with_413() {
    let mut config = base_config();
    config.middleware.max_body_bytes = 1024;

    let app = middleware::apply(
        Router::new().route(
            "/upload",
            post(|body: axum::body::Bytes| async move { body.len().to_string() }),
        ),
        &config,
    );

    let oversized = Request::builder()
        .method("POST")
        .uri("/upload")
        .header(header::CONTENT_LENGTH, "4096")
        .body(Body::from(vec![0_u8; 4096]))
        .expect("构造请求");
    let response = send(app.clone(), oversized).await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let accepted = Request::builder()
        .method("POST")
        .uri("/upload")
        .header(header::CONTENT_LENGTH, "4")
        .body(Body::from(vec![0_u8; 4]))
        .expect("构造请求");
    assert_eq!(send(app, accepted).await.status(), StatusCode::OK);
}

#[tokio::test]
async fn requests_over_the_rate_limit_are_rejected_with_429() {
    let mut config = base_config();
    config.middleware.rate_limit_requests = 2;
    config.middleware.rate_limit_period_secs = 60;

    let app = ping_app(&config);

    for expected in [
        StatusCode::OK,
        StatusCode::OK,
        StatusCode::TOO_MANY_REQUESTS,
    ] {
        let response = send(app.clone(), get_request("/ping")).await;
        assert_eq!(response.status(), expected);
    }

    // 窗口未重置前持续快速拒绝
    assert_eq!(
        send(app, get_request("/ping")).await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn concurrency_overload_is_shed_with_503() {
    let mut config = base_config();
    config.middleware.max_concurrency = 1;
    config.middleware.backpressure_queue = 1;

    let app = middleware::apply(Router::new().route("/hold", get(hold_handler)), &config);

    let started = Instant::now();
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let app = app.clone();
        tasks.push(tokio::spawn(async move {
            send(app, get_request("/hold")).await.status()
        }));
    }

    let mut statuses = Vec::new();
    for task in tasks {
        statuses.push(task.await.expect("请求任务不应 panic"));
    }
    let elapsed = started.elapsed();

    assert!(
        statuses.contains(&StatusCode::OK),
        "容量内请求应正常处理: {statuses:?}"
    );
    assert!(
        statuses.contains(&StatusCode::SERVICE_UNAVAILABLE),
        "超出容量的请求应快速返回 503: {statuses:?}"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "超出排队长度的请求不得继续堆积，实际耗时 {elapsed:?}"
    );
}

#[tokio::test]
async fn request_id_is_generated_and_propagated() {
    let app = ping_app(&base_config());

    let response = send(app.clone(), get_request("/ping")).await;
    let generated = response
        .headers()
        .get("x-request-id")
        .expect("响应应带 x-request-id")
        .to_str()
        .expect("x-request-id 可读");
    assert!(
        Uuid::parse_str(generated).is_ok(),
        "未透传时应生成 UUID: {generated}"
    );

    let request = Request::builder()
        .uri("/ping")
        .header("x-request-id", "client-trace-id")
        .body(Body::empty())
        .expect("构造请求");
    let response = send(app, request).await;
    assert_eq!(
        response
            .headers()
            .get("x-request-id")
            .expect("响应应带 x-request-id"),
        "client-trace-id"
    );
}

#[tokio::test]
async fn security_headers_follow_config() {
    let response = send(ping_app(&base_config()), get_request("/ping")).await;
    let headers = response.headers();
    assert_eq!(
        headers.get("x-content-type-options").expect("缺少 nosniff"),
        "nosniff"
    );
    assert_eq!(headers.get("x-frame-options").expect("缺少 DENY"), "DENY");
    assert_eq!(
        headers
            .get("referrer-policy")
            .expect("缺少 referrer-policy"),
        "strict-origin-when-cross-origin"
    );
    assert!(
        headers.get("strict-transport-security").is_none(),
        "非 HTTPS 环境不应下发 HSTS"
    );

    let mut config = base_config();
    config.middleware.hsts.enabled = true;
    let response = send(ping_app(&config), get_request("/ping")).await;
    assert!(
        response
            .headers()
            .get("strict-transport-security")
            .is_some(),
        "开启后应下发 HSTS"
    );
}

#[tokio::test]
async fn compressible_response_is_gzipped_when_accepted() {
    // 默认压缩谓词只压缩大于 32 字节的响应体
    let app = middleware::apply(
        Router::new().route("/ping", get(|| async { "a".repeat(64) })),
        &base_config(),
    );
    let request = Request::builder()
        .uri("/ping")
        .header(header::ACCEPT_ENCODING, "gzip")
        .body(Body::empty())
        .expect("构造请求");

    let response = send(app, request).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_ENCODING)
            .expect("应压缩响应体"),
        "gzip"
    );
}

/// CORS 由中间件栈统一装配：白名单内的来源放行，未列入的直接无 CORS 头。
#[tokio::test]
async fn cors_layer_follows_allowed_origins_config() {
    let app = ping_app(&base_config());

    let allowed = send(app.clone(), cors_preflight("https://example.com")).await;
    assert_eq!(
        allowed
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .and_then(|value| value.to_str().ok()),
        Some("https://example.com")
    );

    let rejected = send(app, cors_preflight("https://evil.example")).await;
    assert!(
        rejected
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none(),
        "未列入白名单的来源不应收到 allow-origin"
    );
}

/// 拿到背压队列名额后仍在等待并发许可：超过等待上限必须 503，而不是无限等待，
/// 也不是被外层总超时兜成 408（两者对调用方的含义不同）。
#[tokio::test]
async fn queue_wait_timeout_is_rejected_with_503() {
    let mut config = base_config();
    config.middleware.max_concurrency = 1;
    config.middleware.backpressure_queue = 8;
    config.middleware.queue_wait_timeout_secs = 1;
    config.middleware.request_timeout_secs = 10;

    async fn holding_handler() -> &'static str {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        "held"
    }

    let app = middleware::apply(Router::new().route("/hold", get(holding_handler)), &config);
    let first = tokio::spawn({
        let app = app.clone();
        async move { send(app, get_request("/hold")).await.status() }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let started = Instant::now();
    let second = send(app, get_request("/hold")).await;
    let elapsed = started.elapsed();

    assert_eq!(second.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        elapsed >= Duration::from_millis(900) && elapsed < Duration::from_secs(3),
        "应在排队等待上限附近拒绝，实际耗时 {elapsed:?}"
    );
    let status = tokio::time::timeout(Duration::from_secs(3), first)
        .await
        .expect("占住并发名额的请求应正常完成")
        .expect("请求任务不应 panic");
    assert_eq!(status, StatusCode::OK);
}

/// 关闭安全关键中间件时必须真的不生效（生产环境默认拒绝这种配置，见 config_tests）。
#[tokio::test]
async fn disabled_switches_remove_their_layers() {
    let mut config = base_config();
    config.middleware.switches.security_headers = false;
    config.middleware.switches.timeout = false;
    config.middleware.switches.compression = false;
    config.middleware.switches.rate_limit = false;
    config.middleware.switches.request_id = false;
    config.middleware.rate_limit_requests = 1;

    let app = middleware::apply(
        Router::new().route("/ping", get(|| async { "a".repeat(64) })),
        &config,
    );

    let response = send(
        app.clone(),
        Request::builder()
            .uri("/ping")
            .header(header::ACCEPT_ENCODING, "gzip")
            .body(Body::empty())
            .expect("构造请求"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    for name in [
        header::X_CONTENT_TYPE_OPTIONS,
        header::CACHE_CONTROL,
        header::STRICT_TRANSPORT_SECURITY,
        header::CONTENT_ENCODING,
        header::CONTENT_TYPE,
        axum::http::HeaderName::from_static("x-request-id"),
    ] {
        if name == header::CONTENT_TYPE {
            continue;
        }
        assert!(
            response.headers().get(&name).is_none(),
            "关闭开关后不应出现 {name}"
        );
    }

    // 限流关闭后不再出现 429；超时关闭后慢请求不再被 408 打断
    for _ in 0..3 {
        assert_eq!(
            send(app.clone(), get_request("/ping")).await.status(),
            StatusCode::OK
        );
    }
    let slow_app = middleware::apply(Router::new().route("/slow", get(slow_handler)), &config);
    assert_eq!(
        send(slow_app, get_request("/slow")).await.status(),
        StatusCode::OK,
        "关闭超时后慢请求应正常完成"
    );
}

fn cors_preflight(origin: &str) -> Request<Body> {
    Request::builder()
        .method("OPTIONS")
        .uri("/ping")
        .header(header::ORIGIN, origin)
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .body(Body::empty())
        .expect("构造请求")
}

/// 框架层的拒绝（超时/请求体超限）也必须走统一响应结构。
///
/// `tower-http` 的 408 是空响应体、413 是纯文本 `length limit exceeded`，
/// 二者都不符合本服务的响应约定，因此栈里有 `RejectionBodyLayer` 统一改写。
#[tokio::test]
async fn framework_rejections_use_the_unified_envelope() {
    let mut config = base_config();
    config.middleware.request_timeout_secs = 1;
    config.middleware.max_body_bytes = 16;

    let app = middleware::apply(
        Router::new().route("/slow", get(slow_handler)).route(
            "/upload",
            post(|body: axum::body::Bytes| async move { body.len().to_string() }),
        ),
        &config,
    );

    let timeout = send(app.clone(), get_request("/slow")).await;
    assert_eq!(timeout.status(), StatusCode::REQUEST_TIMEOUT);
    let timeout_body: Value = serde_json::from_slice(
        &timeout
            .into_body()
            .collect()
            .await
            .expect("读取响应体")
            .to_bytes(),
    )
    .expect("408 响应体应为 JSON");
    assert_eq!(timeout_body["code"], 408);
    assert_eq!(timeout_body["message"], "Request timeout");

    let oversized = Request::builder()
        .method("POST")
        .uri("/upload")
        .header(header::CONTENT_LENGTH, "4096")
        .body(Body::from(vec![0_u8; 4096]))
        .expect("构造请求");
    let response = send(app, oversized).await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body: Value = serde_json::from_slice(
        &response
            .into_body()
            .collect()
            .await
            .expect("读取响应体")
            .to_bytes(),
    )
    .expect("413 响应体应为 JSON");
    assert_eq!(body["code"], 413);
    assert_eq!(body["message"], "Request body too large");
}
