# 中间件栈与配置

> 本文档详解 HTTP 中间件栈、容量参数与压测调参方法，配置总览见 [README](../README.md)，
> 生产部署与代理数值对齐见 [部署与运维](DEPLOYMENT.md)。

## 中间件栈

所有中间件在 `src/middleware/stack.rs` 里用 `tower::ServiceBuilder` 一次装配，并通过
`middleware::apply` 包在整个 `Router` 外层，控制器对它们完全无感。

之所以不用 `Router::layer` 施加这些层：`PathRouter::layer` 会对**每条路由**复制一份 layer
（`endpoint.layer(layer.clone())`），有状态的并发门 / 限流会被放大成「每路由一份」，
全局并发/限流语义直接失效。唯一的例外是**路由模板捕获层**
（`middleware::route_slot::capture_route_templates`）：它无状态，且必须位于路由匹配之内才能拿到
`MatchedPath`，用于给指标与访问日志提供路由模板标签。

执行顺序（外 → 内）：

| 顺序 | 中间件 | 解决的问题 | 失败响应 |
| --- | --- | --- | --- |
| 1 | `HandleErrorLayer(internal)` | 把栈内未识别错误收敛为统一 500 响应，并让错误类型变成 `Infallible` | `500` |
| 2 | `SetRequestIdLayer` + `PropagateRequestIdLayer` | 请求没有 `x-request-id` 时生成，并写回响应头 | — |
| 3 | `ClientIpLayer` | 按可信代理网段解析真实客户端 IP（写入请求扩展） | — |
| 4 | `SetMultipleResponseHeadersLayer` | `nosniff`、`X-Frame-Options: DENY`、`Referrer-Policy`、`Cache-Control: no-store`，可选 `HSTS` | — |
| 5 | `ObservabilityLayer` | 结构化访问日志 + 指标（状态码、路由模板、延迟直方图、在途数） | — |
| 6 | `UriLimitLayer` | 超长 URI（默认 > 8 KiB）直接拒绝，避免写进日志与指标 | `414` |
| 7 | `HostCheckLayer` | `HTTP_ALLOWED_HOSTS` 白名单校验（含 Host 缺失与伪造） | 缺失 `400` / 不在白名单 `421` |
| 8 | `ErrorToResponseLayer(overload)` | 把背压/concurrency 的 overload 错误转成响应 | `503` |
| 9 | `LoadShedLayer` | 下游不可及时立即拒绝，不排队等待 | `503` |
| 10 | `BufferLayer` | 固定长度等待队列，杜绝无限堆积 | 队列满 → `503` |
| 11 | `ConcurrencyGateLayer` | 在途请求上限（全局），拿到队列名额后最多等待 `MIDDLEWARE_QUEUE_WAIT_TIMEOUT_SECS` | 等待超时 → `503` |
| 12 | `ErrorToResponseLayer(rate_limited)` | 把限流 overload 错误转成响应 | `429` |
| 13 | `BufferLayer` + `LoadShedLayer` + `RateLimitLayer` | 进程级全局限速（每周期 N 个请求）；三层整体开关 | `429` |
| 14 | `CompressionLayer` | 按 `Accept-Encoding` 压缩响应，>32 字节且非图片/gRPC/SSE 才压缩 | — |
| 15 | `TimeoutLayer` | 单个请求处理超时（覆盖排队等待与处理） | `408` |
| 16 | `BodyLimitLayer` | 请求体大小上限，按 `Content-Length` 先行拒绝，分块传输按实际字节累计 | `413` |
| 17 | `CorsLayer` | 按 `MIDDLEWARE_CORS_ALLOWED_ORIGINS` 白名单放行跨域；白名单为空时不发任何 CORS 头 | 无（预检返回 `200` 空响应） |

关键点：

- **并发限制与限速是两个维度**。并发限制约束「同时在途的请求数」，限流约束「单位时间的请求数」，
  两者都要有：前者防打满线程/连接池，后者防突发流量。
- **背压 = 并发门 + 有界队列 + `LoadShed`**。容量内正常执行；短暂超出时请求在有界队列里有限等待
  （等待上限由 `MIDDLEWARE_QUEUE_WAIT_TIMEOUT_SECS` 决定）；队列也满时立即返回 `503`。
- **`BufferLayer` 不是额外的并发池**：它只是一个固定长度的 mpsc 队列，克隆共享同一个 worker，
  因此不会把并发度放大，`axum::serve` / `fallback_service` 要求的 `Clone` 也由此满足。
- **拒绝也能被观测**：可观测层位于各拒绝层之外，因此限流/背压/队列等待/连接上限产生的拒绝
  都会被统计到 `http_rejections_total`，并在访问日志里带上原因。
