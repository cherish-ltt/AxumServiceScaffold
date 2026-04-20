use std::time::Duration;

use anyhow::{Context, Result};
use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection, Statement};

use crate::config::DatabaseConfig;

/// 建立数据库连接。
///
/// 这里统一管理 SeaORM 的连接参数，后续如果需要调连接池、超时、
/// SQL 日志或重试策略，都建议在这里集中处理。
pub async fn connect_database(config: &DatabaseConfig) -> Result<DatabaseConnection> {
    let mut options = ConnectOptions::new(config.url.clone());
    options
        .min_connections(config.min_connections)
        .max_connections(config.max_connections)
        .connect_timeout(Duration::from_secs(config.connect_timeout_secs))
        .sqlx_logging(config.sqlx_logging);

    let database = Database::connect(options)
        .await
        .with_context(|| format!("数据库连接失败: {}", config.url))?;

    ping_database(&database).await?;

    Ok(database)
}

/// 执行最小 SQL 检查数据库可用性。
pub async fn ping_database(database: &DatabaseConnection) -> Result<()> {
    let backend = database.get_database_backend();
    database
        .execute(Statement::from_string(backend, "SELECT 1".to_string()))
        .await
        .context("数据库健康检查失败")?;

    Ok(())
}
