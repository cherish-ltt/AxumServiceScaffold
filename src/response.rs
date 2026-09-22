use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::Local;
use serde::{Serialize, Serializer, ser::SerializeMap};

/// 统一 API 响应结构。
///
/// 状态码只保留一份真值：`status` 是唯一来源，响应体里的 `code` 在序列化时由它派生，
/// 因此 HTTP 状态码与 `code` 字段不可能出现不一致。
#[derive(Debug)]
pub struct ApiResponse<T> {
    status: StatusCode,
    pub message: String,
    pub data: Option<T>,
    pub timestamp: i64,
}

impl<T> ApiResponse<T> {
    /// 指定状态码构造响应，其余构造器都基于它。
    pub fn with_status(status: StatusCode, message: impl Into<String>, data: Option<T>) -> Self {
        Self {
            status,
            message: message.into(),
            data,
            timestamp: Local::now().timestamp_millis(),
        }
    }

    /// 使用默认成功文案返回带数据的响应。
    pub fn ok(data: T) -> Self {
        Self::with_status(StatusCode::OK, "成功", Some(data))
    }

    /// 返回带自定义消息的成功响应。
    pub fn ok_with_message(message: impl Into<String>, data: T) -> Self {
        Self::with_status(StatusCode::OK, message, Some(data))
    }

    /// 返回该响应实际使用的 HTTP 状态码。
    pub fn status(&self) -> StatusCode {
        self.status
    }
}

impl ApiResponse<()> {
    /// 返回不带数据的消息响应。
    pub fn message(message: impl Into<String>) -> Self {
        Self::with_status(StatusCode::OK, message, None)
    }

    /// 返回错误消息响应。
    pub fn error(status: StatusCode, message: impl Into<String>) -> Self {
        Self::with_status(status, message, None)
    }
}

impl<T: Serialize> Serialize for ApiResponse<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("code", &self.status.as_u16())?;
        map.serialize_entry("message", &self.message)?;
        if let Some(data) = &self.data {
            map.serialize_entry("data", data)?;
        }
        map.serialize_entry("timestamp", &self.timestamp)?;
        map.end()
    }
}

impl<T: Serialize> IntoResponse for ApiResponse<T> {
    fn into_response(self) -> Response {
        (self.status, Json(self)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::ApiResponse;
    use axum::{http::StatusCode, response::IntoResponse};

    #[test]
    fn ok_variants_use_success_defaults() {
        let response = ApiResponse::ok("data");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.message, "成功");
        assert_eq!(response.data, Some("data"));
        assert!(response.timestamp > 0);

        let response = ApiResponse::ok_with_message("创建成功", "data");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.message, "创建成功");
        assert!(response.data.is_some());

        let response = ApiResponse::<()>::message("服务已就绪");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.message, "服务已就绪");
        assert!(response.data.is_none());

        let response = ApiResponse::<()>::error(StatusCode::SERVICE_UNAVAILABLE, "服务暂不可用");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.message, "服务暂不可用");
        assert!(response.data.is_none());
    }

    #[test]
    fn code_field_is_derived_from_status() {
        let response = ApiResponse::with_status(StatusCode::CREATED, "已创建", Some("x"));
        let json = serde_json::to_value(&response).expect("序列化响应");

        assert_eq!(json["code"], 201);
        assert_eq!(json["message"], "已创建");
        assert_eq!(json["data"], "x");
        assert!(json["timestamp"].is_i64());
    }

    #[test]
    fn none_data_is_skipped_when_serializing() {
        let with_data = serde_json::to_value(ApiResponse::ok("x")).expect("序列化成功响应");
        assert!(with_data.get("data").is_some());

        let without_data =
            serde_json::to_value(ApiResponse::<()>::message("ok")).expect("序列化消息响应");
        assert!(without_data.get("data").is_none());
        assert_eq!(without_data["code"], 200);
    }

    #[test]
    fn into_response_keeps_status_and_json_content_type() {
        let response =
            ApiResponse::with_status(StatusCode::ACCEPTED, "已受理", Some("x")).into_response();

        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let content_type = response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .expect("content-type 头存在")
            .to_str()
            .expect("content-type 可读");
        assert_eq!(content_type, "application/json");
    }
}
