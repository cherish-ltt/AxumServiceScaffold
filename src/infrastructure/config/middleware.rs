use std::{net::IpAddr, str::FromStr};

use anyhow::{Result, anyhow};
use axum::http::HeaderValue;

use super::env::{get_env_or, parse_env_or, parse_items, parse_list};

/// HSTS 响应头参数。
///
/// 仅在外部已正确部署 HTTPS 时启用；`include_subdomains` / `preload` 会影响
/// 整个域名及其全部子域，默认都关闭，避免把未启用 HTTPS 的子域一起打挂。
#[derive(Debug, Clone, Copy)]
pub struct HstsConfig {
    pub enabled: bool,
    pub max_age_secs: u64,
    pub include_subdomains: bool,
    pub preload: bool,
}

impl HstsConfig {
    /// preload 的前置条件（Chromium HSTS preload 提交要求）。
    const PRELOAD_MIN_MAX_AGE: u64 = 31_536_000;

    /// 按当前配置拼出 HSTS 响应头值。
    pub fn header_value(&self) -> String {
        let mut value = format!("max-age={}", self.max_age_secs);
        if self.include_subdomains {
            value.push_str("; includeSubDomains");
        }
        if self.preload {
            value.push_str("; preload");
        }

        value
    }

    fn validate(&self) -> Result<()> {
        if !self.enabled {
            if self.include_subdomains || self.preload {
                return Err(anyhow!(
                    "MIDDLEWARE_HSTS_INCLUDE_SUBDOMAINS / MIDDLEWARE_HSTS_PRELOAD require MIDDLEWARE_HSTS_ENABLED=true"
                ));
            }
            return Ok(());
        }
        if self.max_age_secs == 0 {
            return Err(anyhow!(
                "MIDDLEWARE_HSTS_MAX_AGE_SECS must be greater than 0"
            ));
        }
        if self.preload
            && (!self.include_subdomains || self.max_age_secs < Self::PRELOAD_MIN_MAX_AGE)
        {
            return Err(anyhow!(
                "MIDDLEWARE_HSTS_PRELOAD requires includeSubDomains and max-age >= {}",
                Self::PRELOAD_MIN_MAX_AGE
            ));
        }

        Ok(())
    }
}

/// 单个中间件的启停开关。
///
/// 安全关键项（请求体限制、请求超时、并发上限、背压、限流、安全响应头、request_id）
/// 在生产环境不允许关闭，除非显式设置 `MIDDLEWARE_ALLOW_INSECURE_PRODUCTION=true`。
#[derive(Debug, Clone, Copy)]
pub struct MiddlewareSwitches {
    pub request_id: bool,
    pub access_log: bool,
    pub compression: bool,
    pub security_headers: bool,
    pub timeout: bool,
    pub body_limit: bool,
    pub concurrency_limit: bool,
    pub backpressure: bool,
    pub rate_limit: bool,
}

impl MiddlewareSwitches {
    /// 生产环境必须保持开启的开关（名称用于报错信息）。
    pub fn disabled_required_in_production(&self, production: bool) -> Vec<&'static str> {
        if !production {
            return Vec::new();
        }
        let all = [
            ("MIDDLEWARE_REQUEST_ID_ENABLED", self.request_id),
            ("MIDDLEWARE_SECURITY_HEADERS_ENABLED", self.security_headers),
            ("MIDDLEWARE_TIMEOUT_ENABLED", self.timeout),
            ("MIDDLEWARE_BODY_LIMIT_ENABLED", self.body_limit),
            (
                "MIDDLEWARE_CONCURRENCY_LIMIT_ENABLED",
                self.concurrency_limit,
            ),
            ("MIDDLEWARE_BACKPRESSURE_ENABLED", self.backpressure),
            ("MIDDLEWARE_RATE_LIMIT_ENABLED", self.rate_limit),
        ];

        all.into_iter()
            .filter(|(_, enabled)| !enabled)
            .map(|(name, _)| name)
            .collect()
    }

    fn from_env() -> Result<Self> {
        Ok(Self {
            request_id: parse_env_or("MIDDLEWARE_REQUEST_ID_ENABLED", true)?,
            access_log: parse_env_or("MIDDLEWARE_ACCESS_LOG_ENABLED", true)?,
            compression: parse_env_or("MIDDLEWARE_COMPRESSION_ENABLED", true)?,
            security_headers: parse_env_or("MIDDLEWARE_SECURITY_HEADERS_ENABLED", true)?,
            timeout: parse_env_or("MIDDLEWARE_TIMEOUT_ENABLED", true)?,
            body_limit: parse_env_or("MIDDLEWARE_BODY_LIMIT_ENABLED", true)?,
            concurrency_limit: parse_env_or("MIDDLEWARE_CONCURRENCY_LIMIT_ENABLED", true)?,
            backpressure: parse_env_or("MIDDLEWARE_BACKPRESSURE_ENABLED", true)?,
            rate_limit: parse_env_or("MIDDLEWARE_RATE_LIMIT_ENABLED", true)?,
        })
    }
}

