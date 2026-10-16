# 中间件栈与配置

> 本文档详解 HTTP 中间件栈、容量参数与压测调参方法，配置总览见 [README](../README.md)。

## 中间件栈

所有中间件在 `src/middleware/stack.rs` 里用 `tower::ServiceBuilder` 一次装配，并通过
`middleware::apply` 包在整个 `Router` 外层，控制器对它们完全无感。

之所以不用 `Router::layer` 施加这些层：`PathRouter::layer` 会对**每条路由**复制一份 layer
（`endpoint.layer(layer.clone())`），有状态的 `ConcurrencyLimit` / `RateLimit` 会被放大成
「每路由一份」，全局并发/限流语义直接失效。此外，栈在 Router 外层意味着未匹配路由的 404
请求也会占用并发/限流额度，这是有意为之（非法流量同样消耗资源）。

执行顺序（外 → 内）：

| 顺序 | 中间件 | 解决的问题 | 失败响应 |
| --- | --- | --- | --- |
| 1 | 请求 ID 校验 / `SetRequestIdLayer` | 只保留 1–64 字节的字母、数字、`-_.` ID，其余替换为 UUID | — |
| 2 | `PropagateRequestIdLayer` | 回写请求 ID | — |
| 3 | 客户端识别 | 读取 TCP 对端，解析显式可信代理链 | 后续保护层返回 400 / 500 |
| 4 | 安全响应头 | nosniff / DENY / Referrer-Policy / 可选 HSTS，覆盖超时响应 | — |
| 5 | 请求观测与总超时 | 创建请求 span，计时含排队；统一框架错误，输出一条访问或诊断日志 | 408 JSON |
| 6 | `CatchPanicLayer` | 将请求处理 panic 转成安全响应，保留错误日志 | 500 JSON |
| 7 | 客户端保护 | 按 IP 限并发 / 固定窗口请求量，有界注册表 | 429 / 503 |
| 8 | `HandleError` / `LoadShed` / `Buffer` | 有界背压，满后快速拒绝 | 503 |
| 9 | `ConcurrencyLimitLayer` | 全局在途上限 | 超出进入队列 |
| 10 | `HandleError` / `Buffer` / `LoadShed` / `RateLimit` | 进程级速率额度，窗口满后快速拒绝 | 429 |
| 11 | `CompressionLayer` | gzip 响应压缩 | — |
| 12 | 诊断上下文 | 记录排队时间，将阶段测量上下文传入 handler | — |
| 13 | `RequestBodyLimitLayer` | Content-Length 先行拒绝，流式读取限制实际累计大小 | 413 |
| 14 | `CorsLayer` | 显式 CORS 白名单 | 预检 200 空响应 |
| 15 | Router 路由诊断 | 匹配后记录路由模板；无状态，不分配独立限流额度 | — |

关键点：

- **并发限制与限速是两个维度**。并发限制约束「同时在途的请求数」，限流约束「单位时间的请求数」，
  两者都要有：前者防打满线程/连接池，后者防突发流量。
- **背压 = `ConcurrencyLimit` + 有界队列 + `LoadShed`**。容量内正常执行；短暂超出时请求在有界队列里
  有限等待；队列也满时立即返回 `503`，不会堆积到拖垮进程。
- **`BufferLayer` 不是额外的并发池**：它只是一个固定长度的 mpsc 队列，克隆共享同一个 worker，
  因此不会把并发度放大，`axum::serve` 要求的 `Clone` 也由此满足。
- **`x-request-id` 贯穿全链路**：合法客户端 ID 保留，否则生成 UUID；请求 span 带 ID 和可信来源 IP，
  handler 日志和响应头可以对齐。转发链和原始非法 ID 不写入访问日志。
- 中间件产生的错误统一走 `ApiResponse` 结构（`src/middleware/error_response.rs`），
  与业务错误 `AppError` 的响应格式一致。
- **客户端额度**：在全局队列之前按规范化 IP 分配并发与固定窗口速率额度，跨路由共享；
  同一 NAT 出口共用额度。拒绝请求不排队，速率超限带 Retry-After；并发超限不猜测重试时间。
  注册表满时新来源返回 503；活动项不驱逐，空闲项定期清理，不依赖无限增长的 IP 缓存。
