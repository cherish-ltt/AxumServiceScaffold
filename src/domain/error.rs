use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Bad request: {0}")]
    BadRequest(String),
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Conflict: {0}")]
    Conflict(String),
    #[error("Service unavailable: {0}")]
    Unavailable(String),
    #[error("Database error: {0}")]
    Database(String),
    #[error("Internal error: {0}")]
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

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict(message.into())
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
            Self::Conflict(_) => 409,
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
        // 唯一约束冲突属于客户端可控冲突（如幂等键重复提交），映射为 409 而非 500。
        if let Some(sea_orm::SqlErr::UniqueConstraintViolation(detail)) = error.sql_err() {
            return Self::Conflict(detail);
        }

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
        assert!(matches!(AppError::conflict("x"), AppError::Conflict(_)));
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
        assert_eq!(AppError::conflict("x").http_code(), 409);
        assert_eq!(AppError::unavailable("x").http_code(), 503);
        assert_eq!(AppError::internal("x").http_code(), 500);
        assert_eq!(AppError::Config("x".to_string()).http_code(), 500);
        assert_eq!(AppError::Database("x".to_string()).http_code(), 500);
    }

    #[test]
    fn display_messages_include_details() {
        assert_eq!(
            AppError::bad_request("title must not be empty").to_string(),
            "Bad request: title must not be empty"
        );
        assert_eq!(
            AppError::unauthorized("token expired").to_string(),
            "Unauthorized: token expired"
        );
        assert_eq!(
            AppError::not_found("example_001").to_string(),
            "Not found: example_001"
        );
        assert_eq!(
            AppError::conflict("duplicate idempotency key").to_string(),
            "Conflict: duplicate idempotency key"
        );
        assert_eq!(
            AppError::unavailable("database not ready").to_string(),
            "Service unavailable: database not ready"
        );
        assert_eq!(
            AppError::Config("missing field".to_string()).to_string(),
            "Configuration error: missing field"
        );
        assert_eq!(
            AppError::Database("connection failed".to_string()).to_string(),
            "Database error: connection failed"
        );
        assert_eq!(
            AppError::Internal("panic".to_string()).to_string(),
            "Internal error: panic"
        );
    }

    #[test]
    fn anyhow_error_converts_to_internal() {
        let error: AppError = anyhow::anyhow!("boom").into();
        assert!(matches!(error, AppError::Internal(message) if message.contains("boom")));
    }

    #[test]
    fn db_error_converts_to_database() {
        let error: AppError = sea_orm::DbErr::Custom("connection failed".to_string()).into();
        assert!(
            matches!(error, AppError::Database(message) if message.contains("connection failed"))
        );
    }

    /// 真实触发数据库唯一约束，确认被识别为 409 冲突而不是 500。
    #[tokio::test]
    async fn unique_constraint_violation_maps_to_conflict() {
        use sea_orm::{ConnectionTrait, Database};

        let url = format!(
            "sqlite://{}?mode=rwc",
            std::env::temp_dir()
                .join(format!(
                    "axum-scaffold-error-test-{}.db",
                    uuid::Uuid::now_v7()
                ))
                .display()
        );
        let database = Database::connect(url).await.expect("连接临时数据库");

        database
            .execute_unprepared(
                "CREATE TABLE demo (id TEXT PRIMARY KEY NOT NULL, request_id TEXT, \
                 CONSTRAINT demo_request_id_unique UNIQUE (request_id))",
            )
            .await
            .expect("建表成功");
        database
            .execute_unprepared("INSERT INTO demo (id, request_id) VALUES ('a', 'dup')")
            .await
            .expect("首次写入成功");

        let db_error = database
            .execute_unprepared("INSERT INTO demo (id, request_id) VALUES ('b', 'dup')")
            .await
            .expect_err("重复幂等键必须被拒绝");

        let error: AppError = db_error.into();
        assert!(
            matches!(error, AppError::Conflict(_)),
            "唯一约束冲突应映射为 Conflict，实际: {error:?}"
        );
        assert_eq!(error.http_code(), 409);
        assert!(
            error.to_string().contains("UNIQUE constraint failed"),
            "冲突信息应保留数据库原因，便于排查: {error}"
        );
    }
}
