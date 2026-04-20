use std::sync::Arc;

use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};

use crate::{
    api::dto::system::{HealthResponse, WelcomeResponse},
    container::Container,
    error::AppError,
    response::ApiResponse,
};

#[cfg(debug_assertions)]
use crate::docs::{DocErrorResponse, DocHealthResponse, DocMessageResponse, DocWelcomeResponse};

pub fn router() -> Router<Arc<Container>> {
    Router::new()
        .route("/system/health", get(health))
        .route("/system/ready", get(ready))
}

#[cfg_attr(debug_assertions, utoipa::path(
    get,
    path = "/",
    tag = "System",
    responses(
        (status = 200, description = "服务首页返回成功", body = DocWelcomeResponse)
    )
))]
pub async fn root(
    State(container): State<Arc<Container>>,
) -> Result<Json<ApiResponse<WelcomeResponse>>, AppError> {
    let welcome = container.system_service.welcome().await?;
    Ok(Json(ApiResponse::ok(welcome.into())))
}

#[cfg_attr(debug_assertions, utoipa::path(
    get,
    path = "/api/v1/system/health",
    tag = "System",
    responses(
        (status = 200, description = "健康检查通过", body = DocHealthResponse),
        (status = 503, description = "服务降级或数据库不可用", body = DocHealthResponse)
    )
))]
pub async fn health(
    State(container): State<Arc<Container>>,
) -> Result<impl IntoResponse, AppError> {
    let health = container.system_service.health().await?;
    let response: HealthResponse = health.into();
    let status = if response.database_status == "up" {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    Ok((
        status,
        Json(ApiResponse::with_parts(
            status.as_u16(),
            "健康检查完成",
            Some(response),
        )),
    ))
}

#[cfg_attr(debug_assertions, utoipa::path(
    get,
    path = "/api/v1/system/ready",
    tag = "System",
    responses(
        (status = 200, description = "服务已就绪", body = DocMessageResponse),
        (status = 503, description = "服务尚未就绪", body = DocErrorResponse)
    )
))]
pub async fn ready(
    State(container): State<Arc<Container>>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    container.system_service.ready().await?;
    Ok(Json(ApiResponse::message("服务已就绪")))
}
