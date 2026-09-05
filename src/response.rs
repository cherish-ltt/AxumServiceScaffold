use chrono::Local;
use serde::{Deserialize, Serialize};

/// 统一 API 响应结构。
///
/// 建议整个项目长期保持这一套结构，避免同一个服务中出现多种风格的返回值。
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub code: u16,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    pub timestamp: i64,
}

impl<T> ApiResponse<T> {
    /// 使用默认成功文案返回带数据的响应。
    pub fn ok(data: T) -> Self {
        Self::with_parts(200, "成功", Some(data))
    }

    /// 返回带自定义消息的成功响应。
    pub fn ok_with_message(message: impl Into<String>, data: T) -> Self {
        Self::with_parts(200, message, Some(data))
    }

    /// 返回任意状态码与消息。
    pub fn with_parts(code: u16, message: impl Into<String>, data: Option<T>) -> Self {
        Self {
            code,
            message: message.into(),
            data,
            timestamp: Local::now().timestamp_millis(),
        }
    }
}

impl ApiResponse<()> {
    /// 返回不带数据的消息响应。
    pub fn message(message: impl Into<String>) -> Self {
        Self::with_parts(200, message, None)
    }

    /// 返回错误消息响应。
    pub fn error(code: u16, message: impl Into<String>) -> Self {
        Self::with_parts(code, message, None)
    }
}

#[cfg(test)]
mod tests {
    use super::ApiResponse;

    #[test]
    fn ok_variants_use_success_defaults() {
        let response = ApiResponse::ok("data");
        assert_eq!(response.code, 200);
        assert_eq!(response.message, "成功");
        assert_eq!(response.data, Some("data"));
        assert!(response.timestamp > 0);

        let response = ApiResponse::ok_with_message("创建成功", "data");
        assert_eq!(response.code, 200);
        assert_eq!(response.message, "创建成功");
        assert!(response.data.is_some());

        let response = ApiResponse::<()>::message("服务已就绪");
        assert_eq!(response.code, 200);
        assert!(response.data.is_none());

        let response = ApiResponse::<()>::error(503, "服务暂不可用");
        assert_eq!(response.code, 503);
        assert_eq!(response.message, "服务暂不可用");

        let response = ApiResponse::<&str>::with_parts(404, "资源缺失", None);
        assert_eq!(response.code, 404);
        assert!(response.data.is_none());
    }

    #[test]
    fn none_data_is_skipped_when_serializing() {
        let with_data = serde_json::to_value(ApiResponse::ok("x")).expect("序列化成功响应");
        assert!(with_data.get("data").is_some());

        let without_data =
            serde_json::to_value(ApiResponse::<()>::message("ok")).expect("序列化消息响应");
        assert!(without_data.get("data").is_none());

        let parsed: ApiResponse<String> = serde_json::from_value(with_data).expect("反序列化响应");
        assert_eq!(parsed.message, "成功");
        assert_eq!(parsed.data.as_deref(), Some("x"));
    }
}
