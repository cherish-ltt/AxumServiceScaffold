use chrono::Local;

use crate::{db::ping_database, state::AppState};

use super::dto::{HealthResponse, WelcomeResponse};

/// 构建首页欢迎信息。
pub fn build_welcome_response(state: &AppState) -> WelcomeResponse {
    WelcomeResponse {
        service_name: state.config.app_name.clone(),
        environment: state.config.app_env.clone(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        docs_enabled: cfg!(debug_assertions),
    }
}

/// 构建健康检查结果。
pub async fn build_health_response(state: &AppState) -> HealthResponse {
    let database_status = match ping_database(&state.database).await {
        Ok(_) => "up".to_string(),
        Err(_) => "down".to_string(),
    };

    let status = if database_status == "up" {
        "ok".to_string()
    } else {
        "degraded".to_string()
    };

    HealthResponse {
        service_name: state.config.app_name.clone(),
        environment: state.config.app_env.clone(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        status,
        database_status,
        timestamp: Local::now().timestamp_millis(),
    }
}
