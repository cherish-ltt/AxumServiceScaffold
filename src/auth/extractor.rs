use axum::{
    extract::FromRequestParts,
    http::{header::AUTHORIZATION, request::Parts},
};

use crate::{error::AppError, state::AppState};

/// 当前登录用户提取器。
///
/// 任何需要 JWT 鉴权的接口都可以直接声明该类型作为参数。
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub user_id: String,
    pub username: String,
    pub roles: Vec<String>,
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let authorization = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| AppError::unauthorized("缺少 Authorization 请求头"))?;

        let token = authorization
            .strip_prefix("Bearer ")
            .or_else(|| authorization.strip_prefix("bearer "))
            .ok_or_else(|| AppError::unauthorized("Authorization 格式应为 Bearer <token>"))?;

        let claims = state.jwt.verify_access_token(token)?;

        Ok(Self {
            user_id: claims.sub,
            username: claims.username,
            roles: claims.roles,
        })
    }
}
