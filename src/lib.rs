//! 脚手架库入口。
//!
//! 这个库把通用 Web 服务所需的基础设施拆成独立模块，便于后续直接复用：
//!
//! - 配置加载
//! - 数据库连接
//! - JWT 鉴权
//! - 统一响应与错误
//! - Swagger 文档
//! - 示例业务模块

pub mod app;
pub mod auth;
pub mod config;
pub mod db;
#[cfg(debug_assertions)]
pub mod docs;
pub mod entities;
pub mod error;
pub mod logging;
pub mod modules;
pub mod response;
pub mod state;
