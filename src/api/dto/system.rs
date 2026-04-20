use serde::Serialize;

use crate::domain::models::system::{HealthReport, WelcomeInfo};

#[cfg(debug_assertions)]
use utoipa::ToSchema;

#[cfg_attr(debug_assertions, derive(ToSchema))]
#[derive(Debug, Serialize)]
pub struct WelcomeResponse {
    #[cfg_attr(debug_assertions, schema(example = "axum-service-scaffold"))]
    pub service_name: String,
    #[cfg_attr(debug_assertions, schema(example = "development"))]
    pub environment: String,
    #[cfg_attr(debug_assertions, schema(example = "0.1.0"))]
    pub version: String,
    #[cfg_attr(debug_assertions, schema(example = true))]
    pub docs_enabled: bool,
}

impl From<WelcomeInfo> for WelcomeResponse {
    fn from(value: WelcomeInfo) -> Self {
        Self {
            service_name: value.service_name,
            environment: value.environment,
            version: value.version,
            docs_enabled: value.docs_enabled,
        }
    }
}

#[cfg_attr(debug_assertions, derive(ToSchema))]
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    #[cfg_attr(debug_assertions, schema(example = "axum-service-scaffold"))]
    pub service_name: String,
    #[cfg_attr(debug_assertions, schema(example = "development"))]
    pub environment: String,
    #[cfg_attr(debug_assertions, schema(example = "0.1.0"))]
    pub version: String,
    #[cfg_attr(debug_assertions, schema(example = "ok"))]
    pub status: String,
    #[cfg_attr(debug_assertions, schema(example = "up"))]
    pub database_status: String,
    #[cfg_attr(debug_assertions, schema(example = 1713179523000i64))]
    pub timestamp: i64,
}

impl From<HealthReport> for HealthResponse {
    fn from(value: HealthReport) -> Self {
        Self {
            service_name: value.service_name,
            environment: value.environment,
            version: value.version,
            status: value.status,
            database_status: value.database_status,
            timestamp: value.timestamp,
        }
    }
}
