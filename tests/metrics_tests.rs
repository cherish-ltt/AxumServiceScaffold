//! 指标采集端点与指标标签验收。
//!
//! 本文件独占一个测试进程：`metrics` 的全局 recorder 每进程只能安装一次
//! （`MetricsHandle::install` 用 `OnceLock` 复用），且各用例共享同一份计数器，
//! 因此断言只检查「标签与数值出现过」，不检查总量相等。

use std::sync::Arc;

mod common;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use axum_service_scaffold::{
    container::Container, create_app::create_app, infrastructure::config::AppConfig,
    metrics::METRICS_PATH,
};
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn setup_app(configure: impl FnOnce(&mut AppConfig)) -> Router {
    let mut config = common::app_config(&std::env::temp_dir().to_string_lossy());
    config.database = common::database_config(common::temp_database_url("metrics-test"));
    config.logging.filename_prefix = "metrics-test".to_string();
    config.middleware.rate_limit_requests = 1000;
    config.metrics = common::metrics_config(true);
    configure(&mut config);

    let container = Container::bootstrap(config).await.expect("容器装配成功");

    create_app(Arc::new(container))
}

fn request(uri: &str, token: Option<&str>) -> Request<Body> {
    let builder = Request::builder().uri(uri);
    let builder = match token {
        Some(token) => builder.header(header::AUTHORIZATION, format!("Bearer {token}")),
        None => builder,
    };

    builder.body(Body::empty()).expect("构造请求")
}

async fn body_text(app: Router, uri: &str, token: Option<&str>) -> (StatusCode, String) {
    let response = app
        .oneshot(request(uri, token))
        .await
        .expect("请求应成功返回");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("响应体可读取")
        .to_bytes();

    (status, String::from_utf8_lossy(&bytes).to_string())
}

async fn scrape(app: Router) -> String {
    let (status, body) = body_text(app, METRICS_PATH, Some(common::TEST_METRICS_TOKEN)).await;
    assert_eq!(status, StatusCode::OK, "带令牌采集应成功: {body}");

    body
}

/// 采集端点与业务接口同端口暴露，必须靠令牌隔离；缺失或错误令牌一律 404，
/// 不泄露端点是否存在。
#[tokio::test]
async fn metrics_endpoint_requires_matching_token() {
    let app = setup_app(|_| {}).await;

    for token in [None, Some("wrong-token")] {
        let (status, _) = body_text(app.clone(), METRICS_PATH, token).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "令牌非法时不应暴露采集端点（token={token:?}）"
        );
    }
}

#[tokio::test]
async fn metrics_endpoint_returns_prometheus_text_format() {
    let app = setup_app(|_| {}).await;
    let response = app
        .oneshot(request(METRICS_PATH, Some(common::TEST_METRICS_TOKEN)))
        .await
        .expect("请求应成功返回");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/plain; version=0.0.4")
    );
}

/// 指标必须按路由模板聚合，且不含任何请求级细节（请求 ID、查询串、路径参数）。
#[tokio::test]
async fn metrics_use_route_templates_and_never_raw_uris() {
    let app = setup_app(|_| {}).await;

    let (status, _) = body_text(app.clone(), "/api/v1/system/health", None).await;
    assert_eq!(status, StatusCode::OK);
    // 未匹配路由统一落到固定标签，避免扫描流量放大基数
    let (status, _) = body_text(app.clone(), "/no-such-route/12345?token=secret", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let rendered = scrape(app).await;
    assert!(
        rendered.contains("route=\"/api/v1/system/health\""),
        "指标应带路由模板标签:\n{rendered}"
    );
    assert!(
        rendered.contains("route=\"unmatched\""),
        "未匹配路由应使用固定标签:\n{rendered}"
    );
    for leaked in ["/no-such-route", "token=secret", "12345"] {
        assert!(
            !rendered.contains(leaked),
            "指标标签不得含原始 URI 片段 {leaked}:\n{rendered}"
        );
    }
}

/// 中间件自身的拒绝（限流 429）也必须被观测到，并按原因分类。
#[tokio::test]
async fn middleware_rejections_are_recorded_with_reason_labels() {
    let app = setup_app(|config| {
        config.middleware.rate_limit_requests = 1;
        config.middleware.rate_limit_period_secs = 60;
    })
    .await;

    let (first, _) = body_text(app.clone(), "/api/v1/system/health", None).await;
    assert_eq!(first, StatusCode::OK);
    let (second, body) = body_text(app, "/api/v1/system/health", None).await;
    assert_eq!(
        second,
        StatusCode::TOO_MANY_REQUESTS,
        "第二次请求应被限流: {body}"
    );

    // 限流器是全局的，也作用于采集端点自身；这里用一个不限流的实例渲染同一份全局指标，
    // 顺带记下这个运维约束（见 docs/DEPLOYMENT.md 的采集说明）。
    let rendered = scrape(setup_app(|_| {}).await).await;
    assert!(
        rendered.contains("http_rejections_total"),
        "拒绝必须计入指标:\n{rendered}"
    );
    assert!(
        rendered.contains("reason=\"rate_limited\""),
        "拒绝原因必须以固定标签暴露:\n{rendered}"
    );
    assert!(
        rendered.contains("status=\"429\""),
        "状态码分布应包含 429:\n{rendered}"
    );
    assert!(
        rendered.contains("http_requests_inflight"),
        "应暴露在途请求数:\n{rendered}"
    );
    assert!(
        rendered.contains("http_request_duration_seconds_bucket"),
        "应暴露延迟直方图分桶:\n{rendered}"
    );
}

/// `METRICS_ENABLED=false` 时端点完全不注册（而不是注册后返回 404）。
#[tokio::test]
async fn metrics_endpoint_is_absent_when_disabled() {
    let app = setup_app(|config| {
        config.metrics = common::metrics_config(false);
    })
    .await;

    let (status, _) = body_text(app, METRICS_PATH, Some(common::TEST_METRICS_TOKEN)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
