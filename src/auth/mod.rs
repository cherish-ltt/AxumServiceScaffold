pub mod dto;
pub mod extractor;
pub mod handlers;
pub mod jwt;
pub mod service;

use axum::{Router, routing::{get, post}};

use crate::state::AppState;

/// 鉴权模块路由。
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/dev-login", post(handlers::dev_login))
        .route("/auth/me", get(handlers::me))
}
