//! 产物端点暴露验收：调试登录、文档路由不得出现在生产配置/发布构建中。
//!
//! 这里只断言「运行时不可达」。真正守住产物的是两道门：编译期 `#[cfg]`
//! 与运行期 `APP_ENV`，两道都验。

use std::sync::Arc;

mod common;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use axum_service_scaffold::{
    container::Container, create_app::create_app, infrastructure::config::AppConfig,
};
use tower::ServiceExt;

async fn setup_app(configure: impl FnOnce(&mut AppConfig)) -> Router {
    let mut config = common::app_config(&std::env::temp_dir().to_string_lossy());
    config.database = common::database_config(common::temp_database_url("artifact-test"));
    config.logging.filename_prefix = "artifact-test".to_string();
    configure(&mut config);

    let container = Container::bootstrap(config).await.expect("容器装配成功");

    create_app(Arc::new(container))
}

async fn status_of(app: Router, uri: &str) -> StatusCode {
    app.oneshot(
        Request::builder()
            .uri(uri)
            .body(Body::empty())
            .expect("构造请求"),
    )
    .await
    .expect("请求应成功返回")
    .status()
}

const PROTECTED_ROUTES: &[&str] = &[
    "/api/v1/auth/dev-login",
    "/api/v1/auth/dev-transfer",
    "/swagger-ui",
    "/swagger-ui/",
    "/api-doc/openapi.json",
];

/// 生产配置下，即使二进制里编进了调试/文档路由，运行期也必须一个都不可达。
///
/// 豁免构建画像检查（`allow_insecure_build_profile`）正是为了验证这一点：
/// 豁免只放弃「拒绝启动」，不放行任何路由。
#[tokio::test]
async fn production_config_never_exposes_debug_or_docs_routes() {
    let app = setup_app(|config| {
        config.app_env = "production".to_string();
        config.allow_insecure_build_profile = true;
    })
    .await;

    for uri in PROTECTED_ROUTES {
        assert_eq!(
            status_of(app.clone(), uri).await,
            StatusCode::NOT_FOUND,
            "生产配置下 {uri} 必须不可达"
        );
    }
}

/// release 构建（无 `docs` feature）里调试路由根本不存在：不是被拒绝，而是没注册。
#[cfg(not(debug_assertions))]
#[tokio::test]
async fn release_build_without_debug_assertions_hides_dev_login() {
    let app = setup_app(|_| {}).await;

    assert_eq!(
        status_of(app, "/api/v1/auth/dev-login").await,
        StatusCode::NOT_FOUND
    );
}

/// release 且未启用 `docs` feature 时，文档路由不编译进产物。
#[cfg(all(not(debug_assertions), not(feature = "docs")))]
#[tokio::test]
async fn release_build_without_docs_feature_hides_swagger() {
    let app = setup_app(|_| {}).await;

    for uri in ["/swagger-ui", "/api-doc/openapi.json"] {
        assert_eq!(
            status_of(app.clone(), uri).await,
            StatusCode::NOT_FOUND,
            "未启用 docs feature 的 release 产物不应暴露 {uri}"
        );
    }
}

/// 调试构建下调试路由可用（对照组）：确保上面的「不可达」不是因为路由压根没接通。
#[cfg(debug_assertions)]
#[tokio::test]
async fn debug_build_and_development_env_expose_dev_login() {
    let app = setup_app(|_| {}).await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/dev-login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"username":"artifact-check"}"#))
                .expect("构造请求"),
        )
        .await
        .expect("请求应成功返回");

    assert_eq!(response.status(), StatusCode::OK);
}
