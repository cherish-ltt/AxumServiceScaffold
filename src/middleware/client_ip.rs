use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Request, Response, header},
};
use tower::{Layer, Service};

use crate::infrastructure::config::TrustedProxy;

/// 解析后的客户端 IP，写入请求扩展供访问日志使用。
#[derive(Debug, Clone, Copy)]
pub struct ClientIp(pub IpAddr);

/// 计算客户端来源 IP。
///
/// 只有直连对端本身落在可信代理网段内时才采信转发头，并且按信任链从右向左解析：
/// 右侧是最近的代理，遇到第一个不在可信网段的地址就是真实客户端。
/// 任意一段无法解析时回退到直连地址，不做「猜测」。
pub fn resolve_client_ip(
    peer: Option<SocketAddr>,
    trusted_proxies: &[TrustedProxy],
    headers: &HeaderMap,
) -> Option<IpAddr> {
    let peer = peer?;
    if trusted_proxies.is_empty() || !is_trusted(peer.ip(), trusted_proxies) {
        return Some(peer.ip());
    }

    if let Some(forwarded) = parse_x_forwarded_for(headers, trusted_proxies) {
        return Some(forwarded);
    }
    if let Some(real_ip) = parse_x_real_ip(headers) {
        return Some(real_ip);
    }

    Some(peer.ip())
}

fn is_trusted(ip: IpAddr, trusted_proxies: &[TrustedProxy]) -> bool {
    trusted_proxies.iter().any(|proxy| proxy.contains(ip))
}

/// 按「客户端, 代理1, 代理2 …」的顺序从右向左找第一个不可信地址。
fn parse_x_forwarded_for(headers: &HeaderMap, trusted_proxies: &[TrustedProxy]) -> Option<IpAddr> {
    let value = headers.get("x-forwarded-for")?.to_str().ok()?;

    for entry in value.split(',').map(str::trim).rev() {
        // 转发头可能带端口，也可能带 IPv6 方括号；解析失败即停止采信，退回直连地址。
        let candidate = match entry.parse::<IpAddr>() {
            Ok(ip) => ip,
            Err(_) => return None,
        };
        if !is_trusted(candidate, trusted_proxies) {
            return Some(candidate);
        }
    }

    None
}

fn parse_x_real_ip(headers: &HeaderMap) -> Option<IpAddr> {
    headers
        .get("x-real-ip")?
        .to_str()
        .ok()?
        .trim()
        .parse::<IpAddr>()
        .ok()
}

/// 把客户端 IP 放进请求扩展。
#[derive(Clone)]
pub struct ClientIpLayer {
    trusted_proxies: Arc<Vec<TrustedProxy>>,
}

impl ClientIpLayer {
    pub fn new(trusted_proxies: Vec<TrustedProxy>) -> Self {
        Self {
            trusted_proxies: Arc::new(trusted_proxies),
        }
    }
}

impl<S> Layer<S> for ClientIpLayer {
    type Service = ClientIpService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ClientIpService {
            inner,
            trusted_proxies: Arc::clone(&self.trusted_proxies),
        }
    }
}

/// 解析来源 IP 的中间件。
#[derive(Clone)]
pub struct ClientIpService<S> {
    inner: S,
    trusted_proxies: Arc<Vec<TrustedProxy>>,
}

impl<S> Service<Request<Body>> for ClientIpService<S>
where
    S: Service<Request<Body>, Response = Response<Body>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        let peer = req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0);
        if let Some(ip) = resolve_client_ip(peer, &self.trusted_proxies, req.headers()) {
            req.extensions_mut().insert(ClientIp(ip));
        }

        self.inner.call(req)
    }
}

/// 请求是否携带 Host / authority（供 Host 校验层复用同一套取值逻辑）。
pub fn request_authority(req: &Request<Body>) -> Option<String> {
    if let Some(authority) = req.uri().authority() {
        return Some(authority.as_str().to_ascii_lowercase());
    }

    req.headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proxies(entries: &[&str]) -> Vec<TrustedProxy> {
        entries
            .iter()
            .map(|entry| entry.parse::<TrustedProxy>().expect("解析可信代理"))
            .collect()
    }

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.append(
                axum::http::HeaderName::from_bytes(name.as_bytes()).expect("头名称"),
                value.parse().expect("头值"),
            );
        }

        headers
    }

    #[test]
    fn direct_peer_is_used_when_no_trusted_proxy_configured() {
        let peer = Some("203.0.113.9:1234".parse().expect("SocketAddr"));
        let spoofed = headers(&[("x-forwarded-for", "1.2.3.4")]);

        assert_eq!(
            resolve_client_ip(peer, &[], &spoofed),
            Some("203.0.113.9".parse().expect("IP"))
        );
    }

    #[test]
    fn spoofed_header_is_ignored_when_peer_is_not_trusted() {
        let peer = Some("203.0.113.9:1234".parse().expect("SocketAddr"));
        let spoofed = headers(&[("x-forwarded-for", "1.2.3.4")]);

        assert_eq!(
            resolve_client_ip(peer, &proxies(&["10.0.0.0/8"]), &spoofed),
            Some("203.0.113.9".parse().expect("IP"))
        );
    }

    #[test]
    fn trusted_proxy_chain_is_resolved_from_the_right() {
        let peer = Some("10.0.0.5:1234".parse().expect("SocketAddr"));
        let chain = headers(&[("x-forwarded-for", "1.2.3.4, 10.0.0.7")]);

        assert_eq!(
            resolve_client_ip(peer, &proxies(&["10.0.0.0/8"]), &chain),
            Some("1.2.3.4".parse().expect("IP"))
        );
    }

    #[test]
    fn client_supplied_leftmost_entry_is_not_trusted_blindly() {
        // 客户端伪造最左项时，真正的对端（10.0.0.7）左侧才是可信链的终点
        let peer = Some("10.0.0.7:1234".parse().expect("SocketAddr"));
        let spoofed = headers(&[("x-forwarded-for", "9.9.9.9, 203.0.113.9")]);

        assert_eq!(
            resolve_client_ip(peer, &proxies(&["10.0.0.0/8"]), &spoofed),
            Some("203.0.113.9".parse().expect("IP"))
        );
    }

    #[test]
    fn invalid_forwarded_value_falls_back_to_peer() {
        let peer = Some("10.0.0.5:1234".parse().expect("SocketAddr"));
        let broken = headers(&[("x-forwarded-for", "not-an-ip")]);

        assert_eq!(
            resolve_client_ip(peer, &proxies(&["10.0.0.0/8"]), &broken),
            Some("10.0.0.5".parse().expect("IP"))
        );
    }

    #[test]
    fn x_real_ip_is_used_when_forwarded_for_is_absent() {
        let peer = Some("10.0.0.5:1234".parse().expect("SocketAddr"));
        let real = headers(&[("x-real-ip", "198.51.100.7")]);

        assert_eq!(
            resolve_client_ip(peer, &proxies(&["10.0.0.0/8"]), &real),
            Some("198.51.100.7".parse().expect("IP"))
        );
    }

    #[test]
    fn missing_peer_yields_none() {
        assert_eq!(
            resolve_client_ip(None, &proxies(&["10.0.0.0/8"]), &HeaderMap::new()),
            None
        );
    }
}