- **`x-request-id` 贯穿全链路**：客户端已带则透传，未带则生成；访问日志的 span 里带有
  `request_id` 字段，可与响应头对齐排查。
- 中间件产生的错误统一走 `ApiResponse` 结构（`src/middleware/error_response.rs`），
  与业务错误 `AppError` 的响应格式一致。

### 拒绝原因与日志级别

批量拒绝不逐条打 `WARN`/`ERROR`：过载时逐条告警会产出 GB 级日志，且没有信息量。
拒绝次数由指标统计，请求本身仍有一条访问日志，级别按下表区分：

| 场景 | 状态码 | 指标 `reason` | 访问日志级别 |
| --- | --- | --- | --- |
| 并发门等待超时 | `503` | `queue_wait_timeout` | `DEBUG` |
| 背压队列满 / 并发过载 | `503` | `overload` | `DEBUG` |
| 限流额度用尽 | `429` | `rate_limited` | `DEBUG` |
| TCP 连接数超上限 | 连接被关闭 | `connection_limit` | 连接层 `DEBUG`（无请求日志） |
| 请求在应答前被取消 | — | `cancelled` | `DEBUG` |
| 其余 4xx（含 414 / 421 / 408 / 413 / 401 / 404 / 405） | 对应状态码 | — | `WARN` |
| 真正的 5xx | `5xx` | — | `ERROR` |

`503` 由业务侧 `AppError::unavailable` 产生时，`src/error.rs` 仍会单独记录一条 `ERROR`，
不会因为访问日志降级而丢失内部故障信息。

常用 PromQL：

```promql
# 拒绝原因分布（按原因看是哪一类限流在起作用）
sum by (reason) (rate(http_rejections_total[5m]))

# 路由级 QPS
sum by (route) (rate(http_requests_total[5m]))

# 路由级 P95 延迟
histogram_quantile(0.95, sum by (le, route) (rate(http_request_duration_seconds_bucket[5m])))

# 5xx 比例
sum(rate(http_requests_total{status=~"5.."}[5m])) / sum(rate(http_requests_total[5m]))

# 在途请求数（判断并发门是否长期打满）
max(http_requests_inflight)
```

指标标签只使用**路由模板**（如 `/api/v1/transactions/{id}`）与固定状态码，未匹配路由统一为
`unmatched`；请求 ID、查询串、路径参数都不进标签，避免基数随流量形态爆炸。

## 中间件开关

`MIDDLEWARE_*_ENABLED` 控制各层是否装配；关闭某一层不会改变其余层的顺序与语义。

| 变量 | 默认值 | 关闭后的影响 |
| --- | --- | --- |
| `MIDDLEWARE_REQUEST_ID_ENABLED` | `true` | 不生成/不回写 `x-request-id`，日志失去请求关联 |
| `MIDDLEWARE_ACCESS_LOG_ENABLED` | `true` | 不再输出访问日志（指标不受影响） |
| `MIDDLEWARE_COMPRESSION_ENABLED` | `true` | 响应不压缩 |
| `MIDDLEWARE_SECURITY_HEADERS_ENABLED` | `true` | 不下发安全响应头与 `Cache-Control` |
| `MIDDLEWARE_TIMEOUT_ENABLED` | `true` | 慢请求不再被 408 打断（**危险**） |
| `MIDDLEWARE_BODY_LIMIT_ENABLED` | `true` | 不限制请求体大小（**危险**） |
| `MIDDLEWARE_CONCURRENCY_LIMIT_ENABLED` | `true` | 不限制在途请求数（**危险**） |
| `MIDDLEWARE_BACKPRESSURE_ENABLED` | `true` | 不再有等待队列，压力直接传导到下游（**危险**） |
| `MIDDLEWARE_RATE_LIMIT_ENABLED` | `true` | 不限速（**危险**） |

`APP_ENV=production` 时，上表中标「危险」的项与 `MIDDLEWARE_SECURITY_HEADERS_ENABLED`、
`MIDDLEWARE_REQUEST_ID_ENABLED` 必须为 `true`，并且 `HTTP_ALLOWED_HOSTS` 不能为空，
否则**拒绝启动**。确需在上游代理提供同等保护的前提下关闭，必须显式设置
`MIDDLEWARE_ALLOW_INSECURE_PRODUCTION=true`。

压缩与请求体限制这两层始终在栈中：它们会改变请求/响应体类型，无法用 `Option<Layer>`
表达关闭，因此靠配置值实现关闭（压强设为不压缩 / 上限设为 `usize::MAX`）。

## 中间件配置

全部集中在 `.env`，读取入口是 `AppConfig::middleware`（`src/infrastructure/config/middleware.rs`），
中间件代码里不出现字面量：

