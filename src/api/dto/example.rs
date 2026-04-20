use serde::{Deserialize, Serialize};

use crate::domain::models::example::{
    CreateExampleCommand, ExampleDetail, ExampleEcho, ExampleFilters, ExampleItem, ExampleList,
};

#[allow(unused_imports)]
#[cfg(debug_assertions)]
use serde_json::json;
#[cfg(debug_assertions)]
use utoipa::{IntoParams, ToSchema};

#[cfg_attr(debug_assertions, derive(ToSchema))]
#[derive(Debug, Deserialize)]
pub struct ExampleEchoRequest {
    #[cfg_attr(debug_assertions, schema(example = "搭建新服务"))]
    pub title: String,
    #[cfg_attr(debug_assertions, schema(example = "先接入日志和 JWT"))]
    pub note: Option<String>,
}

impl From<ExampleEchoRequest> for CreateExampleCommand {
    fn from(value: ExampleEchoRequest) -> Self {
        Self {
            title: value.title,
            note: value.note,
        }
    }
}

#[cfg_attr(debug_assertions, derive(ToSchema))]
#[derive(Debug, Serialize)]
pub struct ExampleEchoResponse {
    #[cfg_attr(
        debug_assertions,
        schema(example = "019680cc-7e1c-7ec0-b7b8-4b4f8e9dff10")
    )]
    pub id: String,
    #[cfg_attr(debug_assertions, schema(example = "搭建新服务"))]
    pub title: String,
    #[cfg_attr(debug_assertions, schema(example = "先接入日志和 JWT"))]
    pub note: Option<String>,
    #[cfg_attr(debug_assertions, schema(example = "example-service"))]
    pub source: String,
}

impl From<ExampleEcho> for ExampleEchoResponse {
    fn from(value: ExampleEcho) -> Self {
        Self {
            id: value.id,
            title: value.title,
            note: value.note,
            source: value.source,
        }
    }
}

#[cfg_attr(debug_assertions, derive(IntoParams, ToSchema))]
#[derive(Debug, Deserialize)]
pub struct ExampleQuery {
    #[cfg_attr(debug_assertions, param(example = 1))]
    pub page: Option<u64>,
    #[cfg_attr(debug_assertions, param(example = 10))]
    pub size: Option<u64>,
    #[cfg_attr(debug_assertions, param(example = "服务"))]
    pub keyword: Option<String>,
}

impl From<ExampleQuery> for ExampleFilters {
    fn from(value: ExampleQuery) -> Self {
        Self {
            page: value.page,
            size: value.size,
            keyword: value.keyword,
        }
    }
}

#[cfg_attr(debug_assertions, derive(ToSchema))]
#[derive(Debug, Serialize)]
pub struct ExampleListItem {
    #[cfg_attr(debug_assertions, schema(example = "example_001"))]
    pub id: String,
    #[cfg_attr(debug_assertions, schema(example = "服务模板"))]
    pub title: String,
    #[cfg_attr(debug_assertions, schema(example = "用于演示分页和查询参数"))]
    pub summary: String,
}

impl From<ExampleItem> for ExampleListItem {
    fn from(value: ExampleItem) -> Self {
        Self {
            id: value.id,
            title: value.title,
            summary: value.summary,
        }
    }
}

#[cfg_attr(debug_assertions, derive(ToSchema))]
#[derive(Debug, Serialize)]
pub struct ExampleListResponse {
    #[cfg_attr(debug_assertions, schema(example = 1))]
    pub page: u64,
    #[cfg_attr(debug_assertions, schema(example = 10))]
    pub size: u64,
    #[cfg_attr(debug_assertions, schema(example = "服务"))]
    pub keyword: Option<String>,
    pub items: Vec<ExampleListItem>,
}

impl From<ExampleList> for ExampleListResponse {
    fn from(value: ExampleList) -> Self {
        Self {
            page: value.page,
            size: value.size,
            keyword: value.keyword,
            items: value.items.into_iter().map(Into::into).collect(),
        }
    }
}

#[cfg_attr(debug_assertions, derive(ToSchema))]
#[derive(Debug, Serialize)]
pub struct ExampleDetailResponse {
    #[cfg_attr(debug_assertions, schema(example = "example_001"))]
    pub id: String,
    #[cfg_attr(debug_assertions, schema(example = "服务模板"))]
    pub title: String,
    #[cfg_attr(
        debug_assertions,
        schema(example = "这是一个用于扩展新模块的详情接口模板。")
    )]
    pub description: String,
    #[cfg_attr(debug_assertions, schema(example = "demo-admin"))]
    pub requested_by: String,
    #[cfg_attr(debug_assertions, schema(example = json!(["developer", "admin"])))]
    pub roles: Vec<String>,
}

impl From<ExampleDetail> for ExampleDetailResponse {
    fn from(value: ExampleDetail) -> Self {
        Self {
            id: value.id,
            title: value.title,
            description: value.description,
            requested_by: value.requested_by,
            roles: value.roles,
        }
    }
}
