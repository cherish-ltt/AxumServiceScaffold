//! Axum 洋葱架构脚手架库入口。

pub mod api;
pub mod container;
pub mod create_app;
#[cfg(debug_assertions)]
pub mod docs;
pub mod domain;
pub mod entities;
pub mod error;
pub mod infrastructure;
pub mod logging;
pub mod response;
pub mod services;
pub mod util;
