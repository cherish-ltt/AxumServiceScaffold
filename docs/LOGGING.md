# 日志

> 本文档详解日志格式、分批落盘与容量控制机制，配置总览见 [README](../README.md)。

## 日志格式

`LOG_FORMAT` 控制输出格式（文件层与控制台层同格式）：

- `TEXT`（默认）：单行文本，`时间 级别 span{字段}: target: 消息 字段=值`；
- `JSON`：单行 JSON，事件字段平铺在顶层，span 字段保留在 `span` 对象内。容器场景下 stdout
  才是被采集的流，因此控制台层同样遵循该配置。

```json
{"timestamp":"2026-10-15T17:21:39.00460","level":"INFO","message":"request completed",
 "status":"200","route":"/api/v1/system/health","path":"/api/v1/system/health","method":"GET",
 "latency_ms":0,"target":"axum_service_scaffold::middleware::observability",
 "span":{"client_ip":"127.0.0.1","peer_ip":"127.0.0.1","request_id":"...","method":"GET",
         "path":"/api/v1/system/health","name":"http_request"}}
```

按字段检索的入口：`message`（事件名）、`status` / `route` / `latency_ms`（访问日志）、
`span.request_id`（与响应头 `x-request-id` 一致）、`level`。

**凭据不入日志**：访问日志只记录方法、路径、路由模板、状态码、耗时与来源 IP，
不记录任何请求头（含 `Authorization`、`Cookie`）或请求体；启动配置快照里的
`JWT_SECRET` 与 `METRICS_TOKEN` 只输出字符数，数据库连接串中的口令被替换为 `***`。

## 日志写入：分批落盘

文件日志采用分批写入（`src/logging/batch.rs`）：事件先进入内存缓冲，满 `LOG_BATCH_MAX_EVENTS`
条或每 `LOG_BATCH_FLUSH_INTERVAL_SECS` 秒落盘一次，避免高频场景下逐条写文件浪费 CPU；
控制台层保持实时输出。参数在 `.env` 配置，默认值见下表：

| 变量 | 默认值 | 含义 |
| --- | --- | --- |
| `LOG_BATCH_MAX_EVENTS` | `50` | 缓冲满 N 条触发一次落盘 |
| `LOG_BATCH_FLUSH_INTERVAL_SECS` | `3` | 距上次落盘超过 N 秒强制落盘一次 |
| `LOG_MAX_LOG_FILES` | `60` | 已归档日志的保留数量，超过后删除最旧的 |
| `LOG_MAX_FILE_BYTES` | `67108864`（64 MiB） | 单个日志文件大小上限，超限归档换新文件；`0` 拒绝启动 |
| `LOG_MAX_EVENTS_PER_SEC` | `2000` | 每秒最多落盘的条数，超出部分丢弃并写一条汇总行；`0` 表示不限速 |
| `LOG_OUT_DIR` | `./log` | 日志目录；相对路径以进程工作目录为基准 |

- `LOG_BATCH_MAX_EVENTS`、`LOG_BATCH_FLUSH_INTERVAL_SECS`、`LOG_MAX_LOG_FILES`、
  `LOG_MAX_FILE_BYTES` 任一项设为 `0` 都会导致启动失败（参数必须大于 0）；
- `LOG_MAX_EVENTS_PER_SEC` 的 `0` 是**合法值**，表示关闭限速；
- `LOG_FILTER` 写错（如使用了不存在的日志级别）同样**拒绝启动**，不会静默回退到默认过滤规则；
- 进程退出时（`BatchGuard` drop）会停止定时刷盘线程并落盘剩余缓冲，不会丢日志；
- 每条日志先格式化为完整一行再写入缓冲，保证「条数」计数与日志行一一对应；
- 轮转与清理失败不会让写日志失败（否则会重复写入同一批内容），失败原因由刷盘线程
  在锁外上报一条 `error`。

## 日志量与等级策略

过载时日志本身会成为故障放大器：审计曾观察到「15 秒 46 万条 ERROR」。当前策略：

- 限流（`429`）、背压与排队超时（`503`）属于**预期内的批量拒绝**，不逐条打告警，
  访问日志降级为 `DEBUG`（默认 `LOG_FILTER=info` 下不输出），次数由指标
  `http_rejections_total` 统计；
- 其余 4xx（含 `408` / `413` / `414` / `421` / `401` / `404` / `405`）为 `WARN`；
- 真正的 5xx 保持 `ERROR`；
- 即使如此仍被打爆时，用 `LOG_MAX_EVENTS_PER_SEC` 兜底：超出的条目被丢弃，
  并且**每个窗口写一条汇总行**（如 `WARN log rate limit reached: dropped 812 events`），
  不会静默丢弃；
- 单文件大小上限（`LOG_MAX_FILE_BYTES`）防止持续过载下单个日志文件涨到 GB 级。

详细等级对照见 [中间件栈与配置](MIDDLEWARE.md#拒绝原因与日志级别)。

## 启动配置日志

`cargo run` 启动时会把本次**实际生效**的配置以手绘 ASCII 列表框形式打印出来
（`src/logging/mod.rs` 的 `log_startup_config`），并且同样写入日志文件，便于事后回溯进程启动
那一刻用的参数：

- 框内分为「application / server / database / jwt / logging / middleware / metrics」七组
  （分组名与键均为英文），字段直接取自 `AppConfig`，渲染为 `key=value` 形式；
  `application` 组的 `version` 是例外，取自 `Cargo.toml` 的 `version`
  （`env!("CARGO_PKG_VERSION")`），发版改 `Cargo.toml` 即自动生效；
- `DATABASE_URL` 中的口令被替换为 `***`（`user:password@host` 与 `?password=` 两种写法都支持，
  口令里含 `@` 或 `:` 也不会漏出），`JWT_SECRET` 与 `METRICS_TOKEN` 只输出字符数
  （如 `secret=***(56 chars)`），密钥不会被写进日志；
- 打印时机在日志初始化之后、`Container::bootstrap` 之前（`src/main.rs`），
  因此数据库引导失败时也能看到配置快照。

`.env-example` 是入库的示例文件，其中的容量参数与 [中间件配置](MIDDLEWARE.md#中间件配置) 的
默认值保持一致（`MIDDLEWARE_MAX_CONCURRENCY=256`、`MIDDLEWARE_BACKPRESSURE_QUEUE=256`、
`MIDDLEWARE_RATE_LIMIT_REQUESTS=32768`）；两者不一致时以启动日志打印的实际值为准。
