use axum::{
    Json,
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
            StatusCode::BAD_REQUEST | StatusCode::UNAUTHORIZED | StatusCode::NOT_FOUND => {
                self.to_string()
            }
            StatusCode::SERVICE_UNAVAILABLE => {
                error!(error = %self, "服务不可用");
                "服务暂不可用".to_string()
            }
            _ => {
                error!(error = %self, "请求处理失败");
                "服务器内部错误".to_string()
            }
        };
        let body = ApiResponse::<()>::error(status.as_u16(), message);
        (status, Json(body)).into_response()
    }
}
