use anyhow::{Context, Result};
use sea_orm::{ConnectionTrait, DatabaseConnection};

/// 创建事务示例所需的表。
///
/// 语句保持 SQLite / MySQL / PostgreSQL 通用；列名与 `src/entities` 中的实体一一对应。
pub async fn create_transfer_tables(database: &DatabaseConnection) -> Result<()> {
    for statement in [
        ACCOUNT_TABLE,
        RECORD_TABLE,
        AUDIT_TABLE,
        RECORD_INDEX,
        AUDIT_INDEX,
    ] {
        database
            .execute_unprepared(statement)
            .await
            .context("创建事务示例表失败")?;
    }

    Ok(())
}

const ACCOUNT_TABLE: &str = "CREATE TABLE IF NOT EXISTS transfer_accounts (\
     id TEXT PRIMARY KEY NOT NULL, \
     owner TEXT NOT NULL, \
     balance_cents BIGINT NOT NULL, \
     version BIGINT NOT NULL, \
     updated_at BIGINT NOT NULL)";

const RECORD_TABLE: &str = "CREATE TABLE IF NOT EXISTS transfer_records (\
     id TEXT PRIMARY KEY NOT NULL, \
     from_account_id TEXT NOT NULL, \
     to_account_id TEXT NOT NULL, \
     amount_cents BIGINT NOT NULL, \
     from_balance_after_cents BIGINT NOT NULL, \
     to_balance_after_cents BIGINT NOT NULL, \
     remark TEXT, \
     created_at BIGINT NOT NULL)";

const AUDIT_TABLE: &str = "CREATE TABLE IF NOT EXISTS transfer_audits (\
     id TEXT PRIMARY KEY NOT NULL, \
     record_id TEXT NOT NULL, \
     action TEXT NOT NULL, \
     detail TEXT NOT NULL, \
     created_at BIGINT NOT NULL)";

const RECORD_INDEX: &str =
    "CREATE INDEX IF NOT EXISTS idx_transfer_records_created_at ON transfer_records (created_at)";

const AUDIT_INDEX: &str =
    "CREATE INDEX IF NOT EXISTS idx_transfer_audits_record_id ON transfer_audits (record_id)";

const SEED_ALICE: &str = "INSERT INTO transfer_accounts (id, owner, balance_cents, version, updated_at) \
     SELECT 'acc_alice', 'Alice', 100000, 0, 0 \
     WHERE NOT EXISTS (SELECT 1 FROM transfer_accounts WHERE id = 'acc_alice')";

const SEED_BOB: &str = "INSERT INTO transfer_accounts (id, owner, balance_cents, version, updated_at) \
     SELECT 'acc_bob', 'Bob', 100000, 0, 0 \
     WHERE NOT EXISTS (SELECT 1 FROM transfer_accounts WHERE id = 'acc_bob')";

/// 首次启动时写入两个演示账户，重复启动不会重复插入。
///
/// 使用 `NOT EXISTS` 而不是先查后写，让播种本身也是幂等且原子的。
/// 每个语句单独执行，避免依赖驱动的多语句支持。
pub async fn seed_transfer_accounts(database: &DatabaseConnection) -> Result<()> {
    for statement in [SEED_ALICE, SEED_BOB] {
        database
            .execute_unprepared(statement)
            .await
            .context("写入事务示例种子账户失败")?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use sea_orm::{Database, DatabaseConnection, EntityTrait};
    use uuid::Uuid;

    use super::{create_transfer_tables, seed_transfer_accounts};
    use crate::entities::{transfer_account, transfer_audit, transfer_record};

    async fn setup_database() -> DatabaseConnection {
        let url = format!(
            "sqlite://{}?mode=rwc",
            std::env::temp_dir()
                .join(format!("axum-scaffold-schema-test-{}.db", Uuid::now_v7()))
                .display()
        );

        Database::connect(url).await.expect("连接临时数据库")
    }

    #[tokio::test]
    async fn tables_and_seed_are_created_once() {
        let database = setup_database().await;

        create_transfer_tables(&database).await.expect("建表成功");
        seed_transfer_accounts(&database).await.expect("播种成功");
        // 重复执行验证幂等性：再次启动服务不应产生重复账户或建表错误。
        create_transfer_tables(&database)
            .await
            .expect("重复建表成功");
        seed_transfer_accounts(&database)
            .await
            .expect("重复播种成功");

        let accounts = transfer_account::Entity::find()
            .all(&database)
            .await
            .expect("查询账户");
        assert_eq!(accounts.len(), 2);
        assert!(
            accounts
                .iter()
                .all(|account| account.balance_cents == 100000)
        );
        assert!(
            transfer_account::Entity::find_by_id("acc_alice".to_string())
                .one(&database)
                .await
                .expect("按主键查询")
                .is_some()
        );

        let records = transfer_record::Entity::find()
            .all(&database)
            .await
            .expect("查询流水");
        assert!(records.is_empty());

        let audits = transfer_audit::Entity::find()
            .all(&database)
            .await
            .expect("查询审计日志");
        assert!(audits.is_empty());
    }
}
