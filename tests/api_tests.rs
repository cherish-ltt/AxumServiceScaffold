use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum_service_scaffold::container::Container;
use axum_service_scaffold::create_app::create_app;
use axum_service_scaffold::infrastructure::config::{
    AppConfig, DatabaseConfig, JwtConfig, LoggingConfig, ServerConfig,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use tracing_appender::rolling::Rotation;
use uuid::Uuid;

const TEST_JWT_SECRET: &str = "api-test-secret-that-is-long-enough";

/// 每个测试独立装配一套容器与临时 SQLite 数据库，互不干扰。
async fn setup_app() -> Router {
    let database_url = format!(
        "sqlite://{}?mode=rwc",
        std::env::temp_dir()
            .join(format!("axum-scaffold-api-test-{}.db", Uuid::now_v7()))
            .display()
    );

    let config = AppConfig {
        app_name: "axum-service-scaffold".to_string(),
        app_env: "development".to_string(),
        server: ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 0,
        },
        database: DatabaseConfig {
            url: database_url,
            min_connections: 1,
            max_connections: 2,
            connect_timeout_secs: 5,
            idle_secs: 30,
            sqlx_logging: false,
        },
        jwt: JwtConfig {
            secret: TEST_JWT_SECRET.to_string(),
            issuer: "test-issuer".to_string(),
            audience: "test-audience".to_string(),
            access_token_ttl_minutes: 120,
        },
        logging: LoggingConfig {
            filter: "info".to_string(),
            utc_offset_hour: 8,
            utc_offset_minute: 0,
            utc_offset_second: 0,
            filename_prefix: "api-test".to_string(),
            filename_suffix: "log".to_string(),
            rotation: Rotation::NEVER,
            max_log_files: 1,
            out_dir: std::env::temp_dir().to_string_lossy().to_string(),
        },
    };

    let container = Container::bootstrap(config).await.expect("容器装配成功");
    create_app(Arc::new(container))
}