| 变量 | 默认值 | 含义 |
| --- | --- | --- |
| `MIDDLEWARE_REQUEST_TIMEOUT_SECS` | `10` | 单请求整体超时（秒），超时返回 `408` |
| `MIDDLEWARE_QUEUE_WAIT_TIMEOUT_SECS` | `5` | 拿到背压队列名额后等待并发许可的上限（秒），超时返回 `503`；不得大于总超时 |
| `MIDDLEWARE_MAX_BODY_BYTES` | `2097152`（2 MiB） | 请求体上限，超限返回 `413` |
| `MIDDLEWARE_MAX_URI_BYTES` | `8192`（8 KiB） | 单个请求 URI 长度上限，超限返回 `414` |
| `MIDDLEWARE_MAX_CONCURRENCY` | `256` | 同时在途请求上限 |
| `MIDDLEWARE_BACKPRESSURE_QUEUE` | `256` | 并发已满时允许排队的请求数，超出立即 `503` |
| `MIDDLEWARE_RATE_LIMIT_REQUESTS` | `32768` | 每个限流周期允许的请求数，超出 `429` |
| `MIDDLEWARE_RATE_LIMIT_PERIOD_SECS` | `1` | 限流周期长度（秒） |
| `MIDDLEWARE_HSTS_ENABLED` | 跟随 `APP_ENV` | 生产环境默认下发 `HSTS`，开发环境默认关闭 |
| `MIDDLEWARE_HSTS_MAX_AGE_SECS` | `31536000` | `HSTS` 的 `max-age` |
| `MIDDLEWARE_HSTS_INCLUDE_SUBDOMAINS` | `false` | 是否包含 `includeSubDomains` |
| `MIDDLEWARE_HSTS_PRELOAD` | `false` | 是否包含 `preload`；要求同时开启 `includeSubDomains` 且 `max-age >= 31536000` |
| `MIDDLEWARE_CORS_ALLOWED_ORIGINS` | 空 | 允许跨域的来源白名单，逗号分隔；留空不下发任何 CORS 头，单值 `*` 放行全部来源 |
| `HTTP_ALLOWED_HOSTS` | 空 | Host 白名单，逗号分隔；条目不含端口时匹配任意端口，含端口时精确匹配；不接受 `*` |
| `MIDDLEWARE_TRUSTED_PROXIES` | 空 | 可信反向代理网段（CIDR 或单个 IP），逗号分隔；只有直连对端落在此范围内才采信转发头 |

未在 `.env` 中出现的参数才使用「跟随 `APP_ENV`」这类默认值：显式赋值（含 `false`）一律覆盖默认值，
因此 `.env-example` 中把 `MIDDLEWARE_HSTS_ENABLED` 注释掉而不是写成 `false`，
生产环境才能拿到「默认开启 HSTS」的语义。

任一容量参数为 `0` 都会导致启动失败，避免「以为限制了其实没限制」；
`MIDDLEWARE_CORS_ALLOWED_ORIGINS`、`MIDDLEWARE_TRUSTED_PROXIES`、`HTTP_ALLOWED_HOSTS` 的每个条目
都会在启动阶段解析校验，非法条目直接拒绝启动；`MIDDLEWARE_QUEUE_WAIT_TIMEOUT_SECS` 大于
`MIDDLEWARE_REQUEST_TIMEOUT_SECS` 同样拒绝启动（否则等待时间会被总超时截断，配置失去意义）。

连接层参数（`SERVER_*`、`SHUTDOWN_TIMEOUT_SECS`）不在中间件栈里，属于 accept 循环与 hyper
连接构建器，见 [部署与运维](DEPLOYMENT.md) 的「连接层防护」。

上表的默认值与 `.env-example` 示例文件一致，适配「单实例 + SQLite/中小型数据库」的中等配置
机器；上线前必须按压测结果调整：

- `MIDDLEWARE_MAX_CONCURRENCY`：由下游能承受的最大在途请求数决定，通常取压测中找到的
  「p99 开始明显劣化」并发点的 80%。
- `MIDDLEWARE_BACKPRESSURE_QUEUE`：吸收突发用，取 `MAX_CONCURRENCY` 的 1/4 ~ 1 倍即可；
  队列越大，被排队的请求尾延迟越高（最长等待时间由 `MIDDLEWARE_QUEUE_WAIT_TIMEOUT_SECS` 兜住）。
- `MIDDLEWARE_QUEUE_WAIT_TIMEOUT_SECS`：应小于业务 p99 的 2~3 倍，让「注定要超时」的请求尽早
  拿到 `503`，而不是白白占住队列名额。
- `MIDDLEWARE_RATE_LIMIT_REQUESTS` / `PERIOD`：由下游 QPS 容量决定；单实例场景是进程级计数，
  **不是分布式限流**，多实例部署需要换到网关层或基于共享存储的实现（见下文「按 IP 限流」）。
