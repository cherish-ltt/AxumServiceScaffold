pub mod controllers;
pub mod dto;
pub mod extractors;
mod fallbacks;

use std::sync::Arc;

use axum::Router;

use crate::container::Container;

pub use fallbacks::{method_not_allowed, not_found};

/// 业务路由。
///
/// `debug_routes` 同时受编译期 `#[cfg(debug_assertions)]` 与运行期环境校验约束：
/// 生产环境即使误用 debug 构建，调试路由也不会注册（见 `debug_routes_enabled`）。
///
/// 采集端点不在这里：`/metrics` 是进程级基础设施端点，不挂在 `/api/v1` 业务前缀下
///（见 `create_app`）。
pub fn router(debug_routes: bool) -> Router<Arc<Container>> {
    Router::new()
        .merge(controllers::auth_controller::router(debug_routes))
        .merge(controllers::system_controller::router())
        .merge(controllers::example_controller::router())
        .merge(controllers::transaction_controller::router(debug_routes))
}

/// 调试路由（`dev-login` / `dev-transfer`）是否注册。
///
/// 必须同时满足：本产物是 debug 构建，且当前不是生产环境。
/// 仅靠编译模式判断是不够的——用 `cargo run` 部署到生产就是完整鉴权绕过。
pub fn debug_routes_enabled(config: &crate::infrastructure::config::AppConfig) -> bool {
    cfg!(debug_assertions) && !config.is_production()
}
