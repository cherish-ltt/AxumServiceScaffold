use axum::{Json, extract::State};

#[cfg(debug_assertions)]
use crate::docs::{DocAccessTokenResponse, DocCurrentUserResponse, DocErrorResponse};
use crate::{error::AppError, response::ApiResponse, state::AppState};

use super::{
    dto::{AccessTokenResponse, CurrentUserResponse, DevLoginRequest},
    extractor::CurrentUser,
    service,
};

/// 生成调试访问令牌。
///
/// 这是一个脚手架示例接口，用于在没有真实账号体系时快速拿到 JWT。
#[cfg_attr(debug_assertions, utoipa::path(
    post,
    path = "/api/v1/auth/dev-login",
    tag = "Auth",
    request_body = DevLoginRequest,
    responses(
        (status = 200, description = "调试令牌签发成功", body = DocAccessTokenResponse),
        (status = 400, description = "请求参数错误", body = DocErrorResponse)
    )
))]
pub async fn dev_login(
    State(state): State<AppState>,
    Json(payload): Json<DevLoginRequest>,
) -> Result<Json<ApiResponse<AccessTokenResponse>>, AppError> {
    let token = service::issue_dev_token(&state, payload)?;
    Ok(Json(ApiResponse::ok_with_message(
        "调试令牌签发成功",
        token,
    )))
}

/// 获取当前登录用户信息。
///
/// 该接口用于验证 JWT 链路是否正常，也可作为受保护接口的最小示例。
#[cfg_attr(debug_assertions, utoipa::path(
    get,
    path = "/api/v1/auth/me",
    tag = "Auth",
    security(
        ("bearer_auth" = [])
    ),
    responses(
        (status = 200, description = "当前用户信息获取成功", body = DocCurrentUserResponse),
        (status = 401, description = "未授权或访问令牌无效", body = DocErrorResponse)
    )
))]
pub async fn me(
    current_user: CurrentUser,
) -> Result<Json<ApiResponse<CurrentUserResponse>>, AppError> {
    let current_user = service::build_current_user_response(&current_user);
    Ok(Json(ApiResponse::ok(current_user)))
}
