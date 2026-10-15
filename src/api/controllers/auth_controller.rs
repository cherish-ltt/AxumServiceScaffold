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

#[cfg(feature = "docs")]
use crate::docs::{DocAccessTokenResponse, DocCurrentUserResponse, DocErrorResponse};

/// 认证路由。
///
/// `debug_routes` 仅在 debug 构建下有意义：release 产物中 dev-login 分支不参与编译。
pub fn router(debug_routes: bool) -> Router<Arc<Container>> {
    let router = Router::new().route("/auth/me", get(me));

    #[cfg(debug_assertions)]
    let router = if debug_routes {
        router.route("/auth/dev-login", post(dev_login))
    } else {
        router
    };

    #[cfg(not(debug_assertions))]
    let _ = debug_routes;

    router
}

#[cfg_attr(feature = "docs", utoipa::path(
    post,
    path = "/api/v1/auth/dev-login",
    tag = "Auth",
    request_body = DevLoginRequest,
    responses(
        (status = 200, description = "Debug token issued", body = DocAccessTokenResponse),
        (status = 400, description = "Bad request", body = DocErrorResponse)
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
        "Debug token issued",
        token.into(),
    ))
}

#[cfg_attr(feature = "docs", utoipa::path(
    get,
    path = "/api/v1/auth/me",
    tag = "Auth",
    security(
        ("bearer_auth" = [])
    ),
    responses(
        (status = 200, description = "Current user info fetched", body = DocCurrentUserResponse),
        (status = 401, description = "Unauthorized or invalid token", body = DocErrorResponse)
    )
))]
pub async fn me(current_user: CurrentUser) -> Result<ApiResponse<CurrentUserResponse>, AppError> {
    Ok(ApiResponse::ok(current_user.into_inner().into()))
}
