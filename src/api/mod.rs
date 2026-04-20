pub mod controllers;
pub mod dto;
pub mod extractors;

use std::sync::Arc;

use axum::Router;

use crate::container::Container;

pub fn router() -> Router<Arc<Container>> {
    Router::new()
        .merge(controllers::auth_controller::router())
        .merge(controllers::system_controller::router())
        .merge(controllers::example_controller::router())
}
