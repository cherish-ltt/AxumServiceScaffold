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