/// 可信反向代理的地址段（CIDR）。
///
/// 只有直连对端落在这些网段内时，才采信 `X-Forwarded-For` / `X-Real-IP`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrustedProxy {
    network: IpAddr,
    prefix: u8,
}

impl FromStr for TrustedProxy {
    type Err = anyhow::Error;

    fn from_str(raw: &str) -> Result<Self> {
        let (addr, prefix) = match raw.split_once('/') {
            Some((addr, prefix)) => (addr, Some(prefix)),
            None => (raw, None),
        };
        let network = addr.parse::<IpAddr>().map_err(|error| anyhow!("{error}"))?;
        let default_prefix = match network {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        let prefix = match prefix {
            Some(value) => value.parse::<u8>().map_err(|error| anyhow!("{error}"))?,
            None => default_prefix,
        };
        let max_prefix = match network {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        if prefix > max_prefix {
            return Err(anyhow!("prefix {prefix} exceeds {max_prefix}"));
        }

        Ok(Self { network, prefix })
    }
}

impl TrustedProxy {
    pub fn contains(&self, candidate: IpAddr) -> bool {
        match (self.network, candidate) {
            (IpAddr::V4(network), IpAddr::V4(candidate)) => {
                prefix_matches(&network.octets(), &candidate.octets(), self.prefix)
            },
            // IPv4 网段与 IPv4-mapped IPv6 对端做归一化比较，
            // 避免双栈监听下同一来源被判成不可信。
            (IpAddr::V4(network), IpAddr::V6(candidate)) => match candidate.to_ipv4_mapped() {
                Some(candidate) => {
                    prefix_matches(&network.octets(), &candidate.octets(), self.prefix)
                },
                None => false,
            },
            (IpAddr::V6(network), IpAddr::V6(candidate)) => {
                prefix_matches(&network.octets(), &candidate.octets(), self.prefix)
            },
            (IpAddr::V6(_), IpAddr::V4(_)) => false,
        }
    }
}

fn prefix_matches(network: &[u8], candidate: &[u8], prefix: u8) -> bool {
    let full_bytes = usize::from(prefix / 8);
    let remaining_bits = prefix % 8;
    if network[..full_bytes] != candidate[..full_bytes] {
        return false;
    }
    if remaining_bits == 0 {
        return true;
    }
    let mask = 0xffu8 << (8 - remaining_bits);

    network[full_bytes] & mask == candidate[full_bytes] & mask
}

/// DNS 名称（或 `host`）形式的允许值，用于 Host 白名单校验。
fn validate_host_entry(entry: &str) -> Result<()> {
    if entry.contains('/') || entry.contains(' ') {
        return Err(anyhow!("host must not contain '/' or spaces"));
    }
    if entry == "*" {
        return Err(anyhow!(
            "wildcard host is not allowed: list every served host explicitly"
        ));
    }
    HeaderValue::from_str(entry).map_err(|error| anyhow!("{error}"))?;

    Ok(())
}

/// HTTP 中间件容量参数与开关，全部集中在 `.env`，业务代码不得内联这些数字。
#[derive(Debug, Clone)]
pub struct MiddlewareConfig {
    /// 单个请求从进入服务到响应完成的整体超时（秒）。
    pub request_timeout_secs: u64,
    /// 背压队列中的最长等待时间（秒），超出返回 503。
    pub queue_wait_timeout_secs: u64,
    /// 请求体最大字节数。
    pub max_body_bytes: usize,
    /// 请求 URI（路径 + 查询串）的最大字节数，超出返回 414。
    pub max_uri_bytes: usize,
    /// 同时进入 handler 的最大请求数。
    pub max_concurrency: usize,
    /// 并发已满时允许排队等待的请求数，超出部分快速拒绝。
    pub backpressure_queue: usize,
    /// 全局限流窗口内允许的请求数。
    pub rate_limit_requests: u64,
    /// 全局限流窗口长度（秒）。
    pub rate_limit_period_secs: u64,
    /// HSTS 响应头配置。
    pub hsts: HstsConfig,
    /// 允许跨域访问的来源白名单（如 `https://a.example`）。
    ///
    /// 空列表表示不下发任何 CORS 放行头；单元素为 `*` 表示放行所有来源。
    /// 条目在 `AppConfig::from_env` 阶段校验为合法 header 值。
    pub cors_allowed_origins: Vec<String>,
    /// 允许的 Host / authority 列表；为空表示不校验。
    pub allowed_hosts: Vec<String>,
    /// 可信反向代理网段；为空表示不采信任何转发头。
    pub trusted_proxies: Vec<TrustedProxy>,
    /// 各中间件的启停开关。
    pub switches: MiddlewareSwitches,
}

impl MiddlewareConfig {
    pub fn from_env(app_env: &str, allow_insecure_middleware: bool) -> Result<Self> {
        let production = app_env.eq_ignore_ascii_case("production");
        let hsts_enabled = parse_env_or("MIDDLEWARE_HSTS_ENABLED", production)?;
        let hsts = HstsConfig {
            enabled: hsts_enabled,
            max_age_secs: parse_env_or("MIDDLEWARE_HSTS_MAX_AGE_SECS", 31_536_000_u64)?,
            include_subdomains: parse_env_or("MIDDLEWARE_HSTS_INCLUDE_SUBDOMAINS", false)?,
            preload: parse_env_or("MIDDLEWARE_HSTS_PRELOAD", false)?,
        };

        let config = Self {
            request_timeout_secs: parse_env_or("MIDDLEWARE_REQUEST_TIMEOUT_SECS", 10_u64)?,
            queue_wait_timeout_secs: parse_env_or("MIDDLEWARE_QUEUE_WAIT_TIMEOUT_SECS", 5_u64)?,
            max_body_bytes: parse_env_or("MIDDLEWARE_MAX_BODY_BYTES", 2 * 1024 * 1024_usize)?,
            max_uri_bytes: parse_env_or("MIDDLEWARE_MAX_URI_BYTES", 8 * 1024_usize)?,
            max_concurrency: parse_env_or("MIDDLEWARE_MAX_CONCURRENCY", 256_usize)?,
            backpressure_queue: parse_env_or("MIDDLEWARE_BACKPRESSURE_QUEUE", 256_usize)?,
            rate_limit_requests: parse_env_or("MIDDLEWARE_RATE_LIMIT_REQUESTS", 32768_u64)?,
            rate_limit_period_secs: parse_env_or("MIDDLEWARE_RATE_LIMIT_PERIOD_SECS", 1_u64)?,
            hsts,
            cors_allowed_origins: parse_list(
                &get_env_or("MIDDLEWARE_CORS_ALLOWED_ORIGINS", ""),
                "MIDDLEWARE_CORS_ALLOWED_ORIGINS",
                |origin| {
                    HeaderValue::from_str(origin).map_err(|error| anyhow!("{error}"))?;
                    Ok(())
                },
            )?,
            allowed_hosts: parse_list(
                &get_env_or("HTTP_ALLOWED_HOSTS", ""),
                "HTTP_ALLOWED_HOSTS",
                validate_host_entry,
            )?,
            trusted_proxies: parse_items(
                &get_env_or("MIDDLEWARE_TRUSTED_PROXIES", ""),
                "MIDDLEWARE_TRUSTED_PROXIES",
            )?,
            switches: MiddlewareSwitches::from_env()?,
        };
        config.validate(production, allow_insecure_middleware)?;

        Ok(config)
    }

    /// 队列等待时间必须短于整体请求超时，否则排队请求永远先被 408 掐断，
    /// 独立的「队列等待超时」就没有意义。
    pub fn queue_wait_effective_secs(&self) -> u64 {
        self.queue_wait_timeout_secs.min(self.request_timeout_secs)
    }

    fn validate(&self, production: bool, allow_insecure_middleware: bool) -> Result<()> {
        if self.request_timeout_secs == 0
            || self.max_body_bytes == 0
            || self.max_concurrency == 0
            || self.backpressure_queue == 0
            || self.rate_limit_requests == 0
            || self.rate_limit_period_secs == 0
        {
            return Err(anyhow!(
                "MIDDLEWARE capacity parameters must be greater than 0"
            ));
        }
        if self.queue_wait_timeout_secs == 0 {
            return Err(anyhow!(
                "MIDDLEWARE_QUEUE_WAIT_TIMEOUT_SECS must be greater than 0"
            ));
        }
        if self.max_uri_bytes == 0 {
            return Err(anyhow!("MIDDLEWARE_MAX_URI_BYTES must be greater than 0"));
        }
        if self.queue_wait_timeout_secs > self.request_timeout_secs {
            return Err(anyhow!(
                "MIDDLEWARE_QUEUE_WAIT_TIMEOUT_SECS must not exceed MIDDLEWARE_REQUEST_TIMEOUT_SECS"
            ));
        }
        self.hsts.validate()?;

        if production && !allow_insecure_middleware {
            let disabled = self.switches.disabled_required_in_production(production);
            if !disabled.is_empty() {
                return Err(anyhow!(
                    "required middleware disabled in production: {} (set MIDDLEWARE_ALLOW_INSECURE_PRODUCTION=true only if an upstream proxy provides the protection)",
                    disabled.join(", ")
                ));
            }
            if self.allowed_hosts.is_empty() {
                return Err(anyhow!(
                    "HTTP_ALLOWED_HOSTS must list every served host in production"
                ));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidr_matches_ipv4_and_ipv6() {
        let block: TrustedProxy = "10.0.0.0/8".parse().expect("解析 CIDR");
        assert!(block.contains("10.1.2.3".parse::<IpAddr>().expect("IPv4")));
        assert!(!block.contains("11.1.2.3".parse::<IpAddr>().expect("IPv4")));

        let single: TrustedProxy = "192.168.1.7".parse().expect("解析单地址");
        assert!(single.contains("192.168.1.7".parse::<IpAddr>().expect("IPv4")));
        assert!(!single.contains("192.168.1.8".parse::<IpAddr>().expect("IPv4")));

        let v6: TrustedProxy = "2001:db8::/32".parse().expect("解析 IPv6 CIDR");
        assert!(v6.contains("2001:db8::1".parse::<IpAddr>().expect("IPv6")));
        assert!(!v6.contains("2001:db9::1".parse::<IpAddr>().expect("IPv6")));

        // 非整字节前缀
        let odd: TrustedProxy = "10.0.0.0/12".parse().expect("解析 /12");
        assert!(odd.contains("10.15.255.255".parse::<IpAddr>().expect("IPv4")));
        assert!(!odd.contains("10.16.0.1".parse::<IpAddr>().expect("IPv4")));
    }

    #[test]
    fn ipv4_mapped_ipv6_peer_matches_ipv4_block() {
        let block: TrustedProxy = "127.0.0.0/8".parse().expect("解析 CIDR");
        let mapped = "::ffff:127.0.0.1".parse::<IpAddr>().expect("IPv4-mapped");

        assert!(block.contains(mapped));
    }

    #[test]
    fn invalid_cidr_is_rejected() {
        assert!("10.0.0.0/33".parse::<TrustedProxy>().is_err());
        assert!("not-an-ip".parse::<TrustedProxy>().is_err());
        assert!("2001:db8::/129".parse::<TrustedProxy>().is_err());
    }

    #[test]
    fn hsts_header_follows_flags() {
        let base = HstsConfig {
            enabled: true,
            max_age_secs: 60,
            include_subdomains: false,
            preload: false,
        };
        assert_eq!(base.header_value(), "max-age=60");

        let full = HstsConfig {
            include_subdomains: true,
            preload: true,
            max_age_secs: 31_536_000,
            ..base
        };
        assert_eq!(
            full.header_value(),
            "max-age=31536000; includeSubDomains; preload"
        );
        assert!(full.validate().is_ok());
    }

    #[test]
    fn hsts_rejects_unsafe_combinations() {
        let preload_without_subdomains = HstsConfig {
            enabled: true,
            max_age_secs: 31_536_000,
            include_subdomains: false,
            preload: true,
        };
        assert!(preload_without_subdomains.validate().is_err());

        let preload_with_short_age = HstsConfig {
            enabled: true,
            max_age_secs: 60,
            include_subdomains: true,
            preload: true,
        };
        assert!(preload_with_short_age.validate().is_err());

        let disabled_but_flagged = HstsConfig {
            enabled: false,
            max_age_secs: 31_536_000,
            include_subdomains: true,
            preload: false,
        };
        assert!(disabled_but_flagged.validate().is_err());
    }

    #[test]
    fn wildcard_host_is_rejected() {
        assert!(validate_host_entry("*").is_err());
        assert!(validate_host_entry("a/b").is_err());
        assert!(validate_host_entry("api.example.com").is_ok());
        assert!(validate_host_entry("api.example.com:8443").is_ok());
    }

    #[test]
    fn production_requires_security_switches() {
        let switches = MiddlewareSwitches {
            request_id: true,
            access_log: true,
            compression: true,
            security_headers: true,
            timeout: true,
            body_limit: false,
            concurrency_limit: true,
            backpressure: true,
            rate_limit: true,
        };

        assert_eq!(
            switches.disabled_required_in_production(true),
            vec!["MIDDLEWARE_BODY_LIMIT_ENABLED"]
        );
        // 非生产环境不限制
        assert!(switches.disabled_required_in_production(false).is_empty());
    }
}
