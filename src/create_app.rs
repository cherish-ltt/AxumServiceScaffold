use std::sync::Arc;

use axum::{Router, routing::get};

use crate::{api, container::Container};

pub fn create_app(container: Arc<Container>) -> Router {
    // 自定义嵌入入口也必须阻止 production 配置运行开发认证路由。
    if let Err(error) = container.config.validate_runtime() {
        panic!("{error}");
    }
    let app = Router::new()
        .route("/", get(api::controllers::system_controller::root))
        .nest("/api/v1", api::router())
        .with_state(Arc::clone(&container));

    #[cfg(feature = "docs")]
    let app = crate::docs::mount(app);

    crate::middleware::apply(app, &container.config.middleware)
}
