use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("配置错误: {0}")]
    Config(String),
    #[error("请求参数错误: {0}")]
    BadRequest(String),
    #[error("未授权访问: {0}")]
    Unauthorized(String),
    #[error("资源不存在: {0}")]
    NotFound(String),
    #[error("服务暂不可用: {0}")]
    Unavailable(String),
    #[error("数据库错误: {0}")]
    Database(String),
    #[error("内部错误: {0}")]
    Internal(String),
}

impl AppError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest(message.into())
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized(message.into())
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::Unavailable(message.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    pub fn http_code(&self) -> u16 {
        match self {
            Self::Config(_) | Self::Internal(_) | Self::Database(_) => 500,
            Self::BadRequest(_) => 400,
            Self::Unauthorized(_) => 401,
            Self::NotFound(_) => 404,
            Self::Unavailable(_) => 503,
        }
    }
}

impl From<anyhow::Error> for AppError {
    fn from(error: anyhow::Error) -> Self {
        Self::Internal(error.to_string())
    }
}

impl From<sea_orm::DbErr> for AppError {
    fn from(error: sea_orm::DbErr) -> Self {
        Self::Database(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::AppError;

    #[test]
    fn constructors_match_variants() {
        assert!(matches!(
            AppError::bad_request("x"),
            AppError::BadRequest(_)
        ));
        assert!(matches!(
            AppError::unauthorized("x"),
            AppError::Unauthorized(_)
        ));
        assert!(matches!(AppError::not_found("x"), AppError::NotFound(_)));
        assert!(matches!(
            AppError::unavailable("x"),
            AppError::Unavailable(_)
        ));
        assert!(matches!(AppError::internal("x"), AppError::Internal(_)));
    }

    #[test]
    fn http_codes_match_design() {
        assert_eq!(AppError::bad_request("x").http_code(), 400);
        assert_eq!(AppError::unauthorized("x").http_code(), 401);
        assert_eq!(AppError::not_found("x").http_code(), 404);
        assert_eq!(AppError::unavailable("x").http_code(), 503);
        assert_eq!(AppError::internal("x").http_code(), 500);
        assert_eq!(AppError::Config("x".to_string()).http_code(), 500);
        assert_eq!(AppError::Database("x".to_string()).http_code(), 500);
    }

    #[test]
    fn display_messages_include_details() {
        assert_eq!(
            AppError::bad_request("标题不能为空").to_string(),
            "请求参数错误: 标题不能为空"
        );
        assert_eq!(
            AppError::unauthorized("令牌过期").to_string(),
            "未授权访问: 令牌过期"
        );
        assert_eq!(
            AppError::not_found("example_001").to_string(),
            "资源不存在: example_001"
        );
        assert_eq!(
            AppError::unavailable("数据库未就绪").to_string(),
            "服务暂不可用: 数据库未就绪"
        );
        assert_eq!(
            AppError::Config("缺字段".to_string()).to_string(),
            "配置错误: 缺字段"
        );
        assert_eq!(
            AppError::Database("连接失败".to_string()).to_string(),
            "数据库错误: 连接失败"
        );
        assert_eq!(
            AppError::Internal("panic".to_string()).to_string(),
            "内部错误: panic"
        );
    }

    #[test]
    fn anyhow_error_converts_to_internal() {
        let error: AppError = anyhow::anyhow!("boom").into();
        assert!(matches!(error, AppError::Internal(message) if message.contains("boom")));
    }

    #[test]
    fn db_error_converts_to_database() {
        let error: AppError = sea_orm::DbErr::Custom("连接失败".to_string()).into();
        assert!(matches!(error, AppError::Database(message) if message.contains("连接失败")));
    }
}
