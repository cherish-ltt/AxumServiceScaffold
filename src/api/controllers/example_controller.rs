use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};

use crate::{
    api::{
        dto::example::{
            ExampleDetailResponse, ExampleEchoRequest, ExampleEchoResponse, ExampleListResponse,
            ExampleQuery,
        },
        extractors::current_user::CurrentUser,
    },
    container::Container,
    error::AppError,
    response::ApiResponse,
};

#[cfg(debug_assertions)]
use crate::docs::{
    DocErrorResponse, DocExampleDetailResponse, DocExampleEchoResponse, DocExampleListResponse,
};

pub fn router() -> Router<Arc<Container>> {
    Router::new()
        .route("/examples/echo", post(create_echo))
        .route("/examples", get(list_examples))
        .route("/examples/{id}", get(get_example))
}

#[cfg_attr(debug_assertions, utoipa::path(
    post,
    path = "/api/v1/examples/echo",
    tag = "Example",
    request_body = ExampleEchoRequest,
    responses(
        (status = 200, description = "示例对象创建成功", body = DocExampleEchoResponse),
        (status = 400, description = "请求参数错误", body = DocErrorResponse)
    )
))]
pub async fn create_echo(
    State(container): State<Arc<Container>>,
    Json(payload): Json<ExampleEchoRequest>,
) -> Result<Json<ApiResponse<ExampleEchoResponse>>, AppError> {
    let echo = container
        .example_service
        .create_echo(payload.into())
        .await?;

    Ok(Json(ApiResponse::ok_with_message(
        "示例对象创建成功",
        echo.into(),
    )))
}

#[cfg_attr(debug_assertions, utoipa::path(
    get,
    path = "/api/v1/examples",
    tag = "Example",
    params(ExampleQuery),
    responses(
        (status = 200, description = "示例列表获取成功", body = DocExampleListResponse)
    )
))]
pub async fn list_examples(
    State(container): State<Arc<Container>>,
    Query(query): Query<ExampleQuery>,
) -> Result<Json<ApiResponse<ExampleListResponse>>, AppError> {
    let list = container
        .example_service
        .list_examples(query.into())
        .await?;
    Ok(Json(ApiResponse::ok(list.into())))
}

#[cfg_attr(debug_assertions, utoipa::path(
    get,
    path = "/api/v1/examples/{id}",
    tag = "Example",
    params(
        ("id" = String, Path, description = "示例 ID")
    ),
    security(
        ("bearer_auth" = [])
    ),
    responses(
        (status = 200, description = "示例详情获取成功", body = DocExampleDetailResponse),
        (status = 401, description = "未授权或令牌无效", body = DocErrorResponse),
        (status = 404, description = "资源不存在", body = DocErrorResponse)
    )
))]
pub async fn get_example(
    State(container): State<Arc<Container>>,
    Path(id): Path<String>,
    current_user: CurrentUser,
) -> Result<Json<ApiResponse<ExampleDetailResponse>>, AppError> {
    let detail = container
        .example_service
        .get_example_detail(id, current_user.into_inner())
        .await?;

    Ok(Json(ApiResponse::ok(detail.into())))
}
