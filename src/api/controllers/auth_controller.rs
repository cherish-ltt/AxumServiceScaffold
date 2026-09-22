use std::sync::Arc;

use axum::{Json, Router, extract::State, routing::get};

#[cfg(debug_assertions)]
use axum::routing::post;

use crate::{
    api::{
        dto::auth::{AccessTokenResponse, CurrentUserResponse, DevLoginRequest},
        extractors::current_user::CurrentUser,
    },
    container::Container,
    error::AppError,
    response::ApiResponse,
};

#[cfg(debug_assertions)]
use crate::docs::{DocAccessTokenResponse, DocCurrentUserResponse, DocErrorResponse};

pub fn router() -> Router<Arc<Container>> {
    let router = Router::new().route("/auth/me", get(me));

    #[cfg(debug_assertions)]
    let router = router.route("/auth/dev-login", post(dev_login));

    router
}

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
    State(container): State<Arc<Container>>,
    Json(payload): Json<DevLoginRequest>,
) -> Result<ApiResponse<AccessTokenResponse>, AppError> {
    let token = container
        .auth_service
        .issue_dev_token(payload.into())
        .await?;

    Ok(ApiResponse::ok_with_message(
        "调试令牌签发成功",
        token.into(),
    ))
}

#[cfg_attr(debug_assertions, utoipa::path(
    get,
    path = "/api/v1/auth/me",
    tag = "Auth",
    security(
        ("bearer_auth" = [])
    ),
    responses(
        (status = 200, description = "当前用户信息获取成功", body = DocCurrentUserResponse),
        (status = 401, description = "未授权或令牌无效", body = DocErrorResponse)
    )
))]
pub async fn me(current_user: CurrentUser) -> Result<ApiResponse<CurrentUserResponse>, AppError> {
    Ok(ApiResponse::ok(current_user.into_inner().into()))
}