- **可信代理**：默认忽略所有转发头。仅当真实 TCP 对端匹配显式 IP/CIDR 时解析
  X-Forwarded-For，从右向左取第一个不可信地址；重复或参与解析的非法地址返回 400。
  不使用 X-Real-IP / Forwarded 兜底，网关必须覆盖或正确追加 X-Forwarded-For。
  例如本机代理设置 `MIDDLEWARE_TRUSTED_PROXIES=127.0.0.1/32,::1/128`，并阻止公网绕过网关。
  单值 `*` 表示信任所有直连对端（等价于完全采信客户端自报的 X-Forwarded-For），
  仅用于本地调试：生产环境遇到前缀长度为 0 的条目会拒绝启动，且 `*` 不得与具体条目混写。
  自定义启动入口必须注入 ConnectInfo；缺少真实对端信息返回 500，不合成一个假 IP。

## 中间件配置

全部集中在 `.env`，读取入口是 `AppConfig::middleware`（`src/infrastructure/config.rs`），
中间件代码里不出现字面量：

| 变量 | 默认值 | 含义 |
| --- | --- | --- |
| `MIDDLEWARE_REQUEST_TIMEOUT_SECS` | `10` | 含排队至响应头产生的超时（秒），408 JSON；不限制响应流发送时间 |
| `MIDDLEWARE_MAX_BODY_BYTES` | `2097152`（2 MiB） | 请求体上限，超限返回 `413` |
| `MIDDLEWARE_MAX_CONCURRENCY` | `256` | 同时在途请求上限 |
| `MIDDLEWARE_BACKPRESSURE_QUEUE` | `256` | 并发已满时允许排队的请求数，超出立即 `503` |
| `MIDDLEWARE_RATE_LIMIT_REQUESTS` | `32768` | 每个限流周期允许的请求数，超出 `429` |
| `MIDDLEWARE_RATE_LIMIT_PERIOD_SECS` | `1` | 限流周期长度（秒） |
| `MIDDLEWARE_HSTS_ENABLED` | 跟随 `APP_ENV` | 生产环境默认下发 `HSTS`，开发环境默认关闭 |
| `MIDDLEWARE_CORS_ALLOWED_ORIGINS` | 空 | 允许跨域的来源白名单，逗号分隔；留空不下发任何 CORS 头，单值 `*` 放行全部来源 |
| `MIDDLEWARE_TRUSTED_PROXIES` | 空 | 显式可信代理 IP/CIDR，逗号分隔；单值 `*` 信任所有直连对端（生产环境拒绝启动，且不得与具体条目混写） |
| `MIDDLEWARE_CLIENT_MAX_CONCURRENCY` | `32` | 单 IP 在途请求（含全局排队）上限 |
| `MIDDLEWARE_CLIENT_RATE_LIMIT_REQUESTS` | `1024` | 单 IP 每窗口请求数上限 |
| `MIDDLEWARE_CLIENT_RATE_LIMIT_PERIOD_SECS` | `1` | 客户端固定窗口长度 |
| `MIDDLEWARE_CLIENT_MAX_ENTRIES` | `16384` | 客户端注册表容量上限 |
| `MIDDLEWARE_CLIENT_IDLE_SECS` | `60` | 空闲保留期，不得小于客户端窗口 |
| `MIDDLEWARE_SLOW_REQUEST_MS` | `1000` | 慢请求阶段汇总阈值（毫秒） |

任一容量参数为 `0` 都会导致启动失败，避免「以为限制了其实没限制」；
`MIDDLEWARE_CORS_ALLOWED_ORIGINS` 的每个条目必须是合法 header 值，否则启动失败。

上表的默认值与 `.env-example` 示例文件一致，适配「单实例 + SQLite/中小型数据库」的中等配置
机器；上线前必须按压测结果调整：

- `MIDDLEWARE_MAX_CONCURRENCY`：由下游能承受的最大在途请求数决定，通常取压测中找到的
  「p99 开始明显劣化」并发点的 80%。
- `MIDDLEWARE_BACKPRESSURE_QUEUE`：吸收突发用，取 `MAX_CONCURRENCY` 的 1/4 ~ 1 倍即可；
  队列越大，被排队的请求尾延迟越高；总超时包含排队，到期取消请求并释放额度。
- `MIDDLEWARE_RATE_LIMIT_REQUESTS` / `PERIOD`：由下游 QPS 容量决定；单实例场景是进程级计数，
  **不是分布式限流**，多实例部署需要换到网关层或基于共享存储的实现。
- `MIDDLEWARE_REQUEST_TIMEOUT_SECS`：取业务正常 p99 的 3~5 倍；过小会误杀慢请求，
  过大则慢请求会长时间占住并发额度。
