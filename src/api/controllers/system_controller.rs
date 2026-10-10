use std::sync::Arc;

use axum::{Router, extract::State, routing::get};

use crate::{
    api::dto::system::{HealthResponse, WelcomeResponse},
    container::Container,
    error::AppError,
    response::ApiResponse,
};

#[cfg(feature = "docs")]
use crate::docs::{DocErrorResponse, DocHealthResponse, DocMessageResponse, DocWelcomeResponse};

pub fn router() -> Router<Arc<Container>> {
    Router::new()
        .route("/system/health", get(health))
        .route("/system/ready", get(ready))
}

#[cfg_attr(feature = "docs", utoipa::path(
    get,
    path = "/",
    tag = "System",
    responses(
        (status = 200, description = "Welcome page returned", body = DocWelcomeResponse)
    )
))]
pub async fn root(
    State(container): State<Arc<Container>>,
) -> Result<ApiResponse<WelcomeResponse>, AppError> {
    let welcome = container.system_service.welcome().await?;
    Ok(ApiResponse::ok(welcome.into()))
}

#[cfg_attr(feature = "docs", utoipa::path(
    get,
    path = "/api/v1/system/health",
    tag = "System",
    responses(
        (status = 200, description = "Health check passed", body = DocHealthResponse),
        (status = 503, description = "Service degraded or database unavailable", body = DocHealthResponse)
    )
))]
pub async fn health(
    State(container): State<Arc<Container>>,
) -> Result<ApiResponse<HealthResponse>, AppError> {
    let health = container.system_service.health().await?;

    Ok(ApiResponse::ok_with_message(
        "Health check completed",
        health.into(),
    ))
}

#[cfg_attr(feature = "docs", utoipa::path(
    get,
    path = "/api/v1/system/ready",
    tag = "System",
    responses(
        (status = 200, description = "Service ready", body = DocMessageResponse),
        (status = 503, description = "Service not ready", body = DocErrorResponse)
    )
))]
pub async fn ready(State(container): State<Arc<Container>>) -> Result<ApiResponse<()>, AppError> {
    container.system_service.ready().await?;
    Ok(ApiResponse::message("Service ready"))
}
