use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use tracing::error;

pub use crate::domain::error::AppError;
use crate::response::ApiResponse;

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.http_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let message = match status {
            StatusCode::BAD_REQUEST
            | StatusCode::UNAUTHORIZED
            | StatusCode::NOT_FOUND
            | StatusCode::CONFLICT => self.to_string(),
            StatusCode::SERVICE_UNAVAILABLE => {
                error!(error = %self, "服务不可用");
                "服务暂不可用".to_string()
            },
            _ => {
                error!(error = %self, "请求处理失败");
                "服务器内部错误".to_string()
            },
        };
        ApiResponse::<()>::error(status, message).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::AppError;
    use axum::body::to_bytes;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    #[test]
    fn each_error_maps_to_expected_status() {
        let cases = [
            (AppError::bad_request("参数错误"), 400),
            (AppError::unauthorized("未授权"), 401),
            (AppError::not_found("资源不存在"), 404),
            (AppError::conflict("幂等键重复"), 409),
            (AppError::unavailable("数据库未就绪"), 503),
            (AppError::internal("意外失败"), 500),
            (AppError::Config("配置缺失".to_string()), 500),
        ];

        for (error, expected) in cases {
            let response = error.into_response();
            assert_eq!(response.status().as_u16(), expected);
        }
    }

    #[tokio::test]
    async fn client_errors_keep_message() {
        let response = AppError::bad_request("标题不能为空").into_response();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("读取响应体");
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("解析响应体");

        assert_eq!(body["code"], 400);
        assert_eq!(body["message"], "请求参数错误: 标题不能为空");
    }

    #[tokio::test]
    async fn server_errors_are_masked() {
        let errors = [
            AppError::unavailable("数据库未就绪"),
            AppError::internal("意外失败"),
        ];

        for error in errors {
            let raw_message = error.to_string();
            let response = error.into_response();
            let bytes = to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("读取响应体");
            let body: serde_json::Value = serde_json::from_slice(&bytes).expect("解析响应体");
            let message = body["message"].as_str().expect("message 字段为字符串");

            assert!(
                !message.contains(&raw_message),
                "服务端错误不应透出内部信息"
            );
            assert!(body.get("data").is_none());
        }
    }

    #[test]
    fn unavailable_maps_to_service_unavailable() {
        let response = AppError::unavailable("数据库未就绪").into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