async fn send(app: Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.oneshot(request).await.expect("请求应成功返回");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("响应体可读取")
        .to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

fn get(uri: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .expect("构造 GET 请求")
}

fn get_with_bearer(uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .expect("构造带令牌的 GET 请求")
}

fn post_json(uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("构造 POST 请求")
}

/// 通过调试登录接口获取访问令牌。
async fn issue_dev_token(app: &Router) -> String {
    let (status, body) = send(
        app.clone(),
        post_json(
            "/api/v1/auth/dev-login",
            json!({ "username": "demo-admin" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    body["data"]["access_token"]
        .as_str()
        .expect("应返回访问令牌")
        .to_string()
}

#[tokio::test]
async fn root_returns_service_info() {
    let app = setup_app().await;
    let (status, body) = send(app, get("/")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], 200);
    assert_eq!(body["data"]["service_name"], "axum-service-scaffold");
    assert_eq!(body["data"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["data"]["docs_enabled"], true);
}

#[tokio::test]
async fn health_reports_ok() {
    let app = setup_app().await;
    let (status, body) = send(app, get("/api/v1/system/health")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], 200);
    assert_eq!(body["message"], "健康检查完成");
    assert_eq!(body["data"]["status"], "ok");
    assert_eq!(body["data"]["database_status"], "not_checked");
}

#[tokio::test]
async fn ready_confirms_database() {
    let app = setup_app().await;
    let (status, body) = send(app, get("/api/v1/system/ready")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["message"], "服务已就绪");
    assert!(body.get("data").is_none(), "无数据时不应输出 data 字段");
}

#[tokio::test]
async fn dev_login_issues_bearer_token() {
    let app = setup_app().await;
    let (status, body) = send(
        app,
        post_json(
            "/api/v1/auth/dev-login",
            json!({ "username": "demo-admin" }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["message"], "调试令牌签发成功");
    assert_eq!(body["data"]["token_type"], "Bearer");
    assert_eq!(body["data"]["expires_in_seconds"], 120 * 60);
    assert!(
        !body["data"]["access_token"]
            .as_str()
            .unwrap_or("")
            .is_empty()
    );
}

#[tokio::test]
async fn dev_login_rejects_blank_username() {
    let app = setup_app().await;
    let (status, body) = send(
        app,
        post_json("/api/v1/auth/dev-login", json!({ "username": "   " })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], 400);
    assert!(body["message"].as_str().unwrap_or("").contains("用户名"));
}

#[tokio::test]
async fn dev_login_applies_default_role_and_generated_user_id() {
    let app = setup_app().await;
    let token = issue_dev_token(&app).await;
    let (status, body) = send(app, get_with_bearer("/api/v1/auth/me", &token)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["username"], "demo-admin");
    assert_eq!(body["data"]["roles"], json!(["developer"]));
    assert!(!body["data"]["user_id"].as_str().unwrap_or("").is_empty());
}

#[tokio::test]
async fn dev_login_keeps_explicit_user_id_and_roles() {
    let app = setup_app().await;
    let (status, body) = send(
        app.clone(),
        post_json(
            "/api/v1/auth/dev-login",
            json!({ "username": "ops", "user_id": "fixed-id", "roles": ["admin"] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let token = body["data"]["access_token"]
        .as_str()
        .expect("应返回访问令牌");

    let (status, body) = send(app, get_with_bearer("/api/v1/auth/me", token)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["user_id"], "fixed-id");
    assert_eq!(body["data"]["roles"], json!(["admin"]));
}

#[tokio::test]
async fn me_requires_authorization_header() {
    let app = setup_app().await;
    let (status, body) = send(app, get("/api/v1/auth/me")).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(
        body["message"]
            .as_str()
            .unwrap_or("")
            .contains("Authorization")
    );
}

#[tokio::test]
async fn me_rejects_non_bearer_scheme() {
    let app = setup_app().await;
    let request = Request::builder()
        .method("GET")
        .uri("/api/v1/auth/me")
        .header(header::AUTHORIZATION, "Basic dXNlcjpwYXNz")
        .body(Body::empty())
        .expect("构造请求");
    let (status, _) = send(app, request).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn me_rejects_invalid_token() {
    let app = setup_app().await;
    let (status, _) = send(app, get_with_bearer("/api/v1/auth/me", "not-a-jwt")).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn echo_creates_example() {
    let app = setup_app().await;
    let (status, body) = send(
        app,
        post_json(
            "/api/v1/examples/echo",
            json!({ "title": "搭建新服务", "note": "先接入日志和 JWT" }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["title"], "搭建新服务");
    assert_eq!(body["data"]["note"], "先接入日志和 JWT");
    assert_eq!(body["data"]["source"], "example-service");
    assert!(!body["data"]["id"].as_str().unwrap_or("").is_empty());
}

#[tokio::test]
async fn echo_accepts_missing_note() {
    let app = setup_app().await;
    let (status, body) = send(
        app,
        post_json("/api/v1/examples/echo", json!({ "title": "x" })),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["data"]["note"].is_null());
}

#[tokio::test]
async fn echo_rejects_blank_title() {
    let app = setup_app().await;
    let (status, body) = send(
        app,
        post_json("/api/v1/examples/echo", json!({ "title": "   " })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["message"].as_str().unwrap_or("").contains("标题"));
}

#[tokio::test]
async fn list_examples_applies_filters() {
    let app = setup_app().await;
    let (status, body) = send(app, get("/api/v1/examples?page=2&size=3&keyword=Swagger")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["page"], 2);
    assert_eq!(body["data"]["size"], 3);
    assert_eq!(body["data"]["keyword"], "Swagger");
    assert_eq!(body["data"]["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(body["data"]["items"][0]["id"], "example_003");
}

#[tokio::test]
async fn list_examples_uses_defaults() {
    let app = setup_app().await;
    let (status, body) = send(app, get("/api/v1/examples")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["page"], 1);
    assert_eq!(body["data"]["size"], 10);
    assert_eq!(body["data"]["items"].as_array().map(Vec::len), Some(3));
}

#[tokio::test]
async fn list_examples_rejects_zero_page() {
    let app = setup_app().await;
    let (status, _) = send(app, get("/api/v1/examples?page=0")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn list_examples_rejects_zero_size() {
    let app = setup_app().await;
    let (status, _) = send(app, get("/api/v1/examples?size=0")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn example_detail_requires_bearer_token() {
    let app = setup_app().await;
    let (status, _) = send(app, get("/api/v1/examples/example_001")).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn example_detail_returns_requester_info() {
    let app = setup_app().await;
    let token = issue_dev_token(&app).await;
    let (status, body) = send(app, get_with_bearer("/api/v1/examples/example_001", &token)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["id"], "example_001");
    assert_eq!(body["data"]["requested_by"], "demo-admin");
    assert_eq!(body["data"]["roles"], json!(["developer"]));
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn swagger_ui_is_mounted_in_debug() {
    let app = setup_app().await;
    let (status, _) = send(app, get("/swagger-ui")).await;

    // Swagger UI 挂载在 /swagger-ui，未带斜杠访问时会 303 重定向到 /swagger-ui/。
    assert!(
        status == StatusCode::OK || status == StatusCode::SEE_OTHER,
        "swagger-ui 应可访问，实际状态码: {status}"
    );
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn openapi_document_is_served() {
    let app = setup_app().await;
    let (status, body) = send(app, get("/api-doc/openapi.json")).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["openapi"].as_str().is_some());
    assert!(
        body["paths"]
            .as_object()
            .map(|paths| !paths.is_empty())
            .unwrap_or(false)
    );
}

#[tokio::test]
async fn unknown_route_returns_404() {
    let app = setup_app().await;
    let (status, _) = send(app, get("/api/v1/unknown")).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}
