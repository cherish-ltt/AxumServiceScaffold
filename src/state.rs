use std::sync::Arc;

use anyhow::Result;
use sea_orm::DatabaseConnection;

use crate::{auth::jwt::JwtService, config::AppConfig, db::connect_database};

/// 全局共享状态。
///
/// 这里集中承载所有需要跨模块共享的基础设施依赖。
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub database: DatabaseConnection,
    pub jwt: Arc<JwtService>,
}

impl AppState {
    /// 根据应用配置启动所有基础设施并生成全局状态。
    pub async fn bootstrap(config: AppConfig) -> Result<Self> {
        let database = connect_database(&config.database).await?;
        let jwt = JwtService::new(config.jwt.clone())?;

        Ok(Self {
            config: Arc::new(config),
            database,
            jwt: Arc::new(jwt),
        })
    }
}
