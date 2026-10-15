use std::time::Duration;

use axum::{
    Router,
    body::Body,
    error_handling::HandleErrorLayer,
    http::{Extensions, HeaderMap, HeaderName, HeaderValue, Response, StatusCode, Version, header},
};
use tower::{ServiceBuilder, buffer::BufferLayer, limit::RateLimitLayer, load_shed::LoadShedLayer};
use tower_http::{
    compression::CompressionLayer,
    cors::{AllowOrigin, Any, CorsLayer},
    request_id::{
        MakeRequestUuid, PropagateRequestId, PropagateRequestIdLayer, SetRequestId,
        SetRequestIdLayer,
    },
    set_header::{HeaderMetadata, response::SetMultipleResponseHeadersLayer},
    timeout::TimeoutLayer,
};

use crate::{
    infrastructure::config::{AppConfig, MiddlewareConfig},
    middleware::{
        body::NormalizeBodyLayer,
        client_ip::ClientIpLayer,
        concurrency::ConcurrencyGateLayer,
        error_layer::{EraseErrorLayer, ErrorToResponseLayer},
        error_response,
        host::HostCheckLayer,
        limits::UriLimitLayer,
        observability::{ObservabilityConfig, ObservabilityLayer},
        payload::BodyLimitLayer,
        rejection::RejectionBodyLayer,
    },
};

const REQUEST_ID_HEADER: &str = "x-request-id";

/// 用 `ServiceBuilder` 把中间件栈包在整个应用之外。
///
/// 必须包住整个 Router，不能用 `Router::layer`：后者会把有状态中间件逐路由复制一份，
/// 全局并发上限与全局限流会因此失效。唯一的例外是路由模板捕获层
/// （`route_slot::capture_route_templates`），它无状态且必须位于路由匹配之内。
///
/// 层序（外 → 内）即执行顺序，语义要点：
/// 1. `request_id` 最先，后续所有日志与响应头都能带上它；
/// 2. 客户端 IP 解析先于可观测性，访问日志才能记录来源；
/// 3. 可观测性位于各拒绝层之外，中间件自身产生的 429/503/408 也能被统计；
/// 4. Host / URI 校验早于并发与限流，避免为无效请求消耗配额；
/// 5. 并发门（内含排队等待上限）在背压队列之内：队列满由 `LoadShed` 快速拒绝，
///    拿到队列名额后等待许可的时间由 `MIDDLEWARE_QUEUE_WAIT_TIMEOUT_SECS` 约束；
/// 6. 请求总超时覆盖排队与处理；
/// 7. 栈顶 `HandleErrorLayer` 把剩余错误收敛为 500，同时把错误类型收敛为 `Infallible`
///    以满足 `Router::fallback_service` 的要求。
///
/// 各层是否装配由 `MiddlewareConfig.switches` 决定（`option_layer`），
/// 关闭某一层不会改变其余层的顺序与语义。
///
/// 两个例外写在装配处：请求体限制与压缩会改变「请求/响应体类型」，
/// 无法用 `Option<L>` 表达关闭（两侧类型必须一致），因此它们始终在栈中，
/// 靠配置值自身（不限制 / 永不压缩）实现关闭。
pub fn apply(app: Router, config: &AppConfig) -> Router {
    let middleware = &config.middleware;
    let switches = middleware.switches;
    let compression_enabled = switches.compression;

    let stack = ServiceBuilder::new()
        .layer(HandleErrorLayer::new(error_response::internal))
        .option_layer(switches.request_id.then_some(RequestIdBundle))
        .layer(ClientIpLayer::new(middleware.trusted_proxies.clone()))
        .option_layer(
            switches
                .security_headers
                .then(|| SetMultipleResponseHeadersLayer::overriding(security_headers(middleware))),
        )
        .layer(ObservabilityLayer::new(ObservabilityConfig {
            metrics_enabled: config.metrics.enabled,
            access_log: switches.access_log,
        }))
        .layer(UriLimitLayer::new(middleware.max_uri_bytes))
        .option_layer(
            (!middleware.allowed_hosts.is_empty())
                .then(|| HostCheckLayer::new(middleware.allowed_hosts.clone())),
        )
        .layer(ErrorToResponseLayer::new(error_response::overload_mapper))
        .option_layer(switches.backpressure.then(LoadShedLayer::new))
        .option_layer(
            switches
                .backpressure
                .then(|| BufferLayer::new(middleware.backpressure_queue)),
        )
        .option_layer(switches.concurrency_limit.then(|| {
            ConcurrencyGateLayer::new(
                middleware.max_concurrency,
                Duration::from_secs(middleware.queue_wait_effective_secs()),
            )
        }))
        .layer(ErrorToResponseLayer::new(
            error_response::rate_limited_mapper,
        ))
        // 限流三层必须整体开关：`RateLimit` 自身不是 `Clone`，靠外层 `Buffer` 包一层
        // 才能满足 `fallback_service` 的 `Clone` 约束；拆开单独开关会同时破坏两者的语义。
        .option_layer(switches.rate_limit.then(|| {
            ServiceBuilder::new()
                .layer(BufferLayer::new(middleware.backpressure_queue))
                .layer(LoadShedLayer::new())
                .layer(RateLimitLayer::new(
                    middleware.rate_limit_requests,
                    Duration::from_secs(middleware.rate_limit_period_secs),
                ))
        }))
        // 统一框架层错误响应（tower-http 的 408 空体 / 413 纯文本）的响应体格式，
        // 必须位于压缩层之外，改写后的响应体才不会被压缩头欺骗。
        .layer(RejectionBodyLayer)
        // 把压缩与请求体限制包装过的响应体类型收敛回 `Body`，
        // 其外侧的所有自定义层才能统一按 `Response<Body>` 编写。
        .layer(NormalizeBodyLayer)
        // 错误响应不压缩：它们只有几十字节，压缩没有收益，还会让上层无法安全改写响应体。
        .layer(CompressionLayer::new().compress_when(
            move |status: StatusCode, _: Version, _: &HeaderMap, _: &Extensions| {
                compression_enabled && !status.is_client_error() && !status.is_server_error()
            },
        ))
        .option_layer(switches.timeout.then(|| {
            TimeoutLayer::with_status_code(
                StatusCode::REQUEST_TIMEOUT,
                Duration::from_secs(middleware.request_timeout_secs),
            )
        }))
        .layer(BodyLimitLayer::new(
            middleware.max_body_bytes,
            switches.body_limit,
        ))
        .layer(cors_layer(middleware))
        .layer(EraseErrorLayer)
        .service(app);

    Router::new().fallback_service(stack)
}

