use serde::{Deserialize, Serialize};

#[cfg(debug_assertions)]
use utoipa::ToSchema;

/// 调试登录请求。
///
/// 这是脚手架的演示接口请求体，便于在没有真实账号体系时先跑通 JWT 链路。
#[cfg_attr(debug_assertions, derive(ToSchema))]
#[derive(Debug, Deserialize)]
pub struct DevLoginRequest {
    /// 用户 ID，不传时自动生成。
    #[cfg_attr(
        debug_assertions,
        schema(example = "019680cc-7e1c-7ec0-b7b8-4b4f8e9dff10")
    )]
    pub user_id: Option<String>,
    /// 用户名。
    #[cfg_attr(debug_assertions, schema(example = "demo-admin"))]
    pub username: String,
    /// 用户角色列表，留空时默认使用 `developer`。
    #[cfg_attr(debug_assertions, schema(example = json!(["developer", "admin"])))]
    #[serde(default)]
    pub roles: Vec<String>,
}

/// JWT 签发结果。
#[cfg_attr(debug_assertions, derive(ToSchema))]
#[derive(Debug, Serialize)]
pub struct AccessTokenResponse {
    /// 访问令牌。
    #[cfg_attr(
        debug_assertions,
        schema(example = "eyJhbGciOiJIUzI1NiJ9.demo.token")
    )]
    pub access_token: String,
    /// 令牌类型。
    #[cfg_attr(debug_assertions, schema(example = "Bearer"))]
    pub token_type: String,
    /// 访问令牌剩余有效秒数。
    #[cfg_attr(debug_assertions, schema(example = 7200))]
    pub expires_in_seconds: i64,
}

/// 当前登录用户信息。
#[cfg_attr(debug_assertions, derive(ToSchema))]
#[derive(Debug, Serialize)]
pub struct CurrentUserResponse {
    /// 用户 ID。
    #[cfg_attr(
        debug_assertions,
        schema(example = "019680cc-7e1c-7ec0-b7b8-4b4f8e9dff10")
    )]
    pub user_id: String,
    /// 用户名。
    #[cfg_attr(debug_assertions, schema(example = "demo-admin"))]
    pub username: String,
    /// 角色列表。
    #[cfg_attr(debug_assertions, schema(example = json!(["developer", "admin"])))]
    pub roles: Vec<String>,
}
