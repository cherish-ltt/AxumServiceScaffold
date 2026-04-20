use uuid::Uuid;

use crate::auth::extractor::CurrentUser;

use super::dto::{
    ExampleDetailResponse, ExampleEchoRequest, ExampleEchoResponse, ExampleListItem,
    ExampleListResponse, ExampleQuery,
};

/// 构建示例创建响应。
pub fn build_echo_response(payload: ExampleEchoRequest) -> ExampleEchoResponse {
    ExampleEchoResponse {
        id: Uuid::now_v7().to_string(),
        title: payload.title,
        note: payload.note,
        source: "example-module".to_string(),
    }
}

/// 构建示例分页列表。
pub fn build_example_list(query: ExampleQuery) -> ExampleListResponse {
    let page = query.page.unwrap_or(1);
    let size = query.size.unwrap_or(10);
    let keyword = query.keyword.clone();

    let mut items = vec![
        ExampleListItem {
            id: "example_001".to_string(),
            title: "服务模板".to_string(),
            summary: "用于演示分页和查询参数".to_string(),
        },
        ExampleListItem {
            id: "example_002".to_string(),
            title: "鉴权样例".to_string(),
            summary: "用于演示 JWT 保护接口".to_string(),
        },
        ExampleListItem {
            id: "example_003".to_string(),
            title: "Swagger 样例".to_string(),
            summary: "用于演示 OpenAPI 注解组织方式".to_string(),
        },
    ];

    if let Some(keyword) = &keyword {
        items.retain(|item| item.title.contains(keyword) || item.summary.contains(keyword));
    }

    ExampleListResponse {
        page,
        size,
        keyword,
        items,
    }
}

/// 构建单个示例详情。
pub fn build_example_detail(id: String, current_user: &CurrentUser) -> ExampleDetailResponse {
    ExampleDetailResponse {
        id,
        title: "服务模板".to_string(),
        description: "这是一个用于扩展新模块的详情接口模板。".to_string(),
        requested_by: current_user.username.clone(),
    }
}
