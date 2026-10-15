use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, Response, StatusCode},
};
use tower::{Layer, Service};

use crate::middleware::{client_ip::request_authority, error_response::Rejection};

/// Host / authority 白名单校验。
///
/// 匹配规则（大小写不敏感）：
/// - 白名单条目不含端口 → 只比较主机名，端口任意；
/// - 白名单条目含端口 → 主机名与端口都要一致。
#[derive(Clone)]
pub struct HostCheckLayer {
    allowed: Arc<Vec<String>>,
}

impl HostCheckLayer {
    pub fn new(allowed: Vec<String>) -> Self {
        Self {
            allowed: allowed
                .into_iter()
                .map(|entry| entry.to_ascii_lowercase())
                .collect::<Vec<_>>()
                .into(),
        }
    }
}

/// 白名单匹配：主机名大小写不敏感，条目不含端口时匹配任意端口。
fn authority_allowed(allowed: &[String], authority: &str) -> bool {
    let (host, port) = split_authority(authority);

    allowed.iter().any(|entry| {
        let (entry_host, entry_port) = split_authority(entry);

        entry_host.eq_ignore_ascii_case(host)
            && entry_port.is_none_or(|entry_port| Some(entry_port) == port)
    })
}

/// 拆分 host 与端口；同时处理 `[::1]:8080` 形式的 IPv6 字面量。
fn split_authority(authority: &str) -> (&str, Option<&str>) {
    let trimmed = authority.trim();
    if let Some(rest) = trimmed.strip_prefix('[') {
        return match rest.split_once(']') {
            Some((host, tail)) => (host, tail.strip_prefix(':').filter(|port| !port.is_empty())),
            None => (trimmed, None),
        };
    }

    match trimmed.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            (host, Some(port))
        },
        _ => (trimmed, None),
    }
}

impl<S> Layer<S> for HostCheckLayer {
    type Service = HostCheckService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        HostCheckService {
            inner,
            allowed: Arc::clone(&self.allowed),
        }
    }
}

/// Host 校验中间件。
#[derive(Clone)]
pub struct HostCheckService<S> {
    inner: S,
    allowed: Arc<Vec<String>>,
}

impl<S> Service<Request<Body>> for HostCheckService<S>
where
    S: Service<Request<Body>, Response = Response<Body>>,
    S::Future: Send + 'static,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        // HTTP/1.1 的 authority 语义由 Host 头承载（hyper 不会把 Host 写进 URI），
        // HTTP/2 与 absolute-form 请求则来自 URI。
        let authority = request_authority(&req);
        let allowed = match &authority {
            None => Err(Rejection::new(
                StatusCode::BAD_REQUEST,
                "missing Host header",
                "host_missing",
            )),
            Some(authority) if authority_allowed(&self.allowed, authority) => Ok(()),
            Some(_) => Err(Rejection::new(
                StatusCode::MISDIRECTED_REQUEST,
                "host is not served by this instance",
                "host_not_allowed",
            )),
        };

        if let Err(rejection) = allowed {
            return Box::pin(async move { Ok(rejection.into_response()) });
        }

        let fut = self.inner.call(req);

        Box::pin(fut)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer_allows(layer: &HostCheckLayer, authority: &str) -> bool {
        authority_allowed(&layer.allowed, authority)
    }

    #[test]
    fn bare_host_matches_any_port() {
        let layer = HostCheckLayer::new(vec!["api.example.com".to_string()]);

        assert!(layer_allows(&layer, "api.example.com"));
        assert!(layer_allows(&layer, "api.example.com:8443"));
        assert!(layer_allows(&layer, "API.Example.COM:8080"));
        assert!(!layer_allows(&layer, "evil.example.com"));
    }

    #[test]
    fn host_with_port_requires_matching_port() {
        let layer = HostCheckLayer::new(vec!["api.example.com:8443".to_string()]);

        assert!(layer_allows(&layer, "api.example.com:8443"));
        assert!(!layer_allows(&layer, "api.example.com"));
        assert!(!layer_allows(&layer, "api.example.com:80"));
    }

    #[test]
    fn ipv6_authority_with_port_is_supported() {
        let layer = HostCheckLayer::new(vec!["[::1]:8080".to_string()]);

        assert!(layer_allows(&layer, "[::1]:8080"));
        assert!(!layer_allows(&layer, "[::1]:9090"));
    }
}
