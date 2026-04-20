use serde::Serialize;

#[cfg(debug_assertions)]
use utoipa::ToSchema;

/// 首页欢迎信息。
#[cfg_attr(debug_assertions, derive(ToSchema))]
#[derive(Debug, Serialize)]
pub struct WelcomeResponse {
    /// 服务名称。
    #[cfg_attr(debug_assertions, schema(example = "axum-service-scaffold"))]
    pub service_name: String,
    /// 当前环境。
    #[cfg_attr(debug_assertions, schema(example = "development"))]
    pub environment: String,
    /// 当前版本。
    #[cfg_attr(debug_assertions, schema(example = "0.1.0"))]
    pub version: String,
    /// 是否启用 Swagger。
    #[cfg_attr(debug_assertions, schema(example = true))]
    pub docs_enabled: bool,
}

/// 健康检查响应体。
#[cfg_attr(debug_assertions, derive(ToSchema))]
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// 服务名称。
    #[cfg_attr(debug_assertions, schema(example = "axum-service-scaffold"))]
    pub service_name: String,
    /// 当前环境。
    #[cfg_attr(debug_assertions, schema(example = "development"))]
    pub environment: String,
    /// 当前版本。
    #[cfg_attr(debug_assertions, schema(example = "0.1.0"))]
    pub version: String,
    /// 总体状态。
    #[cfg_attr(debug_assertions, schema(example = "ok"))]
    pub status: String,
    /// 数据库状态。
    #[cfg_attr(debug_assertions, schema(example = "up"))]
    pub database_status: String,
    /// 当前时间戳，毫秒。
    #[cfg_attr(debug_assertions, schema(example = 1713179523000i64))]
    pub timestamp: i64,
}
