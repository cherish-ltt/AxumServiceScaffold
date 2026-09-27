//! 基础设施层的仓储适配器目录。
//!
//! 仓储接口与 SeaORM 实现都放在这里：接口签名需要引用 `sea_orm::ConnectionTrait`，
//! 放在 `domain` 会破坏「领域层不依赖 sea-orm」的边界。
//! 实现通过 `container.rs` 注入到 services 层。

pub mod transaction;
