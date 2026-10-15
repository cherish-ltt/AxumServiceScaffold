# 部署与运维

> 本文说明连接层防护、反向代理数值对齐、优雅关闭与指标抓取的部署前提。
> 中间件容量参数见 [中间件栈与配置](MIDDLEWARE.md)。

## 责任边界：应用内 vs 代理/TLS 终结

| 事项 | 责任方 | 说明 |
| --- | --- | --- |
| 请求头读取超时、空闲连接回收、请求头数量/大小上限、并发 TCP 连接上限 | **应用内**（`src/server`） | 已实现，参数为 `SERVER_*`；不依赖反向代理存在 |
| 单请求超时、请求体上限、URI 上限、并发/排队/限流、Host 校验、安全响应头、CORS | **应用内**（`src/middleware`） | 参数为 `MIDDLEWARE_*` / `HTTP_ALLOWED_HOSTS` |
| 可信客户端 IP 解析 | **应用内** | 需要 `MIDDLEWARE_TRUSTED_PROXIES` 与代理转发头配置配合 |
| TLS 终结、HTTP/2、HTTP/3、慢连接（如 `slowloris`）网络层丢弃、按 IP 限流/连接数 | **反向代理 / 负载均衡 / 网关** | 应用只提供 HTTP/1.1 明文服务，**未验证**在代理后启用 HTTP/2 回源的行为 |
| 多实例分布式限流与共享会话 | **网关或共享存储** | 应用内限速是进程级的 |

应用**只监听 HTTP/1.1**（hyper 的 `http1` 特性）：`hyper-util` 的 `auto::Builder` 在本仓库
只编译了 `http1` + `server-auto`，没有启用 `http2`。因此：

- 对外可以是 HTTPS/HTTP/2，但代理必须用 `proxy_http_version 1.1` 回源；
- 不要期望应用侧出现 `h2` 相关的配置项，`SERVER_*` 只描述 HTTP/1.1 连接。

## 连接层防护

配置项（`.env`，读取入口 `AppConfig::server`）：

| 变量 | 默认值 | 含义 |
| --- | --- | --- |
| `SERVER_HOST` / `SERVER_PORT` | `127.0.0.1` / `8080` | 监听地址 |
| `SERVER_MAX_CONNECTIONS` | `4096` | 并发 TCP 连接上限（含空闲连接）；超限的新连接被立即关闭并计入 `http_rejections_total{reason="connection_limit"}` |
| `SERVER_HTTP1_HEADER_READ_TIMEOUT_SECS` | `30` | 请求头读取超时；**同时**约束 keep-alive 空闲连接（hyper 1.x 没有独立的 idle timeout） |
| `SERVER_HTTP1_MAX_HEADERS` | `64` | 单个请求的请求头数量上限，超限由 hyper 直接回 `431` |
| `SERVER_HTTP1_MAX_BUF_SIZE` | `16384` | 请求头读取缓冲上限（字节），不得小于 `8192`（hyper 内部下限），超限回 `431` |
| `SHUTDOWN_TIMEOUT_SECS` | `20` | 收到终止信号后等待在途连接排空的上限，到期强制中止 |

要点：

- **`Timer` 必须显式设置**：hyper 的 `header_read_timeout` 只有在连接构建器设置了 `Timer` 之后
  才生效。`axum::serve` 不设置它，这正是必须自己接管 accept 循环的原因。
- **空闲连接也受这个超时约束**：keep-alive 等待下一个请求头时同样会武装超时定时器，
  因此「慢速发头」与「空闲连接占用」由同一个参数覆盖。反向代理的 `keepalive_timeout`
  必须**小于** `SERVER_HTTP1_HEADER_READ_TIMEOUT_SECS`，否则代理会先收到应用侧的连接关闭。
- **应用是明文 HTTP 服务**：TLS/HSTS 语义由代理与 `MIDDLEWARE_HSTS_*` 共同表达，
  只有确认全链路 HTTPS 时才开启 HSTS。
- 超长 URI 由应用层在 `414` 语义下拒绝（`MIDDLEWARE_MAX_URI_BYTES`），
  该检查发生在 HTTP 解析之后、路由匹配之前，因此不会以原始 URI 形态进入日志与指标。

## 优雅关闭

`src/server/shutdown.rs` 监听 `Ctrl-C`（SIGINT）与 `SIGTERM`，流程为：

1. 停止 `accept`（不再接收新连接）；
2. 等待在途连接排空，最长 `SHUTDOWN_TIMEOUT_SECS`（hyper 的 graceful shutdown 会先结束
   已排队请求、再关闭空闲连接）；
3. 超时后强制中止剩余连接任务并打 `warn` 日志；
4. 进程退出（日志缓冲由 `BatchGuard` 落盘，不丢日志）。

**部署前提**：编排层的终止宽限期必须大于应用的关闭上限，例如 Kubernetes
`terminationGracePeriodSeconds: 30` 对应 `SHUTDOWN_TIMEOUT_SECS=20`；
`preStop`/`drain` 时间也算在宽限期内。若宽限期小于应用的关闭上限，进程会在落盘最后一批日志前
被 `SIGKILL` 硬杀。

## 反向代理配置

