use uuid::Uuid;

use crate::{error::AppError, state::AppState};

use super::{
    dto::{AccessTokenResponse, CurrentUserResponse, DevLoginRequest},
    extractor::CurrentUser,
};

/// 生成调试令牌。
pub fn issue_dev_token(
    state: &AppState,
    payload: DevLoginRequest,
) -> Result<AccessTokenResponse, AppError> {
    let username = payload.username.trim();
    if username.is_empty() {
        return Err(AppError::bad_request("用户名不能为空"));
    }

    let user_id = payload.user_id.unwrap_or_else(|| Uuid::now_v7().to_string());
    let roles = if payload.roles.is_empty() {
        vec!["developer".to_string()]
    } else {
        payload.roles
    };

    state
        .jwt
        .issue_access_token(&user_id, username, &roles)
        .map_err(AppError::from)
}

/// 将提取器数据转换成对外响应结构。
pub fn build_current_user_response(current_user: &CurrentUser) -> CurrentUserResponse {
    CurrentUserResponse {
        user_id: current_user.user_id.clone(),
        username: current_user.username.clone(),
        roles: current_user.roles.clone(),
    }
}
