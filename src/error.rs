use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};

pub use crate::domain::error::AppError;
use crate::response::ApiResponse;

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.http_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = ApiResponse::<()>::error(status.as_u16(), self.to_string());
        (status, Json(body)).into_response()
    }
}
