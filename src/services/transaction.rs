use std::sync::Arc;

use async_trait::async_trait;
use chrono::Local;
use sea_orm::{DatabaseConnection, DatabaseTransaction, IsolationLevel, TransactionTrait};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    domain::{
        error::AppError,
        models::transaction::{
            NewTransferAudit, NewTransferRecord, TransferAccountState, TransferCommand,
            TransferDetail, TransferReceipt, TransferRecordPage, audit_action,
        },
        services::transaction::TransferUseCase,
    },
    infrastructure::repositories::transaction::TransferRepository,
};

const DEFAULT_PAGE: u64 = 1;
const DEFAULT_SIZE: u64 = 10;
const MAX_SIZE: u64 = 100;

/// 转账事务示例的用例实现。
///
/// 事务边界只在这里出现一次：成功提交，任一环节失败则回滚，
/// 余额更新、流水、审计日志要么一起生效、要么一起消失。
pub struct TransferService<R: TransferRepository> {
    database: DatabaseConnection,
    repository: Arc<R>,
}

impl<R: TransferRepository> TransferService<R> {
    pub fn new(database: DatabaseConnection, repository: Arc<R>) -> Self {
        Self {
            database,
            repository,
        }
    }
}

#[async_trait]
impl<R: TransferRepository> TransferUseCase for TransferService<R> {
    async fn transfer(&self, command: TransferCommand) -> Result<TransferReceipt, AppError> {
        validate(&command)?;

        // 参数校验放在开启事务之前，避免为必然失败的请求占用连接。
        let transaction = self
            .database
            .begin_with_config(Some(IsolationLevel::Serializable), None)
            .await?;

        match transfer_in_transaction(&transaction, self.repository.as_ref(), &command).await {
            Ok(receipt) => {
                transaction.commit().await?;
                info!(record_id = %receipt.record_id, "转账事务已提交");
                Ok(receipt)
            },
            Err(error) => {
                // 回滚失败不覆盖业务错误：该连接会在归还连接池时被丢弃重建。
                if let Err(rollback_error) = transaction.rollback().await {
                    warn!(%rollback_error, "转账事务回滚失败");
                }
                info!(%error, "转账事务已回滚");
                Err(error)
            },
        }
    }

    async fn get_transfer(&self, record_id: String) -> Result<TransferDetail, AppError> {
        if record_id.trim().is_empty() {
            return Err(AppError::not_found("流水 ID 不能为空"));
        }

        let record = self
            .repository
            .find_record(&self.database, &record_id)
            .await?
            .ok_or_else(|| AppError::not_found(format!("流水 {record_id} 不存在")))?;
        let audits = self
            .repository
            .list_audits(&self.database, &record_id)
            .await?;

        Ok(TransferDetail { record, audits })
    }

    async fn list_transfers(
        &self,
        page: Option<u64>,
        size: Option<u64>,
    ) -> Result<TransferRecordPage, AppError> {
        let page = page.unwrap_or(DEFAULT_PAGE);
        let size = size.unwrap_or(DEFAULT_SIZE);
        if page == 0 {
            return Err(AppError::bad_request("page 必须从 1 开始"));
        }
        if size == 0 || size > MAX_SIZE {
            return Err(AppError::bad_request(format!(
                "size 必须在 1 到 {MAX_SIZE} 之间"
            )));
        }

        let total = self.repository.count_records(&self.database).await?;
        let items = self
            .repository
            .list_records(&self.database, page, size)
            .await?;

        Ok(TransferRecordPage {
            page,
            size,
            total,
            items,
        })
    }
}

