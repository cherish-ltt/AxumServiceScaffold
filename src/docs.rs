use serde::Serialize;
use utoipa::{
    Modify, OpenApi, ToSchema,
    openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
};
use utoipa_swagger_ui::SwaggerUi;

use crate::{
    auth::{
        dto::{AccessTokenResponse, CurrentUserResponse, DevLoginRequest},
    },
    modules::{
        example::{
            dto::{
                ExampleDetailResponse, ExampleEchoRequest, ExampleEchoResponse, ExampleListItem,
                ExampleListResponse, ExampleQuery,
            },
        },
        system::{
            dto::{HealthResponse, WelcomeResponse},
        },
    },
};
use axum::Router;

const SYSTEM_TAG: &str = "System";
const AUTH_TAG: &str = "Auth";
const EXAMPLE_TAG: &str = "Example";

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearer_auth",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .bearer_format("JWT")
                        .build(),
                ),
            );
        }
    }
}

/// 用于不带业务数据的 Swagger 成功响应。
#[derive(Serialize, ToSchema)]
pub struct DocMessageResponse {
    /// 业务状态码。
    #[schema(example = 200)]
    pub code: u16,
    /// 响应说明。
    #[schema(example = "成功")]
    pub message: String,
    /// 业务数据，当前为空。
    #[schema(value_type = Option<Object>, example = json!(null))]
    pub data: Option<serde_json::Value>,
    /// 时间戳，单位毫秒。
    #[schema(example = 1713179523000i64)]
    pub timestamp: i64,
}

/// Swagger 错误响应结构。
#[derive(Serialize, ToSchema)]
pub struct DocErrorResponse {
    /// 业务状态码。
    #[schema(example = 400)]
    pub code: u16,
    /// 错误说明。
    #[schema(example = "请求参数错误")]
    pub message: String,
    /// 业务数据，通常为空。
    #[schema(value_type = Option<Object>, example = json!(null))]
    pub data: Option<serde_json::Value>,
    /// 时间戳，单位毫秒。
    #[schema(example = 1713179523000i64)]
    pub timestamp: i64,
}

/// 首页响应包装。
#[derive(Serialize, ToSchema)]
pub struct DocWelcomeResponse {
    #[schema(example = 200)]
    pub code: u16,
    #[schema(example = "成功")]
    pub message: String,
    pub data: Option<WelcomeResponse>,
    #[schema(example = 1713179523000i64)]
    pub timestamp: i64,
}

/// 健康检查响应包装。
#[derive(Serialize, ToSchema)]
pub struct DocHealthResponse {
    #[schema(example = 200)]
    pub code: u16,
    #[schema(example = "健康检查完成")]
    pub message: String,
    pub data: Option<HealthResponse>,
    #[schema(example = 1713179523000i64)]
    pub timestamp: i64,
}

/// 调试登录响应包装。
#[derive(Serialize, ToSchema)]
pub struct DocAccessTokenResponse {
    #[schema(example = 200)]
    pub code: u16,
    #[schema(example = "调试令牌签发成功")]
    pub message: String,
    pub data: Option<AccessTokenResponse>,
    #[schema(example = 1713179523000i64)]
    pub timestamp: i64,
}

/// 当前用户响应包装。
#[derive(Serialize, ToSchema)]
pub struct DocCurrentUserResponse {
    #[schema(example = 200)]
    pub code: u16,
    #[schema(example = "成功")]
    pub message: String,
    pub data: Option<CurrentUserResponse>,
    #[schema(example = 1713179523000i64)]
    pub timestamp: i64,
}

/// 示例回显响应包装。
#[derive(Serialize, ToSchema)]
pub struct DocExampleEchoResponse {
    #[schema(example = 200)]
    pub code: u16,
    #[schema(example = "示例对象创建成功")]
    pub message: String,
    pub data: Option<ExampleEchoResponse>,
    #[schema(example = 1713179523000i64)]
    pub timestamp: i64,
}

/// 示例列表响应包装。
#[derive(Serialize, ToSchema)]
pub struct DocExampleListResponse {
    #[schema(example = 200)]
    pub code: u16,
    #[schema(example = "成功")]
    pub message: String,
    pub data: Option<ExampleListResponse>,
    #[schema(example = 1713179523000i64)]
    pub timestamp: i64,
}

/// 示例详情响应包装。
#[derive(Serialize, ToSchema)]
pub struct DocExampleDetailResponse {
    #[schema(example = 200)]
    pub code: u16,
    #[schema(example = "成功")]
    pub message: String,
    pub data: Option<ExampleDetailResponse>,
    #[schema(example = 1713179523000i64)]
    pub timestamp: i64,
}

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::modules::system::handlers::root,
        crate::modules::system::handlers::health,
        crate::modules::system::handlers::ready,
        crate::auth::handlers::dev_login,
        crate::auth::handlers::me,
        crate::modules::example::handlers::create_echo,
        crate::modules::example::handlers::list_examples,
        crate::modules::example::handlers::get_example
    ),
    components(
        schemas(
            WelcomeResponse,
            HealthResponse,
            DevLoginRequest,
            AccessTokenResponse,
            CurrentUserResponse,
            ExampleEchoRequest,
            ExampleEchoResponse,
            ExampleQuery,
            ExampleListItem,
            ExampleListResponse,
            ExampleDetailResponse,
            DocMessageResponse,
            DocErrorResponse,
            DocWelcomeResponse,
            DocHealthResponse,
            DocAccessTokenResponse,
            DocCurrentUserResponse,
            DocExampleEchoResponse,
            DocExampleListResponse,
            DocExampleDetailResponse
        )
    ),
    modifiers(&SecurityAddon),
    tags(
        (name = SYSTEM_TAG, description = "系统基础接口"),
        (name = AUTH_TAG, description = "JWT 鉴权接口"),
        (name = EXAMPLE_TAG, description = "示例业务接口")
    )
)]
struct ApiDoc;

/// 在调试环境挂载 Swagger UI。
pub fn mount(router: Router) -> Router {
    router.merge(
        SwaggerUi::new("/swagger-ui").url("/api-doc/openapi.json", ApiDoc::openapi()),
    )
}
