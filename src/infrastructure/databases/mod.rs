use std::time::Duration;

use anyhow::{Context, Result};
use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection};
use tokio::time::{Duration as TokioDuration, timeout};

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

    ping_database(&database, config.connect_timeout_secs).await?;

    Ok(database)
}

pub async fn ping_database(database: &DatabaseConnection, timeout_secs: u64) -> Result<()> {
    timeout(TokioDuration::from_secs(timeout_secs), database.ping())
        .await
        .context("数据库健康检查超时")?
        .context("数据库健康检查失败")?;

    Ok(())
}

pub async fn run_migrations(database: &DatabaseConnection) -> Result<()> {
    database
        .execute_unprepared(
            "CREATE TABLE IF NOT EXISTS _schema_migrations (version TEXT PRIMARY KEY NOT NULL, applied_at TEXT NOT NULL)",
        )
        .await
        .context("创建数据库迁移表失败")?;
    Ok(())
}
