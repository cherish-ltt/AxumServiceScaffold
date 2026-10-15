use anyhow::Result;
use axum_service_scaffold::{
    container::Container, create_app::create_app, infrastructure::config::AppConfig, logging,
    server::Server,
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
/// 1. 读取环境变量（含生产环境安全门控校验）
/// 2. 初始化日志
/// 3. 打印生效配置
/// 4. 构建全局状态（含指标出口）
/// 5. 构建 Axum 路由
/// 6. 启动 HTTP 服务（自定义 accept 循环，含连接层防护与关闭期限）
#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    let config = AppConfig::from_env()?;
    let _log_guard = logging::init(&config)?;
    logging::log_startup_config(&config);

    let container = Arc::new(Container::bootstrap(config).await?);
    let app = create_app(Arc::clone(&container));
    let server = Server::bind(Arc::clone(&container.config), app).await?;
    info!(
        address = %server.local_addr()?,
        "HTTP server started successfully"
    );

    server.run_until_signal().await?;

    info!("HTTP server shut down gracefully");
    if let Err(error) = container.database.close_by_ref().await {
        tracing::warn!(%error, "failed to close the database connection pool");
    }

    Ok(())
}
