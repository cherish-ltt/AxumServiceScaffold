use async_trait::async_trait;
use chrono::Local;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, Set,
};

use crate::{
    domain::models::transaction::{
        NewTransferAudit, NewTransferRecord, TransferAccountState, TransferAuditView,
        TransferRecordView,
    },
    entities::{transfer_account, transfer_audit, transfer_record},
    error::AppError,
};

/// 转账示例的仓储接口。
///
/// 每个方法都接收 `C: ConnectionTrait`：传入 `&DatabaseConnection` 表示不进事务，
/// 传入 `&DatabaseTransaction` 表示在调用方开启的事务内执行。方法本身不开启事务，
/// 事务边界由 `services/transaction.rs` 统一掌控。
#[async_trait]
pub trait TransferRepository: Send + Sync {
    async fn find_account<C>(
        &self,
        db: &C,
        id: &str,
    ) -> Result<Option<TransferAccountState>, AppError>
    where
        C: ConnectionTrait;

    async fn update_account_balance<C>(
        &self,
        db: &C,
        account: &TransferAccountState,
    ) -> Result<(), AppError>
    where
        C: ConnectionTrait;

    async fn insert_record<C>(&self, db: &C, record: &NewTransferRecord) -> Result<(), AppError>
    where
        C: ConnectionTrait;

    async fn insert_audit<C>(&self, db: &C, audit: &NewTransferAudit) -> Result<(), AppError>
    where
        C: ConnectionTrait;

    async fn find_record<C>(
        &self,
        db: &C,
        record_id: &str,
    ) -> Result<Option<TransferRecordView>, AppError>
    where
        C: ConnectionTrait;

    async fn list_audits<C>(
        &self,
        db: &C,
        record_id: &str,
    ) -> Result<Vec<TransferAuditView>, AppError>
    where
        C: ConnectionTrait;

    async fn count_records<C>(&self, db: &C) -> Result<u64, AppError>
    where
        C: ConnectionTrait;

    async fn list_records<C>(
        &self,
        db: &C,
        page: u64,
        size: u64,
    ) -> Result<Vec<TransferRecordView>, AppError>
    where
        C: ConnectionTrait;
}

/// 基于 SeaORM 的仓储实现。
#[derive(Debug, Default, Clone, Copy)]
pub struct SeaOrmTransferRepository;

#[async_trait]
impl TransferRepository for SeaOrmTransferRepository {
    async fn find_account<C>(
        &self,
        db: &C,
        id: &str,
    ) -> Result<Option<TransferAccountState>, AppError>
    where
        C: ConnectionTrait,
    {
        let account = transfer_account::Entity::find_by_id(id.to_string())
            .one(db)
            .await?;

        Ok(account.map(TransferAccountState::from))
    }

    async fn update_account_balance<C>(
        &self,
        db: &C,
        account: &TransferAccountState,
    ) -> Result<(), AppError>
    where
        C: ConnectionTrait,
    {
        let active = transfer_account::ActiveModel {
            id: Set(account.id.clone()),
            balance_cents: Set(account.balance_cents),
            version: Set(account.version),
            updated_at: Set(Local::now().timestamp_millis()),
            ..Default::default()
        };
        active.update(db).await?;

        Ok(())
    }

    async fn insert_record<C>(&self, db: &C, record: &NewTransferRecord) -> Result<(), AppError>
    where
        C: ConnectionTrait,
    {
        let active = transfer_record::ActiveModel {
            id: Set(record.id.clone()),
            from_account_id: Set(record.from_account_id.clone()),
            to_account_id: Set(record.to_account_id.clone()),
            amount_cents: Set(record.amount_cents),
            from_balance_after_cents: Set(record.from_balance_after_cents),
            to_balance_after_cents: Set(record.to_balance_after_cents),
            remark: Set(record.remark.clone()),
            created_at: Set(record.created_at),
        };
        active.insert(db).await?;

        Ok(())
    }

    async fn insert_audit<C>(&self, db: &C, audit: &NewTransferAudit) -> Result<(), AppError>
    where
        C: ConnectionTrait,
    {
        let active = transfer_audit::ActiveModel {
            id: Set(audit.id.clone()),
            record_id: Set(audit.record_id.clone()),
            action: Set(audit.action.clone()),
            detail: Set(audit.detail.clone()),
            created_at: Set(audit.created_at),
        };
        active.insert(db).await?;

        Ok(())
    }

    async fn find_record<C>(
        &self,
        db: &C,
        record_id: &str,
    ) -> Result<Option<TransferRecordView>, AppError>
    where
        C: ConnectionTrait,
    {
        let record = transfer_record::Entity::find_by_id(record_id.to_string())
            .one(db)
            .await?;

        Ok(record.map(TransferRecordView::from))
    }

    async fn list_audits<C>(
        &self,
        db: &C,
        record_id: &str,
    ) -> Result<Vec<TransferAuditView>, AppError>
    where
        C: ConnectionTrait,
    {
        let audits = transfer_audit::Entity::find()
            .filter(transfer_audit::Column::RecordId.eq(record_id))
            .order_by_asc(transfer_audit::Column::CreatedAt)
            .all(db)
            .await?;

        Ok(audits.into_iter().map(TransferAuditView::from).collect())
    }

    async fn count_records<C>(&self, db: &C) -> Result<u64, AppError>
    where
        C: ConnectionTrait,
    {
        let total = transfer_record::Entity::find().count(db).await?;

        Ok(total)
    }

    async fn list_records<C>(
        &self,
        db: &C,
        page: u64,
        size: u64,
    ) -> Result<Vec<TransferRecordView>, AppError>
    where
        C: ConnectionTrait,
    {
        let records = transfer_record::Entity::find()
            .order_by_desc(transfer_record::Column::CreatedAt)
            .offset((page - 1) * size)
            .limit(size)
            .all(db)
            .await?;

        Ok(records.into_iter().map(TransferRecordView::from).collect())
    }
}

impl From<transfer_account::Model> for TransferAccountState {
    fn from(model: transfer_account::Model) -> Self {
        Self {
            id: model.id,
            owner: model.owner,
            balance_cents: model.balance_cents,
            version: model.version,
        }
    }
}

impl From<transfer_record::Model> for TransferRecordView {
    fn from(model: transfer_record::Model) -> Self {
        Self {
            record_id: model.id,
            from_account_id: model.from_account_id,
            to_account_id: model.to_account_id,
            amount_cents: model.amount_cents,
            from_balance_after_cents: model.from_balance_after_cents,
            to_balance_after_cents: model.to_balance_after_cents,
            remark: model.remark,
            created_at: model.created_at,
        }
    }
}

impl From<transfer_audit::Model> for TransferAuditView {
    fn from(model: transfer_audit::Model) -> Self {
        Self {
            action: model.action,
            detail: model.detail,
            created_at: model.created_at,
        }
    }
}