- `MIDDLEWARE_REQUEST_TIMEOUT_SECS`：取业务正常 p99 的 3~5 倍；过小会误杀慢请求，
  过大则慢请求会长时间占住并发额度。
- `MIDDLEWARE_MAX_BODY_BYTES`：按最大上传接口的实际需求设置，默认 2 MiB 对 JSON API 足够宽裕。
- `MIDDLEWARE_MAX_URI_BYTES`：与反向代理的 `large_client_header_buffers` 保持同一量级，
  代理侧先拒绝时应用侧这一层不会触发。
- `MIDDLEWARE_CORS_ALLOWED_ORIGINS`：生产环境应显式列出前端域名，不要用 `*`；
  留空表示服务不供浏览器跨域调用（同源与后端调用不受影响）。

## 按 IP 限流（现状与扩展点）

当前实现是**进程级全局限速**，不按来源 IP 分桶。原因：按 IP 限流必须解决「IP 从哪来」与
「桶存在哪」两个问题——前者已经有基础（`ClientIp` 中间件解析出了可信来源 IP），后者在单实例
部署下只能存在进程内存里（多实例各算各的，等于把额度乘以实例数）。

需要时按下面的方式扩展，而不是把逻辑塞进全局栈：

- 单实例、按路由分桶：在 `api::router()` 里对目标路由用 `Router::layer` 叠一个自定义限流层
  （`Router::layer` 会按路由复制 layer，这里正是需要的「每路由一份额度」语义），
  键取 `Request::extensions` 里的 `ClientIp`。
- 多实例：把额度判定与计数搬到网关（Nginx `limit_req` + `limit_conn`，见
  [nginx 片段](../deploy/nginx/axum-service-scaffold.conf)）或共享存储（Redis 计数），
  应用侧只保留全局限速作为兜底。

不引入 Redis 作为默认依赖：脚手架默认单实例 + SQLite，多一个必需的外部组件会破坏
「克隆即可运行」，代价大于收益。真需要分布式限流时，应当连同部署形态一起决策。

## 长连接（WebSocket / SSE）

当前没有 WebSocket / SSE 端点，因此栈里的 `TimeoutLayer` 只作用于请求-响应模型。若将来引入
长连接，注意它不能沿用现有超时与并发语义：

- `MIDDLEWARE_REQUEST_TIMEOUT_SECS` 会按「响应产生时间」计时，长连接会一直被 408 截断，
  需要为长连接路由单独装配（`Router::layer` 作用域内）不带超时的子栈；
- 连接级超时（`SERVER_HTTP1_HEADER_READ_TIMEOUT_SECS`）只作用于请求头读取与 keep-alive 空闲，
  不影响升级后的隧道；
- 并发额度应按连接而不是按请求计量，否则一条连接上的多个消息会各自占用额度；
- 反向代理侧需要显式放开 `proxy_read_timeout`（nginx 默认 60s 会掐断空闲长连接）。

## 最小压测方法

先确认并发上限，再确定限速额度。以 [oha](https://github.com/hatoo/oha) 为例：

```bash
cargo install oha

# 1. 把限流放宽（甚至暂时设为很大），排除限流干扰，逐级提高并发观察 p99 与错误率
oha -c 64  -z 30s --no-tui http://127.0.0.1:8080/api/v1/system/health
oha -c 128 -z 30s --no-tui http://127.0.0.1:8080/api/v1/system/health
oha -c 256 -z 30s --no-tui http://127.0.0.1:8080/api/v1/system/health

# 2. 固定并发在上一步的容量内，逐级提高目标 QPS，观察 429 出现点与下游延迟
oha -c 128 -q 500  -z 30s --no-tui http://127.0.0.1:8080/api/v1/system/health
oha -c 128 -q 2000 -z 30s --no-tui http://127.0.0.1:8080/api/v1/system/health
```

读结果的方式：

- 并发压测出现 `503` 是**预期**的背压行为，关注点是「503 立即返回」且成功请求的 p99 稳定；
  如果成功请求的 p99 随并发持续恶化，说明 `MAX_CONCURRENCY` 设得比下游容量大。
- `503` 的比例长期很高说明 `MAX_CONCURRENCY` 偏小，容量被浪费。
- `429` 只应在压测目标 QPS 超过 `RATE_LIMIT_REQUESTS / PERIOD` 时出现，且延迟应接近零
  （快速拒绝而不是排队等窗口）。
- 超时 `408` 的理想值是 0；出现 `408` 应当先查慢查询/外部依赖，而不是直接调大超时。
- 观察指标而不是只读压测报告：`sum by (reason) (rate(http_rejections_total[5m]))` 能直接区分
  「队列满」「排队超时」「限流」三种拒绝。
