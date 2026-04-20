use axum::{Router, routing::get};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::{auth, modules, state::AppState};

/// 构建应用主路由。
///
/// 所有模块路由都应该在这里汇总，避免入口散落到多个文件中。
pub fn build_app(state: AppState) -> Router {
    let api_router = Router::new()
        .merge(auth::router())
        .merge(modules::system::router())
        .merge(modules::example::router());

    let app = Router::new()
        .route("/", get(modules::system::handlers::root))
        .nest("/api/v1", api_router)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    #[cfg(debug_assertions)]
    {
        crate::docs::mount(app)
    }

    #[cfg(not(debug_assertions))]
    {
        app
    }
}
