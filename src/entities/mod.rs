//! SeaORM 实体目录。
//!
//! 洋葱架构里，实体集中放在这里，由基础设施层的仓储适配器使用。
//! 每个实体单独一个文件，也可以通过 `sea-orm-cli generate entity` 生成到该目录下。

pub mod transfer_account;
pub mod transfer_audit;
pub mod transfer_record;
