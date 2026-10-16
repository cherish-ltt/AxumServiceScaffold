use std::{net::SocketAddr, sync::Arc};

use axum::{
    Router,
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
    routing::get,
};
use axum_service_scaffold::{infrastructure::config::MiddlewareConfig, middleware};
use tower::ServiceExt;

fn config() -> MiddlewareConfig {
    MiddlewareConfig {
        request_timeout_secs: 1,
        max_body_bytes: 1024,
        max_concurrency: 256,
        backpressure_queue: 256,
        rate_limit_requests: 1000,
        rate_limit_period_secs: 1,
        hsts_enabled: false,
        cors_allowed_origins: Vec::new(),
        trusted_proxies: Vec::new(),
        client_max_concurrency: 1,
        client_rate_limit_requests: 2,
        client_rate_limit_period_secs: 60,
        client_max_entries: 100,
        client_idle_secs: 60,
        slow_request_ms: 1000,
    }
}

fn request(peer: &str, forwarded: Option<&str>) -> Request<Body> {
    let mut request = Request::builder().uri("/ping").body(Body::empty()).unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
    if let Some(value) = forwarded {
        request
            .headers_mut()
            .insert("x-forwarded-for", value.parse().unwrap());
    }
    request
}

fn app(config: &MiddlewareConfig) -> Router {
    middleware::apply(
        Router::new().route("/ping", get(|| async { "pong" })),
        config,
    )
}

#[tokio::test]
async fn client_rate_is_shared_across_routes_but_not_ips() {
    let app = middleware::apply(
        Router::new()
            .route("/ping", get(|| async { "pong" }))
            .route("/other", get(|| async { "other" })),
        &config(),
    );
    for _ in 0..2 {
        assert_eq!(
            app.clone()
                .oneshot(request("192.0.2.1:1000", None))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }
    let mut other = request("192.0.2.1:2000", None);
    *other.uri_mut() = "/other".parse().unwrap();
    let rejected = app.clone().oneshot(other).await.unwrap();
    assert_eq!(rejected.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(rejected.headers().contains_key("retry-after"));
    assert_eq!(
        app.oneshot(request("192.0.2.2:1000", None))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn untrusted_peers_cannot_change_identity_with_headers() {
    let app = app(&config());
    for forwarded in ["192.0.2.2", "192.0.2.3"] {
        assert_eq!(
            app.clone()
                .oneshot(request("192.0.2.1:1000", Some(forwarded)))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }
    assert_eq!(
        app.oneshot(request("192.0.2.1:1000", Some("192.0.2.4")))
            .await
            .unwrap()
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn trusted_proxy_walks_chain_from_right_and_rejects_invalid_headers() {
    let mut config = config();
    config.trusted_proxies = vec!["10.0.0.0/8".parse().unwrap(), "::1/128".parse().unwrap()];
    let app = app(&config);
    for spoof in ["192.0.2.2", "192.0.2.3"] {
        let chain = format!("{spoof}, 198.51.100.1, 10.0.0.2");
        assert_eq!(
            app.clone()
                .oneshot(request("10.0.0.1:1000", Some(&chain)))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }
    assert_eq!(
        app.clone()
            .oneshot(request("10.0.0.1:1000", Some("198.51.100.1")))
            .await
            .unwrap()
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        app.clone()
            .oneshot(request("[::1]:1000", Some("2001:db8::1")))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        app.clone()
            .oneshot(request("10.0.0.1:1000", Some("unknown")))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    let mut duplicated = request("10.0.0.1:1000", Some("192.0.2.9"));
    duplicated
        .headers_mut()
        .append("x-forwarded-for", "192.0.2.10".parse().unwrap());
    assert_eq!(
        app.oneshot(duplicated).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn mapped_ipv6_shares_ipv4_quota_and_missing_peer_fails_closed() {
    let app = app(&config());
    for peer in ["192.0.2.1:1000", "[::ffff:192.0.2.1]:1000"] {
        assert_eq!(
            app.clone()
                .oneshot(request(peer, None))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }
    assert_eq!(
        app.clone()
            .oneshot(request("192.0.2.1:1000", None))
            .await
            .unwrap()
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        app.oneshot(Request::builder().uri("/ping").body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

#[tokio::test]
async fn concurrent_client_is_rejected_without_blocking_other_clients_and_releases_on_cancel() {
    let mut config = config();
    config.client_rate_limit_requests = 100;
    let entered = Arc::new(tokio::sync::Notify::new());
    let gate = Arc::new(tokio::sync::Notify::new());
    let app = middleware::apply(
        Router::new()
            .route(
                "/ping",
                get({
                    let entered = entered.clone();
                    let gate = gate.clone();
                    move || {
                        let entered = entered.clone();
                        let gate = gate.clone();
                        async move {
                            entered.notify_one();
                            gate.notified().await;
                            "done"
                        }
                    }
                }),
            )
            .route("/fast", get(|| async { "fast" })),
        &config,
    );
    let first = tokio::spawn(app.clone().oneshot(request("192.0.2.1:1000", None)));
    entered.notified().await;
    assert_eq!(
        app.clone()
            .oneshot(request("192.0.2.1:1000", None))
            .await
            .unwrap()
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    let mut other = request("192.0.2.2:1000", None);
    *other.uri_mut() = "/fast".parse().unwrap();
    assert_eq!(
        app.clone().oneshot(other).await.unwrap().status(),
        StatusCode::OK
    );
    first.abort();
    let _ = first.await;
    let mut next = request("192.0.2.1:1000", None);
    *next.uri_mut() = "/fast".parse().unwrap();
    assert_eq!(app.oneshot(next).await.unwrap().status(), StatusCode::OK);
}
