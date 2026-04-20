use std::sync::Arc;

use async_trait::async_trait;
use chrono::Local;
use sea_orm::DatabaseConnection;

use crate::{
    domain::{
        error::AppError,
        models::system::{HealthReport, WelcomeInfo},
        services::system::SystemUseCase,
    },
    infrastructure::{config::AppConfig, databases::ping_database},
};

pub struct SystemService {
    config: Arc<AppConfig>,
    database: DatabaseConnection,
}

impl SystemService {
    pub fn new(config: Arc<AppConfig>, database: DatabaseConnection) -> Self {
        Self { config, database }
    }
}

#[async_trait]
impl SystemUseCase for SystemService {
    async fn welcome(&self) -> Result<WelcomeInfo, AppError> {
        Ok(WelcomeInfo {
            service_name: self.config.app_name.clone(),
            environment: self.config.app_env.clone(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            docs_enabled: cfg!(debug_assertions),
        })
    }

    async fn health(&self) -> Result<HealthReport, AppError> {
        let database_status = if ping_database(&self.database).await.is_ok() {
            "up".to_string()
        } else {
            "down".to_string()
        };

        let status = if database_status == "up" {
            "ok".to_string()
        } else {
            "degraded".to_string()
        };

        Ok(HealthReport {
            service_name: self.config.app_name.clone(),
            environment: self.config.app_env.clone(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            status,
            database_status,
            timestamp: Local::now().timestamp_millis(),
        })
    }

    async fn ready(&self) -> Result<(), AppError> {
        let health = self.health().await?;
        if health.database_status != "up" {
            return Err(AppError::unavailable("数据库尚未就绪"));
        }

        Ok(())
    }
}
