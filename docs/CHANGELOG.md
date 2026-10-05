# 更新日志

本项目的所有重要变更都会记录在本文件中。

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [语义化版本 2.0.0](https://semver.org/lang/zh-CN/)。

## [Unreleased]

### 变更

- 依赖版本升级：`tokio` 1.53.1→1.53.2、`sea-orm` 2.0.3→2.0.4、`uuid` 1.26.1→1.27.0。

### 新增

- **启动配置快照**：`logging::log_startup_config`（`src/logging.rs`）在启动时按
  「应用 / 服务 / 数据库 / JWT / 日志 / 中间件」六组结构化打印本次生效的配置；
  调用点位于 `src/main.rs` 日志初始化之后、`Container::bootstrap` 之前，
  数据库引导失败时也能看到配置。
- 脱敏：`DATABASE_URL` 中的口令被替换为 `***`（同时支持 `user:password@host` 与
  `?password=` 两种写法，口令中含 `@` 或 `:` 也不会漏出）；`JWT_SECRET` 只输出字符数，
  敏感信息不落日志。

### 变更

- `.env-public` 重命名为 `.env-example`（仍为入库的示例文件，用法不变：`cp .env-example .env`），
  避免与「公开」语义混淆。
- 中间件容量默认值与示例文件统一为按实测调整后的值：`MIDDLEWARE_MAX_CONCURRENCY` 256→512、
  `MIDDLEWARE_BACKPRESSURE_QUEUE` 256→512、`MIDDLEWARE_RATE_LIMIT_REQUESTS` 1000→8192；
  未显式配置这些环境变量时，代码兜底默认值与 `.env-example` 完全一致。

### 文档

- README 新增「启动配置日志」小节；「中间件配置」表格的默认值同步为 512 / 512 / 8192，
  并说明默认值与 `.env-example` 保持一致。
- README 新增「架构动效预览」小节：内嵌 `docs/live-panel/axum-service-scaffold.gif`（30 秒循环）
  并链接到 `docs/live-panel/axum-service-scaffold.mp4`，同时说明面板配置（`docs/live-panel/config.json`）
  与示意值范围。GitHub 渲染会剥离手写 `<video>` 标签，仓库相对路径的 mp4 无法内嵌播放，
  故动效用 GIF 内嵌，完整画质用 mp4 点击跳转。

## [0.4.0] - 2026-10-05

集成生产环境可用的 HTTP 中间件栈：请求 ID / Trace / 安全响应头 / 背压 / 限流 / 超时 /
Body 限制 / 压缩，全部由 `tower::ServiceBuilder` 统一装配，参数集中在 `.env`。

### 新增

- **中间件栈（`src/middleware/stack.rs`）**：`middleware::apply(app, &config.middleware)` 用
  `ServiceBuilder` 依次叠加 `SetRequestIdLayer`、`PropagateRequestIdLayer`、`TraceLayer`、
  `SetMultipleResponseHeadersLayer`、背压三件套（`LoadShedLayer` + `BufferLayer` +
  `ConcurrencyLimitLayer`）、限流链（`LoadShedLayer` + `BufferLayer` + `RateLimitLayer`）、
  `CompressionLayer`、`TimeoutLayer`、`RequestBodyLimitLayer`、`CorsLayer`。
- **配置（`MiddlewareConfig`）**：`MIDDLEWARE_REQUEST_TIMEOUT_SECS`（10）、
  `MIDDLEWARE_MAX_BODY_BYTES`（2 MiB）、`MIDDLEWARE_MAX_CONCURRENCY`（512）、
  `MIDDLEWARE_BACKPRESSURE_QUEUE`（512）、`MIDDLEWARE_RATE_LIMIT_REQUESTS`（8192）、
  `MIDDLEWARE_RATE_LIMIT_PERIOD_SECS`（1）、`MIDDLEWARE_HSTS_ENABLED`（默认跟随 `APP_ENV`）；
  任一容量参数为 0 时启动失败。
- **错误语义（`src/middleware/error_response.rs`）**：超时 408、Body 超限 413、
  限流 429、背压/overload 503，均复用 `ApiResponse` 结构。
- **请求 ID 贯穿**：`x-request-id` 缺失时生成 UUID，存在时透传；写入响应头，
  同时作为 `http_request` tracing span 的字段，handler 日志可与响应关联。
- **安全响应头**：`X-Content-Type-Options: nosniff`、`X-Frame-Options: DENY`、
  `Referrer-Policy: strict-origin-when-cross-origin`，生产环境额外下发 `HSTS`。
- **测试**：`tests/middleware_tests.rs` 新增 7 个集成测试（超时 408、Body 413、限流 429、
  背压 503 且不堆积、`request_id` 生成/透传、安全响应头与 HSTS 开关、gzip 压缩）；
  `tests/middleware_log_tests.rs` 单独验证日志关联（`request_id` 出现在 `http_request` span）；
  `tests/config_tests.rs` 新增 3 个配置测试（覆盖项、HSTS 默认值、容量参数为 0）；
  `src/middleware/error_response.rs` 补充错误分支的单元测试（覆盖率 100%）。
  其后共 97 个测试通过，行覆盖率 97.16%（`cargo llvm-cov`）。

### 变更

- `src/create_app.rs`：移除内联的 4 个 layer，改为 `middleware::apply`；
  `create_app` 的签名与调用方式不变。
- **依赖**：`tower` 由 dev-dependencies 提升为正式依赖（`buffer`、`limit`、`load-shed`）；
  `tower-http` 追加 `limit`、`timeout`、`set-header`、`compression-gzip` 特性。
- **配置结构**：`AppConfig` 新增 `middleware` 字段（结构体字面量构造会编译失败，测试已同步）。
- `.env` / `.env-example` 新增 `MIDDLEWARE_*` 配置段。
- 中间件栈包在整个 Router 外层而非 `Router::layer`，避免 `PathRouter::layer` 按路由复制
  有状态层导致全局并发/限流失效。

### 文档

- README 新增「中间件栈」「中间件配置」「最小压测方法」章节。
- AGENTS.md 新增「HTTP 中间件」规范（10.6）。

## [0.3.0] - 2026-09-27

内置「启动事务 → 读写数据 → 提交事务」的完整示例：一次转账在一个事务内读账户、
校验余额、更新双方余额、写流水与审计日志，任一步失败整体回滚；
并演示「写入过程中命中唯一约束冲突 → 跨表写入全部回滚」。

### 新增

- **事务示例（`services/transaction.rs`）**：`TransferService` 是脚手架内唯一的事务边界，
  以 `IsolationLevel::Serializable` 开启事务，成功后 `commit`，任一环节失败则 `rollback`，
  余额更新、流水、审计日志要么一起生效、要么一起消失。
- **唯一约束冲突 → 回滚**：`transfer_records.request_id` 作为幂等键建有唯一索引，
  重复提交时冲突发生在余额更新之后，账户余额、流水、审计日志被整体回滚，
  接口返回 `409 Conflict` 并带出数据库原因，用于演示「约束冲突不会留下半截数据」。
- **实体与建表**：`entities/transfer_account.rs`、`transfer_record.rs`、`transfer_audit.rs`
  三个实体；`infrastructure/databases/schema.rs` 负责建表、建索引与幂等播种两个演示账户
  （`acc_alice`、`acc_bob`，各 100000 分）。
- **克隆即可运行**：数据库文件不入库，首次启动或跑测试时自动建表、建索引并播种演示账户；
  老版本数据库启动时通过 `PRAGMA` 判断并自动补 `request_id` 列与唯一索引。
- **仓储适配器（`infrastructure/repositories/transaction.rs`）**：`TransferRepository`
  的每个方法接收 `C: ConnectionTrait`，传入连接表示不进事务、传入事务即在调用方事务内执行，
  同一套方法可被事务与普通查询复用。
- **HTTP 接口**：`POST /api/v1/transactions/dev-transfer`（调试构建专用，可传 `force_fail`）、
  `GET /api/v1/transactions/{id}`（流水详情与审计日志）、`GET /api/v1/transactions`（分页）。
- **回滚验证**：调试构建下 `force_fail = true` 会在事务内写入全部完成后主动失败，
  用于证明回滚确实撤销了已落库的数据；release 构建不注册该路由、DTO 中也没有该字段。
- **测试**：新增 11 个服务层单测与 9 个 HTTP 集成测试，覆盖提交、余额不足回滚、强制回滚、
  幂等键冲突回滚、老库补列、账户不存在、参数校验、余额溢出、分页与详情查询。
- **文档**：README 增加「事务示例」章节与完整调用示例；AGENTS.md 增加「事务与持久化规范」。

### 变更

- 新增 `AppError::Conflict`（HTTP 409），唯一约束冲突在 `From<DbErr>` 中统一识别并映射，
  不再被当作 500 数据库错误；409 与 400/401/404 一样向客户端透出可读原因。
- 金额统一使用最小货币单位的整数（`amount_cents: i64`），领域层与响应中不出现浮点金额，
  账户表同时维护自增 `version` 用于暴露并发丢失更新。
- `domain/repositories/mod.rs` 移除空的 `Repository` trait：仓储接口需要引用
  `sea_orm::ConnectionTrait`，继续放在 `domain` 会破坏「领域层不依赖 sea-orm」的边界，
  因此仓储接口与实现统一落在 `infrastructure/repositories`。
- 领域模型与用例接口新增 `domain/models/transaction.rs`、`domain/services/transaction.rs`。

### 兼容性

- 既有 `/api/v1/examples/*`、`/api/v1/system/*`、`/api/v1/auth/*` 接口与统一响应结构均未改动。
- 转账请求体的 `request_id` 为可选字段，不传时不参与幂等判定；老数据库启动时自动补列，
  历史流水该列为空，多个空值不触发唯一索引冲突。
- 数据库文件被 `.gitignore` 排除，克隆仓库后无需手工建库，启动与测试会自动完成初始化。
- 依赖版本提升至 `sea-orm 2.0.3`、`utoipa 6.0.0`、`utoipa-swagger-ui 10.0.1`、
  `thiserror 2.0.21`、`rand 0.10.3`、`reqwest 0.13.5`、`uuid 1.26.1`。

## [0.2.0] - 2026-09-22

统一响应结构的状态码真值，消除 HTTP 状态码与响应体 `code` 不一致的可能。

### 变更

- `ApiResponse` 内部改为只保存 `StatusCode`，响应体 `code` 在序列化时由状态码派生，
  HTTP 状态码与 `code` 字段不再可能不一致。
- 移除 `ApiResponse::with_parts(u16, ...)`，新增 `ApiResponse::with_status(StatusCode, ...)`
  作为唯一底层构造入口，并新增 `status()` 只读访问器。
- `ApiResponse` 实现 `IntoResponse`，controller 直接返回 `ApiResponse<T>`，不再手动包裹 `Json`。
- 移除 `ApiResponse` 的 `Deserialize` 实现（脚手架内无生产调用点）。
- README 补充「统一响应结构」约定。

### 兼容性

- JSON 响应结构保持不变：`{code, message, data?, timestamp}`，`data` 为空时仍省略该字段。
- Swagger 文档定义与既有集成测试断言无需调整。

## [0.1.0] - 2026-09-05

首个正式版本：一个面向 `axum + sea-orm` 的 Rust 空白脚手架，采用 DDD + 洋葱架构组织代码，
目标是可以直接复制作为新项目的基础骨架。

### 新增

- **洋葱架构骨架**：`api -> services -> domain -> infrastructure` 四层加装配层，
  依赖方向由外向内，`domain` 不依赖 axum、sea-orm 等外部框架。
- **Web 能力**：`axum 0.8` 路由、统一 API 返回结构 `ApiResponse`、统一错误映射 `AppError`。
- **鉴权**：`jsonwebtoken`（HS256）签发与校验访问令牌；`CurrentUser` 提取器解析
  `Authorization: Bearer <token>`；调试构建提供 `POST /api/v1/auth/dev-login` 调试登录。
- **示例接口**：系统健康检查（`/system/health`、`/system/ready`）、示例回显与分页列表
  （`/api/v1/examples` 系列接口）。
- **OpenAPI 文档**：`utoipa + swagger-ui`，调试构建下挂载 `/swagger-ui` 与
  `/api-doc/openapi.json`。
- **密码工具**：`src/util/password.rs` 提供 `argon2` 哈希与校验（`hash_password` /
  `verify_password`）。
- **基础设施**：`SeaORM` 连接池与连通性检查、`_schema_migrations` 迁移表、
  `JWT` 服务、环境变量配置加载与校验、按天滚动日志（`tracing-appender`）、
  请求 ID 与 Trace 中间件、CORS。
- **优雅关闭**：监听 Ctrl-C / SIGTERM 后停止接收新请求并关闭数据库连接池。
- **配置守卫**：拒绝过短的 `JWT_SECRET`、生产环境示例密钥、非法连接池与日志参数。
- **工程化**：GitHub Actions CI（格式、Clippy、构建、测试）、rustfmt/clippy 配置、
  61 个单元与集成测试，行覆盖率 94% 以上（`cargo llvm-cov`）。
- **预置依赖**：`rayon`、`reqwest`、`validator`、`regex`、`base64`、`once_cell`、
  `rand`、`derive_more`、`itertools` 等常用库按功能分组并锁定版本。
- **双许可**：采用 `MIT OR Apache-2.0` 双许可，文本见 `LICENSE-MIT` 与 `LICENSE-APACHE`。

### 修复

- 为 `jsonwebtoken` 显式启用 `aws_lc_rs` 加密后端，避免 v11 默认配置下
  签发/校验令牌时因缺少 CryptoProvider 而在运行时 panic。

### 说明

- 调试登录接口与 Swagger UI 仅在 debug 构建中提供，release 构建自动移除。
- 生产环境必须替换示例 `JWT_SECRET`，启动时会主动拒绝示例值。