仓库在 [`deploy/nginx/axum-service-scaffold.conf`](../deploy/nginx/axum-service-scaffold.conf)
提供一份 nginx 片段（**未在真实生产环境验证**，适用 nginx 1.24+）。核心是数值对齐：

| nginx 指令 | 应对齐的应用参数 | 关系 |
| --- | --- | --- |
| `client_header_timeout` | `SERVER_HTTP1_HEADER_READ_TIMEOUT_SECS` | 代理侧更短（先于应用超时断开慢速发头） |
| `keepalive_timeout` | `SERVER_HTTP1_HEADER_READ_TIMEOUT_SECS` | 代理侧更短 |
| `client_max_body_size` | `MIDDLEWARE_MAX_BODY_BYTES` | 代理侧不小于应用侧，保证应用能返回结构化 `413` |
| `large_client_header_buffers` | `MIDDLEWARE_MAX_URI_BYTES` | 代理侧缓冲容量与 URI 上限同量级 |
| `limit_conn` | `SERVER_MAX_CONNECTIONS` | 代理侧更小或相等，避免把连接数压力推给应用 |
| `limit_req` | `MIDDLEWARE_RATE_LIMIT_REQUESTS` / `PERIOD` | 代理侧承担按 IP 限流 |
| `proxy_read_timeout` | `MIDDLEWARE_REQUEST_TIMEOUT_SECS` | 代理侧略大（让应用先返回 408/500 而不是被代理截断） |
| `proxy_http_version 1.1` + `Connection ""` | —— | 回源必须用 HTTP/1.1 与长连接 |

转发头必须显式设置，且 `MIDDLEWARE_TRUSTED_PROXIES` 要填代理的**真实出口网段**：

```nginx
proxy_set_header Host              $host;
proxy_set_header X-Real-IP         $remote_addr;
proxy_set_header X-Forwarded-For   $proxy_add_x_forwarded_for;
proxy_set_header X-Forwarded-Proto $scheme;
```

应用只在这些头来自可信网段时才采信，并按信任链从右向左取第一个不可信地址；
`MIDDLEWARE_TRUSTED_PROXIES` 留空时**完全忽略**转发头，客户端伪造的 `X-Forwarded-For`
不会影响访问日志与 Host 校验。

## 指标抓取

`/metrics` 与业务接口同端口暴露，必须带令牌：

```bash
curl -H "Authorization: Bearer $METRICS_TOKEN" http://127.0.0.1:8080/metrics
```

- 令牌不正确时返回 `404`（而不是 `401/403`），避免向探测者暴露端点存在；
- `METRICS_ENABLED=false` 时端点完全不注册；
- 生产环境不允许使用示例令牌（`METRICS_TOKEN` 至少 16 字符且不等于 `.env-example` 中的示例值，
  否则拒绝启动）。

抓取注意：**全局限速也作用于 `/metrics`**。如果 `MIDDLEWARE_RATE_LIMIT_REQUESTS` 设得很小，
Prometheus 抓取可能拿到 `429`。生产环境应满足以下之一：

- 限速额度远高于抓取频率（默认配置即可）；
- 用代理把 `/metrics` 放到内网端口/内网监听地址，不经过应用的限速路径；
- 抓取间隔与额度对齐（例如额度 ≥ 抓取频率 × 实例数 × 2）。

`/metrics` 不应暴露到公网；令牌是唯一的准入手段，建议同时用代理或安全组限制来源。

## 生产环境启动校验

`APP_ENV=production` 时启动阶段会强制校验（不满足则拒绝启动）：

- 构建画像：**debug 构建或启用 `docs` feature 的构建默认拒绝启动**，因为调试登录接口
  （`/api/v1/auth/dev-login`）能伪造任意身份。确需在生产运行这类构建时，设置
  `APP_ALLOW_INSECURE_PRODUCTION=true`，此时调试路由与文档路由仍然不会注册；
- 安全关键中间件必须开启（见 [中间件栈与配置](MIDDLEWARE.md#中间件开关)），
  确需关闭时设置 `MIDDLEWARE_ALLOW_INSECURE_PRODUCTION=true`；
- `HTTP_ALLOWED_HOSTS` 必须非空且不含 `*`；
- `JWT_SECRET`、`METRICS_TOKEN` 不得为 `.env-example` 中的示例值。

生产构建与启动的最小命令：

```bash
cargo build --release          # 不要加 --features docs
APP_ENV=production HTTP_ALLOWED_HOSTS=api.example.com ./target/release/axum-service-scaffold
```

`.env` 不参与版本控制（已在 `.gitignore` 中）；容器部署请用编排系统的密钥管理注入
`JWT_SECRET` / `METRICS_TOKEN`，不要把它们写进镜像。

## 未验证事项

以下行为本仓库未做验证，请在生产环境自行确认后再依赖：

- nginx 片段在真实流量下的表现（连接数、Keep-Alive、长连接）；
- 代理后 HTTP/2 对外、HTTP/1.1 回源的完整链路（含 HSTS 与重定向策略）；
- 多实例部署下的限速与指标聚合语义；
- `SERVER_MAX_CONNECTIONS` 与文件描述符上限（`ulimit -n`）的匹配关系：
  该值加上日志文件、数据库连接后必须小于进程可用 FD 上限。
