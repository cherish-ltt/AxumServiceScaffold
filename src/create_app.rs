use std::sync::Arc;

use axum::{Router, routing::get};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::{api, container::Container};

pub fn create_app(container: Arc<Container>) -> Router {
    let app = Router::new()
        .route("/", get(api::controllers::system_controller::root))
        .nest("/api/v1", api::router())
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(container);

    #[cfg(debug_assertions)]
    {
        crate::docs::mount(app)
    }

    #[cfg(not(debug_assertions))]
    {
        app
    }
}
