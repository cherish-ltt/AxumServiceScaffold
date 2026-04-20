use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
};

#[cfg(debug_assertions)]
use crate::docs::{DocErrorResponse, DocHealthResponse, DocWelcomeResponse, DocMessageResponse};
use crate::{error::AppError, response::ApiResponse, state::AppState};

use super::{dto::WelcomeResponse, service};

/// 服务首页。
///
/// 用于快速确认服务是否已启动，并返回最基础的元信息。
#[cfg_attr(debug_assertions, utoipa::path(
    get,
    path = "/",
    tag = "System",
    responses(
        (status = 200, description = "服务首页返回成功", body = DocWelcomeResponse)
    )
))]
pub async fn root(State(state): State<AppState>) -> Json<ApiResponse<WelcomeResponse>> {
    Json(ApiResponse::ok(service::build_welcome_response(&state)))
}

/// 健康检查接口。
///
/// 该接口会同时检查 Web 服务本身和数据库连接是否可用。
#[cfg_attr(debug_assertions, utoipa::path(
    get,
    path = "/api/v1/system/health",
    tag = "System",
    responses(
        (status = 200, description = "健康检查通过", body = DocHealthResponse),
        (status = 503, description = "服务降级或数据库不可用", body = DocHealthResponse)
    )
))]
pub async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let health = service::build_health_response(&state).await;
    let status = if health.database_status == "up" {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status,
        Json(ApiResponse::with_parts(
            status.as_u16(),
            "健康检查完成",
            Some(health),
        )),
    )
}

/// 就绪检查接口。
///
/// 用于给负载均衡或容器编排系统判断当前实例是否已具备接流量条件。
#[cfg_attr(debug_assertions, utoipa::path(
    get,
    path = "/api/v1/system/ready",
    tag = "System",
    responses(
        (status = 200, description = "服务已就绪", body = DocMessageResponse),
        (status = 503, description = "服务尚未就绪", body = DocErrorResponse)
    )
))]
pub async fn ready(State(state): State<AppState>) -> Result<Json<ApiResponse<()>>, AppError> {
    let health = service::build_health_response(&state).await;
    if health.database_status != "up" {
        return Err(AppError::unavailable("数据库尚未就绪"));
    }

    Ok(Json(ApiResponse::message("服务已就绪")))
}