- `MIDDLEWARE_MAX_BODY_BYTES`：按最大上传接口的实际需求设置，默认 2 MiB 对 JSON API 足够宽裕。
- `MIDDLEWARE_CORS_ALLOWED_ORIGINS`：生产环境应显式列出前端域名，不要用 `*`；
  留空表示服务不供浏览器跨域调用（同源与后端调用不受影响）。

## 部署配置

### 真实来源 IP 与 Nginx 透传

- 用户直接连接服务：自动使用连接来源 IP，不需要配置代理；本机访问看到 `127.0.0.1` 属于正常情况。
- 用户经过 Nginx 等代理：服务直接看到代理的 IP。本机代理通常是 `127.0.0.1`，
  未配置透传与可信代理时，所有经过该代理的用户都会共用代理 IP 的额度。

以「用户直接访问本机 Nginx，Nginx 再连接本服务」为例，在 Nginx 的 `server` 中加入：

```nginx
location / {
    proxy_pass http://127.0.0.1:8080;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-For $remote_addr;
}
```

在本服务的 `.env` 中配置：

```env
MIDDLEWARE_TRUSTED_PROXIES=127.0.0.1/32
```

该示例覆盖用户自带的转发头，避免用户伪造 IP。服务仅信任配置中的代理，默认忽略转发头；
没有转发头时仍使用代理 IP。若 Nginx 在其他机器或容器中，应填写服务实际看到的代理 IP/CIDR，
而不是照抄 `127.0.0.1`。若前面还有 CDN/负载均衡器，需在边缘清除不可信转发头，后续可信
代理正确追加来源，并显式配置可信代理链。不要直接信任所有地址或整个私网：
单值 `*` 只用于本地调试（生产环境会拒绝启动），而不是省事的部署方案。

本示例只说明 IP 透传，不是完整公网 Nginx 配置；TLS、连接保护及阻止绕过网关需另行配置。

### 生产启动守卫

它是一道防止误上线的启动检查，不是登录鉴权或防火墙。开发构建含可直接指定身份生成令牌的
测试登录接口，因此 `APP_ENV=production` 时禁止运行开发构建：程序报错并停止启动。

生产部署先在 `.env` 中设置 `APP_ENV=production`，替换示例 JWT 密钥，再执行：

```bash
cargo build --release
./target/release/axum-service-scaffold
```

发布版本不提供开发登录和开发转账入口。`docs` feature 独立控制 Swagger，默认关闭；
如生产显式启用，必须在网关限制文档访问。

## 生产保障边界

- 生产配置必须运行 release 二进制；main 与 create_app 均拒绝 production + debug，
  防止误暴露可伪造身份的开发登录。docs feature 仍独立，生产网关应限制文档访问。
- 首选网关后单实例：TLS、TCP 连接数、请求头大小/读取超时、空闲连接回收、
  公网 DDoS 与可信代理入口隔离由网关负责。HSTS 只是响应头，不提供 TLS。
- 直接公网部署时本服务仍只有明文 HTTP；应用层请求保护在请求头解析后才开始，
  不能替代 TLS 和连接层慢速攻击防护。未实现这些设施时不能称为安全公网入口。
- 所有限额为进程级；多实例额度随实例数增加，分布式公平性由网关/共享存储承担。
  SQLite 文件不能当成跨机器共享数据库，本次不提供高可用数据库或集群故障转移。
- 408 表示停止等待，不证明外部副作用已撤销；提交与超时竞争时结果可能未知，
  写请求必须使用业务幂等键。不能用 HTTP 超时替代数据库事务回滚或外部调用自身超时。
- 超时和并发额度截至响应头产生，不覆盖 SSE/大文件等长响应发送；独立后台任务
  和不可让出的 CPU 工作也不能由此超时强制终止。本脚手架主要针对有限 JSON 请求。
- 网关应在编排终止宽限期内停止转发并排空连接。存活/就绪端点同样受额度保护，
  不要将公网请求失败等同于进程死亡；探针频率与失败阈值要考虑短时过载。
- 文件日志按时间轮转而非大小轮转；磁盘配额、轮转归档和磁盘满告警须由部署设施保证。

慢接口排查方法见 [日志：低噪声请求诊断](LOGGING.md#低噪声请求诊断)。

## 最小压测方法

先确认并发上限，再确定限速额度。以 [oha](https://github.com/hatoo/oha) 为例：

```bash
cargo install oha

# 单机压测来自同一 IP，应先按压测目标设置客户端额度，不能只放宽全局额度

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
