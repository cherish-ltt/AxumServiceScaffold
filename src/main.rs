use anyhow::Result;
use axum_service_scaffold::{
    container::Container, create_app::create_app, infrastructure::config::AppConfig, logging,
};
use dotenv::dotenv;
use mimalloc::MiMalloc;
use std::sync::Arc;
use tracing::info;

/// 全局内存分配器。
///
/// 对于长期运行的 Web 服务，统一分配器有助于保持行为稳定。
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

/// 程序启动入口。
///
/// 启动流程固定为：
///
/// 1. 读取环境变量
/// 2. 初始化日志
/// 3. 构建全局状态
/// 4. 构建 Axum 路由
/// 5. 启动 HTTP 服务
#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    let config = AppConfig::from_env()?;
    logging::init(&config);

    let container = Arc::new(Container::bootstrap(config).await?);
    let app = create_app(container.clone());
    let address = container.config.server.socket_addr()?;

    let listener = tokio::net::TcpListener::bind(address).await?;
    info!(address = %address, "HTTP 服务启动成功");

    axum::serve(listener, app).await?;

    Ok(())
}
