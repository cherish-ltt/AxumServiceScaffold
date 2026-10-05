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
/// 3. 打印生效配置
/// 4. 构建全局状态
/// 5. 构建 Axum 路由
/// 6. 启动 HTTP 服务
#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    let config = AppConfig::from_env()?;
    let _log_guard = logging::init(&config)?;
    logging::log_startup_config(&config);

    let container = Arc::new(Container::bootstrap(config).await?);
    let app = create_app(container.clone());
    let address = container.config.server.socket_addr()?;

    let listener = tokio::net::TcpListener::bind(address).await?;
    info!(address = %address, "HTTP 服务启动成功");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("HTTP 服务已优雅关闭");
    if let Err(error) = container.database.close_by_ref().await {
        tracing::warn!(%error, "关闭数据库连接池失败");
    }
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("监听 Ctrl-C 信号失败");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("监听终止信号失败")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => info!("收到 Ctrl-C，开始优雅关闭"),
        _ = terminate => info!("收到终止信号，开始优雅关闭"),
    }
}
