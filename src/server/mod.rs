//! HTTP 服务接入层：自定义 accept 循环 + 连接层防护 + 优雅关闭。
//!
//! 为什么不用 `axum::serve`：它只用 `hyper_util::server::conn::auto::Builder::new(...)`
//! 且从不设置 `Timer`，导致 hyper 的请求头读取超时永远不生效（慢速发头可无限占用连接），
//! 也无法设置请求头数量/缓冲上限与并发连接上限。这里显式接管连接构建器，
//! 并把连接任务集中管理，以便关闭时限定等待时间。

use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use axum::{
    Router, body::Body, extract::connect_info::IntoMakeServiceWithConnectInfo, http::Request,
};
use hyper::body::Incoming;
use hyper_util::{rt::TokioExecutor, server::conn::auto};
use tokio::net::TcpListener;
use tower::{Service as _, ServiceExt as _};
use tracing::{info, warn};

use crate::{
    infrastructure::config::AppConfig,
    server::{
        connection::http1_builder,
        connections::{ConnectionSet, log_task_result},
        shutdown::drain_connections,
    },
};

mod connection;
mod connections;
mod shutdown;

pub use shutdown::shutdown_signal;

/// 逐连接的请求服务工厂（`connect_info` 版本）。
type MakeService = IntoMakeServiceWithConnectInfo<Router, SocketAddr>;

/// 已绑定端口的 HTTP 服务。
pub struct Server {
    listener: TcpListener,
    app: Router,
    config: Arc<AppConfig>,
}

impl Server {
    /// 绑定监听端口（此时还未开始接收连接）。
    pub async fn bind(config: Arc<AppConfig>, app: Router) -> Result<Self> {
        let address = config.server.socket_addr()?;
        let listener = TcpListener::bind(address)
            .await
            .with_context(|| format!("failed to bind HTTP listener on {address}"))?;

        Ok(Self {
            listener,
            app,
            config,
        })
    }

    /// 实际监听的地址（端口为 0 时由内核分配）。
    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.listener
            .local_addr()
            .context("failed to read the listening address")
    }

    /// 运行服务，直到内置的终止信号到达。
    pub async fn run_until_signal(self) -> Result<()> {
        self.run(async {
            shutdown_signal().await;
        })
        .await
    }

    /// 运行服务，直到 `shutdown` 完成；随后按 `SHUTDOWN_TIMEOUT_SECS` 排空在途连接。
    pub async fn run(self, shutdown: impl std::future::Future<Output = ()>) -> Result<()> {
        let Self {
            listener,
            app,
            config,
        } = self;
        let server_config = &config.server;
        let builder = http1_builder(server_config);
        let mut make_service = app.into_make_service_with_connect_info::<SocketAddr>();
        let mut connections = ConnectionSet::new(server_config.max_connections);

        info!(
            max_connections = server_config.max_connections,
            shutdown_timeout_secs = server_config.shutdown_timeout_secs,
            "connection layer protection enabled"
        );

        accept_until_shutdown(
            &listener,
            &mut make_service,
            &builder,
            &mut connections,
            shutdown,
        )
        .await;
        connections
            .drain(Duration::from_secs(server_config.shutdown_timeout_secs))
            .await;

        Ok(())
    }
}

/// 反复接受连接，直到 `shutdown` 完成。
///
/// 单个连接的错误（EMFILE、对端重置等）不得终止 accept 循环；连接数上限由
/// `ConnectionSet::accept` 内部的许可控制，超限连接被立即关闭。
async fn accept_until_shutdown(
    listener: &TcpListener,
    make_service: &mut MakeService,
    builder: &auto::Builder<TokioExecutor>,
    connections: &mut ConnectionSet,
    shutdown: impl std::future::Future<Output = ()>,
) {
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            accepted = listener.accept() => {
                let (stream, remote_addr) = match accepted {
                    Ok(pair) => pair,
                    Err(error) => {
                        warn!(%error, "failed to accept connection");
                        continue;
                    }
                };

                let service = make_service
                    .call(remote_addr)
                    .await
                    .expect("connect-info make service is infallible")
                    .map_request(|request: Request<Incoming>| request.map(Body::new));
                connections.accept(stream, remote_addr, service, builder);
            }
            Some(result) = connections.join_next(), if connections.has_pending_tasks() => {
                log_task_result(result, false);
            }
        }
    }
}
