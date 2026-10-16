use std::time::{Duration, Instant};

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
    routing::{get, post},
};
use axum_service_scaffold::{infrastructure::config::MiddlewareConfig, middleware};
use tower::ServiceExt;
use uuid::Uuid;

fn base_config() -> MiddlewareConfig {
    MiddlewareConfig {
        request_timeout_secs: 10,
        max_body_bytes: 2 * 1024 * 1024,
        max_concurrency: 256,
        backpressure_queue: 256,
        rate_limit_requests: 1000,
        rate_limit_period_secs: 1,
        hsts_enabled: false,
        cors_allowed_origins: vec!["https://example.com".to_string()],
        trusted_proxies: Vec::new(),
        client_max_concurrency: 32,
        client_rate_limit_requests: 1024,
        client_rate_limit_period_secs: 1,
        client_max_entries: 16384,
        client_idle_secs: 60,
        slow_request_ms: 1000,
    }
}

fn ping_app(config: &MiddlewareConfig) -> Router {
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

async fn send(app: Router, mut request: Request<Body>) -> axum::response::Response {
    if request
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .is_none()
    {
        request.extensions_mut().insert(axum::extract::ConnectInfo(
            "127.0.0.1:12345".parse::<std::net::SocketAddr>().unwrap(),
        ));
    }
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
    config.request_timeout_secs = 1;

    let app = middleware::apply(Router::new().route("/slow", get(slow_handler)), &config);

    let started = Instant::now();
    let response = send(app, get_request("/slow")).await;

    assert_error(response, StatusCode::REQUEST_TIMEOUT).await;
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "超时应在 handler 完成前返回，实际耗时 {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn oversized_body_is_rejected_with_413() {
    let mut config = base_config();
    config.max_body_bytes = 1024;

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
    assert_error(response, StatusCode::PAYLOAD_TOO_LARGE).await;
    let chunked = Request::builder()
        .method("POST")
        .uri("/upload")
        .body(Body::from(vec![0_u8; 4096]))
        .unwrap();
    assert_error(
        send(app.clone(), chunked).await,
        StatusCode::PAYLOAD_TOO_LARGE,
    )
    .await;

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
    config.rate_limit_requests = 2;
    config.rate_limit_period_secs = 60;

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
    config.max_concurrency = 1;
    config.backpressure_queue = 1;

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
    config.hsts_enabled = true;
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

async fn assert_error(response: axum::response::Response, status: StatusCode) {
    assert_eq!(response.status(), status);
    assert!(response.headers().contains_key("x-request-id"));
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["code"], status.as_u16());
    assert!(body["timestamp"].is_number());
}

#[tokio::test]
async fn panic_is_isolated_and_framework_errors_are_json() {
    let app = middleware::apply(
        Router::new()
            .route(
                "/panic",
                get(|| async {
                    panic!("internal-secret");
                    #[allow(unreachable_code)]
                    StatusCode::OK
                }),
            )
            .route("/ping", get(|| async { "pong" })),
        &base_config(),
    );
    let response = send(app.clone(), get_request("/panic")).await;
    assert_error(response, StatusCode::INTERNAL_SERVER_ERROR).await;
    assert_eq!(
        send(app.clone(), get_request("/ping")).await.status(),
        StatusCode::OK
    );
    assert_error(
        send(app.clone(), get_request("/missing")).await,
        StatusCode::NOT_FOUND,
    )
    .await;
    let mut request = get_request("/ping");
    *request.method_mut() = axum::http::Method::POST;
    let response = send(app, request).await;
    assert!(response.headers().contains_key(header::ALLOW));
    assert_error(response, StatusCode::METHOD_NOT_ALLOWED).await;
}

#[tokio::test]
async fn total_timeout_covers_queue_and_releases_client_permit() {
    let mut config = base_config();
    config.request_timeout_secs = 1;
    config.client_max_concurrency = 1;
    let app = middleware::apply(
        Router::new()
            .route("/slow", get(slow_handler))
            .route("/ping", get(|| async { "pong" })),
        &config,
    );
    assert_error(
        send(app.clone(), get_request("/slow")).await,
        StatusCode::REQUEST_TIMEOUT,
    )
    .await;
    assert_eq!(
        send(app, get_request("/ping")).await.status(),
        StatusCode::OK
    );

    let mut config = base_config();
    config.max_concurrency = 1;
    config.request_timeout_secs = 1;
    let entered = std::sync::Arc::new(tokio::sync::Notify::new());
    let app = middleware::apply(
        Router::new().route(
            "/slow",
            get({
                let entered = entered.clone();
                move || {
                    let entered = entered.clone();
                    async move {
                        entered.notify_one();
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        "slow"
                    }
                }
            }),
        ),
        &config,
    );
    let first = tokio::spawn(send(app.clone(), get_request("/slow")));
    entered.notified().await;
    let started = Instant::now();
    assert_error(
        send(app, get_request("/slow")).await,
        StatusCode::REQUEST_TIMEOUT,
    )
    .await;
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "排队不能额外延长处理超时"
    );
    assert_error(first.await.unwrap(), StatusCode::REQUEST_TIMEOUT).await;
}

#[tokio::test]
async fn head_errors_never_send_a_body() {
    let mut request = get_request("/missing");
    *request.method_mut() = axum::http::Method::HEAD;
    let response = send(ping_app(&base_config()), request).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(
        axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn invalid_request_ids_are_replaced_not_logged_or_reflected() {
    for value in ["", "id with spaces", "id=spoof", &"x".repeat(65)] {
        let mut request = get_request("/ping");
        request
            .headers_mut()
            .insert("x-request-id", value.parse().unwrap());
        let response = send(ping_app(&base_config()), request).await;
        assert!(Uuid::parse_str(response.headers()["x-request-id"].to_str().unwrap()).is_ok());
    }
    let mut request = get_request("/ping");
    request
        .headers_mut()
        .append("x-request-id", "one".parse().unwrap());
    request
        .headers_mut()
        .append("x-request-id", "two".parse().unwrap());
    let response = send(ping_app(&base_config()), request).await;
    assert!(Uuid::parse_str(response.headers()["x-request-id"].to_str().unwrap()).is_ok());
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