/// 请求 ID 两层组合成一个 Layer。
///
/// 必须成套出现：`SetRequestIdLayer` 在最外负责生成，`PropagateRequestIdLayer` 在其内
/// 读取请求头并回写到响应；只开一半会让关联失效，所以开关只能整体生效。
#[derive(Clone, Copy)]
struct RequestIdBundle;

impl<S> tower::Layer<S> for RequestIdBundle {
    type Service = SetRequestId<PropagateRequestId<S>, MakeRequestUuid>;

    fn layer(&self, inner: S) -> Self::Service {
        SetRequestIdLayer::new(request_id_header(), MakeRequestUuid)
            .layer(PropagateRequestIdLayer::new(request_id_header()).layer(inner))
    }
}

/// CORS 与其余中间件一起装配，不再由 controller 或 `create_app` 各自添加。
///
/// CORS 没有独立开关：白名单为空时本层不下发任何跨域放行头，等价于关闭，
/// 而生产环境误配 `*` 属于必须被显式审视的动作。
fn cors_layer(config: &MiddlewareConfig) -> CorsLayer {
    if config
        .cors_allowed_origins
        .iter()
        .any(|origin| origin == "*")
    {
        return CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);
    }

    let allowed: Vec<HeaderValue> = config
        .cors_allowed_origins
        .iter()
        .filter_map(|origin| HeaderValue::from_str(origin).ok())
        .collect();

    if allowed.is_empty() {
        return CorsLayer::new();
    }

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(allowed))
        .allow_methods(Any)
        .allow_headers(Any)
}

fn request_id_header() -> HeaderName {
    HeaderName::from_static(REQUEST_ID_HEADER)
}

/// 安全响应头。
///
/// `Cache-Control: no-store` 是全体 API 的默认值：本服务的响应都带用户身份或业务数据，
/// 且没有任何接口声明过可缓存语义，漏一条缓存头就可能让中间代理缓存敏感响应。
fn security_headers(config: &MiddlewareConfig) -> Vec<HeaderMetadata<Response<Body>>> {
    let mut headers: Vec<HeaderMetadata<Response<Body>>> = vec![
        (
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        )
            .into(),
        (header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY")).into(),
        (
            header::REFERRER_POLICY,
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        )
            .into(),
        (
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store, no-cache, must-revalidate"),
        )
            .into(),
    ];

    if config.hsts.enabled {
        // HSTS 组合在启动阶段已校验，这里的兜底仅为极端场景下的合法头值。
        let value = HeaderValue::from_str(&config.hsts.header_value())
            .unwrap_or_else(|_| HeaderValue::from_static("max-age=31536000"));
        headers.push((header::STRICT_TRANSPORT_SECURITY, value).into());
    }

    headers
}
