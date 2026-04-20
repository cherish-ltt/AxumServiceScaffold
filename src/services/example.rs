use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::{
    error::AppError,
    models::{
        auth::CurrentUser,
        example::{
            CreateExampleCommand, ExampleDetail, ExampleEcho, ExampleFilters, ExampleItem,
            ExampleList,
        },
    },
    services::example::ExampleUseCase,
};

pub struct ExampleService;

impl ExampleService {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ExampleUseCase for ExampleService {
    async fn create_echo(&self, command: CreateExampleCommand) -> Result<ExampleEcho, AppError> {
        let title = command.title.trim();
        if title.is_empty() {
            return Err(AppError::bad_request("标题不能为空"));
        }

        Ok(ExampleEcho {
            id: Uuid::now_v7().to_string(),
            title: title.to_string(),
            note: command.note,
            source: "example-service".to_string(),
        })
    }

    async fn list_examples(&self, filters: ExampleFilters) -> Result<ExampleList, AppError> {
        let page = filters.page.unwrap_or(1);
        let size = filters.size.unwrap_or(10);

        if page == 0 {
            return Err(AppError::bad_request("page 必须从 1 开始"));
        }

        if size == 0 {
            return Err(AppError::bad_request("size 必须大于 0"));
        }

        let keyword = filters.keyword;
        let mut items = vec![
            ExampleItem {
                id: "example_001".to_string(),
                title: "服务模板".to_string(),
                summary: "用于演示分页和查询参数".to_string(),
            },
            ExampleItem {
                id: "example_002".to_string(),
                title: "鉴权样例".to_string(),
                summary: "用于演示 JWT 保护接口".to_string(),
            },
            ExampleItem {
                id: "example_003".to_string(),
                title: "Swagger 样例".to_string(),
                summary: "用于演示 OpenAPI 注解组织方式".to_string(),
            },
        ];

        if let Some(keyword) = &keyword {
            items.retain(|item| item.title.contains(keyword) || item.summary.contains(keyword));
        }

        Ok(ExampleList {
            page,
            size,
            keyword,
            items,
        })
    }

    async fn get_example_detail(
        &self,
        id: String,
        current_user: CurrentUser,
    ) -> Result<ExampleDetail, AppError> {
        if id.trim().is_empty() {
            return Err(AppError::not_found("示例 ID 不存在"));
        }

        Ok(ExampleDetail::new(id, current_user))
    }
}
