use std::sync::Arc;

use axum::{Router, routing::get};

use crate::{api, container::Container, middleware};

/// 构建完整应用：业务路由 + 404/405 兜底 + 文档（仅开发环境）+ 中间件栈。
///
/// 公开签名保持不变（`Arc<Container>` → `Router`）：中间件栈仍在函数内部施加。
pub fn create_app(container: Arc<Container>) -> Router {
    let debug_routes = api::debug_routes_enabled(&container.config);
    let app = Router::new()
        .route("/", get(api::controllers::system_controller::root))
        // 采集端点是进程级基础设施端点（路径由 `metrics::METRICS_PATH` 唯一定义），
        // 与业务接口同端口暴露，靠令牌鉴权隔离。
        .merge(api::controllers::metrics_controller::router(
            container.metrics.is_some(),
        ))
        .nest("/api/v1", api::router(debug_routes))
        // 404 / 405 走统一响应结构，避免出现空响应体这种第二种错误格式。
        .fallback(api::not_found)
        .method_not_allowed_fallback(api::method_not_allowed)
        .with_state(Arc::clone(&container));

    // 文档路由只在「编译期启用 docs feature」且「非生产环境」时挂载：
    // 生产环境即使误用 `--features docs` 构建，也不会暴露 Swagger UI 与 OpenAPI。
    #[cfg(feature = "docs")]
    let app = if container.config.is_production() {
        app
    } else {
        crate::docs::mount(app)
    };

    // 路由模板捕获层必须位于路由匹配之内：`MatchedPath` 在匹配后才写入请求扩展，
    // 而指标层包在整个栈的最外层（才能统计到中间件自身的拒绝），两层靠 `RouteSlot` 传递。
    let app = middleware::route_slot::capture_route_templates(app);

    middleware::apply(app, &container.config)
}
