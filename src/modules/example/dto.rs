use serde::{Deserialize, Serialize};

#[cfg(debug_assertions)]
use utoipa::{IntoParams, ToSchema};

/// 示例请求体。
#[cfg_attr(debug_assertions, derive(ToSchema))]
#[derive(Debug, Deserialize)]
pub struct ExampleEchoRequest {
    /// 主标题。
    #[cfg_attr(debug_assertions, schema(example = "搭建新服务"))]
    pub title: String,
    /// 可选备注。
    #[cfg_attr(debug_assertions, schema(example = "先接入日志和 JWT"))]
    pub note: Option<String>,
}

/// 示例创建返回体。
#[cfg_attr(debug_assertions, derive(ToSchema))]
#[derive(Debug, Serialize)]
pub struct ExampleEchoResponse {
    /// 生成的示例 ID。
    #[cfg_attr(
        debug_assertions,
        schema(example = "019680cc-7e1c-7ec0-b7b8-4b4f8e9dff10")
    )]
    pub id: String,
    /// 标题。
    #[cfg_attr(debug_assertions, schema(example = "搭建新服务"))]
    pub title: String,
    /// 备注。
    #[cfg_attr(debug_assertions, schema(example = "先接入日志和 JWT"))]
    pub note: Option<String>,
    /// 数据来源说明。
    #[cfg_attr(debug_assertions, schema(example = "example-module"))]
    pub source: String,
}

/// 示例列表查询参数。
#[cfg_attr(debug_assertions, derive(IntoParams, ToSchema))]
#[derive(Debug, Deserialize)]
pub struct ExampleQuery {
    /// 页码。
    #[cfg_attr(debug_assertions, param(example = 1))]
    pub page: Option<u64>,
    /// 每页条数。
    #[cfg_attr(debug_assertions, param(example = 10))]
    pub size: Option<u64>,
    /// 关键字过滤。
    #[cfg_attr(debug_assertions, param(example = "服务"))]
    pub keyword: Option<String>,
}

/// 示例列表单项。
#[cfg_attr(debug_assertions, derive(ToSchema))]
#[derive(Debug, Serialize)]
pub struct ExampleListItem {
    /// 唯一 ID。
    #[cfg_attr(debug_assertions, schema(example = "example_001"))]
    pub id: String,
    /// 标题。
    #[cfg_attr(debug_assertions, schema(example = "服务模板"))]
    pub title: String,
    /// 摘要。
    #[cfg_attr(debug_assertions, schema(example = "用于演示分页和查询参数"))]
    pub summary: String,
}

/// 示例列表返回体。
#[cfg_attr(debug_assertions, derive(ToSchema))]
#[derive(Debug, Serialize)]
pub struct ExampleListResponse {
    /// 当前页码。
    #[cfg_attr(debug_assertions, schema(example = 1))]
    pub page: u64,
    /// 每页条数。
    #[cfg_attr(debug_assertions, schema(example = 10))]
    pub size: u64,
    /// 当前关键字。
    #[cfg_attr(debug_assertions, schema(example = "服务"))]
    pub keyword: Option<String>,
    /// 列表结果。
    pub items: Vec<ExampleListItem>,
}

/// 示例详情返回体。
#[cfg_attr(debug_assertions, derive(ToSchema))]
#[derive(Debug, Serialize)]
pub struct ExampleDetailResponse {
    /// 示例 ID。
    #[cfg_attr(debug_assertions, schema(example = "example_001"))]
    pub id: String,
    /// 标题。
    #[cfg_attr(debug_assertions, schema(example = "服务模板"))]
    pub title: String,
    /// 详细描述。
    #[cfg_attr(debug_assertions, schema(example = "这是一个用于扩展新模块的详情接口模板。"))]
    pub description: String,
    /// 当前请求人。
    #[cfg_attr(debug_assertions, schema(example = "demo-admin"))]
    pub requested_by: String,
}
