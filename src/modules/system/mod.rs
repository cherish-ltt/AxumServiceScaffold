pub mod dto;
pub mod handlers;
pub mod service;

use axum::{Router, routing::get};

use crate::state::AppState;

/// 系统模块路由。
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/system/health", get(handlers::health))
        .route("/system/ready", get(handlers::ready))
}
