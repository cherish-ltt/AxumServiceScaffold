//! 连接集合：连接级许可、优雅关闭句柄与连接任务的生命周期管理。

use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::body::Body;
use hyper::{
    body::Incoming,
    http::{Request, Response},
};
use hyper_util::{
    rt::{TokioExecutor, TokioIo},
    server::{conn::auto, graceful::GracefulShutdown},
    service::TowerToHyperService,
};
use tokio::{
    net::TcpStream,
    sync::Semaphore,
    task::{JoinError, JoinSet},
};
use tower::BoxError;
use tracing::{debug, error, info, warn};

use crate::metrics;

/// 单个连接上的请求（`axum::serve` 内部也把 `Incoming` 转成 `Body`）。
pub type IncomingRequest = Request<Incoming>;

/// 连接集合。
///
/// 一次性持有三样东西：连接数许可（约束文件描述符占用，与请求级并发限制是两件事）、
/// 优雅关闭句柄（关闭时等待在途连接）与连接任务集合（超时后强制中止）。
pub struct ConnectionSet {
    permits: Arc<Semaphore>,
    graceful: GracefulShutdown,
    tasks: JoinSet<()>,
}

impl ConnectionSet {
    pub fn new(max_connections: usize) -> Self {
        Self {
            permits: Arc::new(Semaphore::new(max_connections)),
            graceful: GracefulShutdown::new(),
            tasks: JoinSet::new(),
        }
    }

    /// 是否已没有在途连接任务（accept 循环据此决定是否还有必要等待回收）。
    pub fn has_pending_tasks(&self) -> bool {
        !self.tasks.is_empty()
    }

    /// 接收一个新连接；超出连接上限时立即关闭并记录拒绝原因。
    pub fn accept<S>(
        &mut self,
        stream: TcpStream,
        remote_addr: SocketAddr,
        service: S,
        builder: &auto::Builder<TokioExecutor>,
    ) where
        S: tower::Service<IncomingRequest, Response = Response<Body>> + Clone + Send + 'static,
        S::Error: Into<BoxError> + Send + 'static,
        S::Future: Send + 'static,
    {
        let Ok(permit) = Arc::clone(&self.permits).try_acquire_owned() else {
            metrics::record_rejection("connection_limit");
            debug!(%remote_addr, "connection limit reached, closing new connection");
            drop(stream);

            return;
        };

        let hyper_service = TowerToHyperService::new(service);
        let builder = builder.clone();
        let watcher = self.graceful.watcher();

        self.tasks.spawn(async move {
            // 许可随任务结束、出错或取消而释放（OwnedSemaphorePermit）。
            let _permit = permit;
            let io = TokioIo::new(stream);
            let connection = builder.serve_connection_with_upgrades(io, hyper_service);
            if let Err(error) = watcher.watch(connection).await {
                debug!(%remote_addr, %error, "connection closed with error");
            }
        });
    }

    /// 等待任一连接任务结束；当前没有任务时该 future 永远 Pending（供 `select!` 使用）。
    pub fn join_next(&mut self) -> impl std::future::Future<Output = Option<JoinResult>> + '_ {
        self.tasks.join_next()
    }

    /// 等待在途连接排空，最多 `timeout`；超时后强制中止剩余任务。
    ///
    /// 排空超时不代表异常：宽限期只保证「尽力排空」，到期后剩余连接被强制中止，
    /// 客户端看到连接断开（因此 `SHUTDOWN_TIMEOUT_SECS` 必须小于编排层的终止宽限期）。
    pub async fn drain(self, timeout: Duration) {
        let Self {
            graceful,
            mut tasks,
            ..
        } = self;

        info!(
            in_flight = graceful.count(),
            "stopped accepting connections, draining in-flight connections"
        );
        if !crate::server::drain_connections(graceful_drain(graceful, &mut tasks), timeout).await {
            return;
        }

        force_abort(&mut tasks).await;
    }
}

/// 排空 future：优雅关闭连接 + 回收全部连接任务。
async fn graceful_drain(graceful: GracefulShutdown, tasks: &mut JoinSet<()>) {
    let reap = async {
        while let Some(result) = tasks.join_next().await {
            log_task_result(result, true);
        }
    };
    tokio::join!(graceful.shutdown(), reap);
}

/// 宽限期用尽：中止剩余连接任务，避免退出路径上残留后台任务。
async fn force_abort(tasks: &mut JoinSet<()>) {
    let aborted = tasks.len();
    tasks.shutdown().await;
    warn!(aborted, "forced shutdown of remaining connections");
}

/// 连接任务的结束结果。
pub type JoinResult = Result<(), JoinError>;

/// 连接任务 panic 只影响该连接，但必须被显式暴露出来，不能静默丢弃。
pub fn log_task_result(result: JoinResult, during_shutdown: bool) {
    let Err(error) = result else {
        return;
    };
    if !error.is_panic() {
        return;
    }
    let phase = if during_shutdown {
        "shutdown"
    } else {
        "serving"
    };
    error!(%error, phase, "connection task panicked");
}