/// 事务内的全部读写：读账户 → 校验余额 → 更新双方余额 → 写流水与审计。
///
/// 运行在调用方开启的事务中；返回 `Err` 由调用方负责回滚。
async fn transfer_in_transaction<R: TransferRepository>(
    transaction: &DatabaseTransaction,
    repository: &R,
    command: &TransferCommand,
) -> Result<TransferReceipt, AppError> {
    let from = require_account(transaction, repository, &command.from_account_id).await?;
    let to = require_account(transaction, repository, &command.to_account_id).await?;

    let from_after = debit(&from, command.amount_cents)?;
    let to_after = credit(&to, command.amount_cents)?;
    repository
        .update_account_balance(transaction, &from_after)
        .await?;
    repository
        .update_account_balance(transaction, &to_after)
        .await?;

    let created_at = Local::now().timestamp_millis();
    let record_id = Uuid::now_v7().to_string();
    let record = build_record(
        record_id.clone(),
        command,
        &from_after,
        &to_after,
        created_at,
    );
    repository.insert_record(transaction, &record).await?;

    let changes = BalanceChanges {
        from_before: &from,
        from_after: &from_after,
        to_before: &to,
        to_after: &to_after,
    };
    let mut audits = balance_audits(&record_id, &changes, created_at);
    if command.force_fail {
        // 写完全部数据后再失败，用来证明回滚确实撤销了已写入的内容。
        audits.push(build_audit(
            &record_id,
            audit_action::ROLLBACK_VERIFIED,
            "调试开关触发：事务内写入将全部回滚",
            created_at,
        ));
    }

    for audit in &audits {
        repository.insert_audit(transaction, audit).await?;
    }

    if command.force_fail {
        return Err(AppError::internal(
            "force_fail 已开启：事务内写入全部完成后主动失败",
        ));
    }

    Ok(TransferReceipt {
        record_id,
        from_account_id: command.from_account_id.clone(),
        to_account_id: command.to_account_id.clone(),
        amount_cents: command.amount_cents,
        from_balance_after_cents: from_after.balance_cents,
        to_balance_after_cents: to_after.balance_cents,
        remark: command.remark.clone(),
        created_at,
        committed: true,
        rolled_back: false,
    })
}

/// 事务内读取账户，不存在时直接失败以触发回滚。
async fn require_account<R: TransferRepository>(
    transaction: &DatabaseTransaction,
    repository: &R,
    account_id: &str,
) -> Result<TransferAccountState, AppError> {
    repository
        .find_account(transaction, account_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("账户 {account_id} 不存在")))
}

fn validate(command: &TransferCommand) -> Result<(), AppError> {
    if command.from_account_id.trim().is_empty() || command.to_account_id.trim().is_empty() {
        return Err(AppError::bad_request("账户 ID 不能为空"));
    }
    if command.from_account_id == command.to_account_id {
        return Err(AppError::bad_request("转出账户与转入账户不能相同"));
    }
    if command.amount_cents <= 0 {
        return Err(AppError::bad_request("转账金额必须大于 0"));
    }

    Ok(())
}

/// 扣减余额并自增版本号；余额不足时返回业务错误，由调用方触发回滚。
fn debit(
    account: &TransferAccountState,
    amount_cents: i64,
) -> Result<TransferAccountState, AppError> {
    if account.balance_cents < amount_cents {
        return Err(AppError::bad_request(format!(
            "账户 {} 余额不足：可用 {} 分，需要 {amount_cents} 分",
            account.id, account.balance_cents
        )));
    }

    Ok(TransferAccountState {
        balance_cents: account.balance_cents - amount_cents,
        version: account.version + 1,
        ..account.clone()
    })
}

/// 增加余额并自增版本号；溢出时显式失败，不做静默截断。
fn credit(
    account: &TransferAccountState,
    amount_cents: i64,
) -> Result<TransferAccountState, AppError> {
    let balance_cents = account
        .balance_cents
        .checked_add(amount_cents)
        .ok_or_else(|| AppError::bad_request("转入后余额超出可表示范围"))?;

    Ok(TransferAccountState {
        balance_cents,
        version: account.version + 1,
        ..account.clone()
    })
}

fn build_record(
    record_id: String,
    command: &TransferCommand,
    from_after: &TransferAccountState,
    to_after: &TransferAccountState,
    created_at: i64,
) -> NewTransferRecord {
    NewTransferRecord {
        id: record_id,
        from_account_id: command.from_account_id.clone(),
        to_account_id: command.to_account_id.clone(),
        amount_cents: command.amount_cents,
        from_balance_after_cents: from_after.balance_cents,
        to_balance_after_cents: to_after.balance_cents,
        remark: command.remark.clone(),
        created_at,
    }
}

/// 一次转账前后的双方账户，用于生成审计明细。
struct BalanceChanges<'a> {
    from_before: &'a TransferAccountState,
    from_after: &'a TransferAccountState,
    to_before: &'a TransferAccountState,
    to_after: &'a TransferAccountState,
}

/// 一次事务内的两条审计记录：余额变更与流水创建。
fn balance_audits(
    record_id: &str,
    changes: &BalanceChanges<'_>,
    created_at: i64,
) -> Vec<NewTransferAudit> {
    vec![
        build_audit(
            record_id,
            audit_action::BALANCE_UPDATED,
            balance_detail(changes),
            created_at,
        ),
        build_audit(
            record_id,
            audit_action::RECORD_CREATED,
            "转账流水写入完成",
            created_at,
        ),
    ]
}

