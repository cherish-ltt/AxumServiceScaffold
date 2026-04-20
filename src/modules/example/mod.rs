pub mod dto;
pub mod handlers;
pub mod service;

use axum::{Router, routing::{get, post}};

use crate::state::AppState;

/// 示例业务模块路由。
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/examples/echo", post(handlers::create_echo))
        .route("/examples", get(handlers::list_examples))
        .route("/examples/{id}", get(handlers::get_example))
}
