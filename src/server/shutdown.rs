use std::time::Duration;

use tracing::{info, warn};

/// 等待进程终止信号（Ctrl-C 或 SIGTERM），返回触发的信号名。
///
/// 两个信号各自跑在独立任务里，共同向通道上报；这样终止路径只有一个 `.await` 点，
/// 不需要把信号处理塞进 `select!`。
pub async fn shutdown_signal() -> &'static str {
    let (sender, mut receiver) = tokio::sync::mpsc::channel::<&'static str>(1);
    let ctrl_c_sender = sender.clone();

    tokio::spawn(async move {
        let _ = ctrl_c().await;
        let _ = ctrl_c_sender.send("SIGINT").await;
    });
    tokio::spawn(async move {
        terminate().await;
        let _ = sender.send("SIGTERM").await;
    });

    let signal = receiver.recv().await.unwrap_or("termination signal");
    info!(
        signal,
        "received termination signal, starting graceful shutdown"
    );

    signal
}

async fn ctrl_c() -> std::io::Result<()> {
    tokio::signal::ctrl_c().await
}

/// SIGTERM（编排系统下发的终止信号）。非 Unix 平台没有该信号，永不触发。
async fn terminate() {
    #[cfg(unix)]
    {
        let mut stream = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to listen for termination signal");
        stream.recv().await;
    }

    #[cfg(not(unix))]
    std::future::pending::<()>().await;
}

/// 等待在途连接排空，最多等待 `timeout`。
///
/// 返回 `true` 表示超时：调用方必须随后**强制中止**剩余连接任务。
/// 宽限期只保证「尽力排空」，不保证所有在途请求都能完成；到期后剩余连接的客户端会看到
/// 连接断开。因此 `SHUTDOWN_TIMEOUT_SECS` 必须小于容器编排的
/// `terminationGracePeriodSeconds`，否则进程会被 SIGKILL 硬杀，连最后的日志落盘都没有机会。
pub async fn drain_connections<F>(in_flight: F, timeout: Duration) -> bool
where
    F: std::future::Future<Output = ()>,
{
    match tokio::time::timeout(timeout, in_flight).await {
        Ok(()) => false,
        Err(_elapsed) => {
            warn!(
                timeout_secs = timeout.as_secs(),
                "shutdown timeout reached, forcing remaining connections to stop"
            );

            true
        },
    }
}