fn balance_detail(changes: &BalanceChanges<'_>) -> String {
    format!(
        "{} 余额 {} -> {}，{} 余额 {} -> {}",
        changes.from_before.id,
        changes.from_before.balance_cents,
        changes.from_after.balance_cents,
        changes.to_before.id,
        changes.to_before.balance_cents,
        changes.to_after.balance_cents
    )
}

fn build_audit(
    record_id: &str,
    action: &str,
    detail: impl Into<String>,
    created_at: i64,
) -> NewTransferAudit {
    NewTransferAudit {
        id: Uuid::now_v7().to_string(),
        record_id: record_id.to_string(),
        action: action.to_string(),
        detail: detail.into(),
        created_at,
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{Database, DatabaseConnection};

    use super::TransferService;
    use crate::infrastructure::repositories::transaction::SeaOrmTransferRepository as SeaOrmRepo;
    use crate::{
        domain::{
            error::AppError,
            models::transaction::{TransferCommand, audit_action},
            services::transaction::TransferUseCase,
        },
        entities::{transfer_account, transfer_record},
        infrastructure::{databases::schema, repositories::transaction::SeaOrmTransferRepository},
    };
    use sea_orm::EntityTrait;
    use std::sync::Arc;
    use uuid::Uuid;

    const ALICE: &str = "acc_alice";
    const BOB: &str = "acc_bob";
    const SEED_BALANCE: i64 = 100000;

    /// 每个用例独立建库并跑一遍建表与播种，互不共享状态。
    async fn setup() -> (TransferService<SeaOrmRepo>, DatabaseConnection) {
        let url = format!(
            "sqlite://{}?mode=rwc",
            std::env::temp_dir()
                .join(format!("axum-scaffold-transfer-test-{}.db", Uuid::now_v7()))
                .display()
        );
        let database = Database::connect(url).await.expect("连接临时数据库");
        schema::create_transfer_tables(&database)
            .await
            .expect("建表成功");
        schema::seed_transfer_accounts(&database)
            .await
            .expect("播种成功");

        let repository = Arc::new(SeaOrmTransferRepository);
        let service = TransferService::new(database.clone(), repository);

        (service, database)
    }

    fn command(from: &str, to: &str, amount_cents: i64) -> TransferCommand {
        TransferCommand {
            from_account_id: from.to_string(),
            to_account_id: to.to_string(),
            amount_cents,
            remark: Some("单测转账".to_string()),
            force_fail: false,
        }
    }

    async fn balance(database: &DatabaseConnection, account_id: &str) -> i64 {
        transfer_account::Entity::find_by_id(account_id.to_string())
            .one(database)
            .await
            .expect("查询账户")
            .expect("账户存在")
            .balance_cents
    }

    async fn account_version(database: &DatabaseConnection, account_id: &str) -> i64 {
        transfer_account::Entity::find_by_id(account_id.to_string())
            .one(database)
            .await
            .expect("查询账户")
            .expect("账户存在")
            .version
    }

    async fn record_count(database: &DatabaseConnection) -> usize {
        transfer_record::Entity::find()
            .all(database)
            .await
            .expect("查询流水")
            .len()
    }

    #[tokio::test]
    async fn transfer_commits_balances_record_and_audits() {
        let (service, database) = setup().await;

        let receipt = service
            .transfer(command(ALICE, BOB, 25000))
            .await
            .expect("转账成功");

        assert!(receipt.committed);
        assert!(!receipt.rolled_back);
        assert_eq!(receipt.from_balance_after_cents, SEED_BALANCE - 25000);
        assert_eq!(receipt.to_balance_after_cents, SEED_BALANCE + 25000);
        assert_eq!(balance(&database, ALICE).await, 75000);
        assert_eq!(balance(&database, BOB).await, 125000);
        assert_eq!(account_version(&database, ALICE).await, 1);
        assert_eq!(account_version(&database, BOB).await, 1);

        let detail = service
            .get_transfer(receipt.record_id.clone())
            .await
            .expect("查询流水详情");
        assert_eq!(detail.record.amount_cents, 25000);
        assert_eq!(detail.audits.len(), 2);
        assert_eq!(detail.audits[0].action, audit_action::BALANCE_UPDATED);
        assert_eq!(detail.audits[1].action, audit_action::RECORD_CREATED);
    }

    #[tokio::test]
    async fn rollback_keeps_database_unchanged_when_balance_is_insufficient() {
        let (service, database) = setup().await;

        let error = service
            .transfer(command(ALICE, BOB, SEED_BALANCE + 1))
            .await
            .expect_err("余额不足应当失败");

        assert!(matches!(error, AppError::BadRequest(_)));
        assert_eq!(balance(&database, ALICE).await, SEED_BALANCE);
        assert_eq!(balance(&database, BOB).await, SEED_BALANCE);
        assert_eq!(account_version(&database, ALICE).await, 0);
        assert_eq!(record_count(&database).await, 0);
    }

    #[tokio::test]
    async fn rollback_discards_all_writes_when_force_fail_is_on() {
        let (service, database) = setup().await;

        let mut failing = command(ALICE, BOB, 10000);
        failing.force_fail = true;
        let error = service.transfer(failing).await.expect_err("强制失败");

        assert!(matches!(error, AppError::Internal(_)));
        // 四次写操作（两个账户、一条流水、两条审计）全部被回滚。
        assert_eq!(balance(&database, ALICE).await, SEED_BALANCE);
        assert_eq!(balance(&database, BOB).await, SEED_BALANCE);
        assert_eq!(account_version(&database, ALICE).await, 0);
        assert_eq!(record_count(&database).await, 0);
    }

    #[tokio::test]
    async fn missing_account_is_reported_as_not_found() {
        let (service, database) = setup().await;

        let error = service
            .transfer(command(ALICE, "acc_missing", 100))
            .await
            .expect_err("账户不存在应当失败");

        assert!(matches!(error, AppError::NotFound(_)));
        assert_eq!(balance(&database, ALICE).await, SEED_BALANCE);
        assert_eq!(record_count(&database).await, 0);
    }

    #[tokio::test]
    async fn invalid_commands_are_rejected_before_transaction() {
        let (service, _database) = setup().await;

        let cases = [
            command("", BOB, 100),
            command(ALICE, ALICE, 100),
            command(ALICE, BOB, 0),
            command(ALICE, BOB, -1),
        ];

        for case in cases {
            let error = service.transfer(case).await.expect_err("非法参数应当失败");
            assert!(matches!(error, AppError::BadRequest(_)));
        }
    }

    #[tokio::test]
    async fn credit_overflow_is_rejected() {
        let (service, database) = setup().await;

        // 先把转入账户余额推到接近 i64 上限，再触发溢出分支。
        let account = transfer_account::Entity::find_by_id(BOB.to_string())
            .one(&database)
            .await
            .expect("查询账户")
            .expect("账户存在");
        use sea_orm::{ActiveModelTrait, Set};
        transfer_account::ActiveModel {
            id: Set(account.id),
            balance_cents: Set(i64::MAX),
            ..Default::default()
        }
        .update(&database)
        .await
        .expect("更新余额");

        let error = service
            .transfer(command(ALICE, BOB, 1))
            .await
            .expect_err("溢出应当失败");

        assert!(matches!(error, AppError::BadRequest(_)));
        assert_eq!(balance(&database, ALICE).await, SEED_BALANCE);
    }

    #[tokio::test]
    async fn list_transfers_paginates_newest_first() {
        let (service, _database) = setup().await;

        for amount in [1000, 2000, 3000] {
            service
                .transfer(command(ALICE, BOB, amount))
                .await
                .expect("转账成功");
        }

        let page = service
            .list_transfers(Some(1), Some(2))
            .await
            .expect("分页查询");
        assert_eq!(page.page, 1);
        assert_eq!(page.size, 2);
        assert_eq!(page.total, 3);
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0].amount_cents, 3000);

        let remaining = service
            .list_transfers(Some(2), Some(2))
            .await
            .expect("第二页");
        assert_eq!(remaining.items.len(), 1);
        assert_eq!(remaining.items[0].amount_cents, 1000);
    }

    #[tokio::test]
    async fn list_transfers_rejects_invalid_paging() {
        let (service, _database) = setup().await;

        assert!(matches!(
            service.list_transfers(Some(0), None).await,
            Err(AppError::BadRequest(_))
        ));
        assert!(matches!(
            service.list_transfers(None, Some(0)).await,
            Err(AppError::BadRequest(_))
        ));
        assert!(matches!(
            service.list_transfers(None, Some(101)).await,
            Err(AppError::BadRequest(_))
        ));
        // 不传分页参数时使用默认值：第 1 页、每页 10 条。
        let page = service.list_transfers(None, None).await.expect("默认分页");
        assert_eq!((page.page, page.size), (1, 10));
    }

    #[tokio::test]
    async fn get_transfer_rejects_empty_and_unknown_id() {
        let (service, _database) = setup().await;

        assert!(matches!(
            service.get_transfer("   ".to_string()).await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            service.get_transfer("missing-record".to_string()).await,
            Err(AppError::NotFound(_))
        ));
    }
}
