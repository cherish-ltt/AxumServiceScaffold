use std::sync::Arc;

use axum::{Router, routing::get};
use tower_http::cors::CorsLayer;

use crate::{api, container::Container};

pub fn create_app(container: Arc<Container>) -> Router {
    let app = Router::new()
        .route("/", get(api::controllers::system_controller::root))
        .nest("/api/v1", api::router())
        .layer(CorsLayer::permissive())
        .with_state(Arc::clone(&container));

    #[cfg(debug_assertions)]
    let app = crate::docs::mount(app);

    crate::middleware::apply(app, &container.config.middleware)
}
