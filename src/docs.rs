use axum::Router;
use serde::Serialize;
#[allow(unused_imports)]
use serde_json::json;
use utoipa::{
    Modify, OpenApi, ToSchema,
    openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
};
use utoipa_swagger_ui::SwaggerUi;

use crate::api::dto::{
    auth::{AccessTokenResponse, CurrentUserResponse, DevLoginRequest},
    example::{
        ExampleDetailResponse, ExampleEchoRequest, ExampleEchoResponse, ExampleListItem,
        ExampleListResponse, ExampleQuery,
    },
    system::{HealthResponse, WelcomeResponse},
};

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

#[derive(Serialize, ToSchema)]
pub struct DocMessageResponse {
    #[schema(example = 200)]
    pub code: u16,
    #[schema(example = "成功")]
    pub message: String,
    #[schema(value_type = Option<Object>, example = json!(null))]
    pub data: Option<serde_json::Value>,
    #[schema(example = 1713179523000i64)]
    pub timestamp: i64,
}

#[derive(Serialize, ToSchema)]
pub struct DocErrorResponse {
    #[schema(example = 400)]
    pub code: u16,
    #[schema(example = "请求参数错误")]
    pub message: String,
    #[schema(value_type = Option<Object>, example = json!(null))]
    pub data: Option<serde_json::Value>,
    #[schema(example = 1713179523000i64)]
    pub timestamp: i64,
}

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
        crate::api::controllers::system_controller::root,
        crate::api::controllers::system_controller::health,
        crate::api::controllers::system_controller::ready,
        crate::api::controllers::auth_controller::dev_login,
        crate::api::controllers::auth_controller::me,
        crate::api::controllers::example_controller::create_echo,
        crate::api::controllers::example_controller::list_examples,
        crate::api::controllers::example_controller::get_example
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

pub fn mount(router: Router) -> Router {
    router.merge(SwaggerUi::new("/swagger-ui").url("/api-doc/openapi.json", ApiDoc::openapi()))
}
