use std::time::Duration;

use anyhow::{Context, Result};
use sea_orm::{ConnectOptions, Database, DatabaseConnection};

use crate::infrastructure::config::DatabaseConfig;

pub async fn connect_database(config: &DatabaseConfig) -> Result<DatabaseConnection> {
    let mut options = ConnectOptions::new(config.url.clone());
    options
        .min_connections(config.min_connections)
        .max_connections(config.max_connections)
        .connect_timeout(Duration::from_secs(config.connect_timeout_secs))
        .idle_timeout(Duration::from_secs(config.idle_secs))
        .sqlx_logging(config.sqlx_logging);

    let database = Database::connect(options)
        .await
        .with_context(|| format!("数据库连接失败: {}", config.url))?;

    ping_database(&database).await?;

    Ok(database)
}

pub async fn ping_database(database: &DatabaseConnection) -> Result<()> {
    let _backend = database.get_database_backend();
    database.ping().await.context("数据库健康检查失败")?;

    Ok(())
}
