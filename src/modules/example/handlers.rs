use axum::{
    Json,
    extract::{Path, Query},
};

#[cfg(debug_assertions)]
use crate::docs::{
    DocErrorResponse, DocExampleDetailResponse, DocExampleEchoResponse, DocExampleListResponse,
};
use crate::{auth::extractor::CurrentUser, error::AppError, response::ApiResponse};

use super::{
    dto::{
        ExampleDetailResponse, ExampleEchoRequest, ExampleEchoResponse, ExampleListResponse,
        ExampleQuery,
    },
    service,
};

/// 创建一个示例回显对象。
///
/// 该接口用于演示最常见的 JSON 请求体与 JSON 返回体写法。
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
    Json(payload): Json<ExampleEchoRequest>,
) -> Result<Json<ApiResponse<ExampleEchoResponse>>, AppError> {
    if payload.title.trim().is_empty() {
        return Err(AppError::bad_request("标题不能为空"));
    }

    let echo = service::build_echo_response(payload);
    Ok(Json(ApiResponse::ok_with_message(
        "示例对象创建成功",
        echo,
    )))
}

/// 获取示例列表。
///
/// 该接口用于演示 Query 参数与分页返回结构。
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
    Query(query): Query<ExampleQuery>,
) -> Result<Json<ApiResponse<ExampleListResponse>>, AppError> {
    let list = service::build_example_list(query);
    Ok(Json(ApiResponse::ok(list)))
}

/// 获取单个示例详情。
///
/// 该接口用于演示路径参数与 JWT 保护接口的组合写法。
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
        (status = 401, description = "未授权或访问令牌无效", body = DocErrorResponse)
    )
))]
pub async fn get_example(
    Path(id): Path<String>,
    current_user: CurrentUser,
) -> Result<Json<ApiResponse<ExampleDetailResponse>>, AppError> {
    let detail = service::build_example_detail(id, &current_user);
    Ok(Json(ApiResponse::ok(detail)))
}
