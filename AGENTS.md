# AGENTS.md

本文件定义了本项目的 Rust 开发规范与自动化流程。所有贡献者必须严格遵守，并在每次修改代码后及时更新本文件（如有新增规范或调整）。

---

## 1. Git 提交规范

- **范围**：每次提交应独立且完整地对应一个逻辑变更（如单一功能点、缺陷修复或配置调整），禁止混合多个不相关改动；按功能批次顺序组织，单次提交代码变动量建议控制在 500 行以内(仅建议，非强制，可适当突破)，避免大批量改动挤在同一条提交信息中。
- **格式**：`<type>: <中文描述>`
- **常用 type**：
  - `feat` – 新功能
  - `fix` – 修复 bug
  - `docs` – 文档更新
  - `style` – 代码格式（不影响逻辑）
  - `refactor` – 重构
  - `perf` – 性能优化
  - `test` – 测试相关
  - `build` – 构建系统或外部依赖变更
  - `ci` – CI 配置变更
  - `chore` – 杂项（如工具、配置等）
  - `revert` – 回退提交

示例：`feat: 添加用户登录接口`

---

## 2. Rust CI 标准（GitHub Actions）

确保 `.github/workflows/rust-ci.yml` 存在，内容如下：

```yaml
name: Rust CI

on:
  push:
    branches: [ "main", "master" ]
    paths:
      - "**.rs"
      - "**.proto"
      - "**/Cargo.toml"
      - "**/Cargo.lock"
      - ".rustfmt.toml"
      - ".clippy.toml"
      - "rust-toolchain.toml"
      - ".github/workflows/rust-ci.yml"
  pull_request:
    branches: [ "main", "master" ]
    paths:
      - "**.rs"
      - "**.proto"
      - "**/Cargo.toml"
      - "**/Cargo.lock"
      - ".rustfmt.toml"
      - ".clippy.toml"
      - "rust-toolchain.toml"
      - ".github/workflows/rust-ci.yml"

env:
  CARGO_TERM_COLOR: always

concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true

jobs:
  check:
    name: Check & Test
    runs-on: ubuntu-latest
    steps:
      - name: Checkout repository
        uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@master
        with:
          toolchain: "1.99.0"
          components: rustfmt, clippy

      - name: Show rustup info
        run: rustup show

      - name: Cache Cargo dependencies
        uses: Swatinem/rust-cache@v2

      - name: Check formatting
        run: cargo fmt --all -- --check

      - name: Run Clippy (lints)
        run: cargo clippy --all-targets --features docs -- -D warnings

      - name: Check release + docs combination
        run: cargo check --release --features docs

      - name: Build the project
        run: cargo build --verbose

      - name: Run tests
        run: cargo test --features docs --verbose

      # 真实二进制的门禁验证：debug 构建 + APP_ENV=production 必须拒绝启动
      - name: Verify production refuses a debug build
        run: |
          set -euo pipefail
          if APP_ENV=production HTTP_ALLOWED_HOSTS=api.example.com \
             JWT_SECRET=ci-secret-that-is-long-enough-for-validation \
             METRICS_TOKEN=ci-metrics-token-0123456789 \
             ./target/debug/axum-service-scaffold 2>"$RUNNER_TEMP/production-gate.log"; then
            echo "::error::production with a debug build must refuse to start"
            exit 1
          fi
          grep -q "refusing to start" "$RUNNER_TEMP/production-gate.log"
          echo "production gate verified"

      # 产物层面的门禁：release 二进制里不得存在调试路由与文档路由的字符串。
      # 编译期 cfg 与运行期 APP_ENV 两道门都靠它兜底（任一处回归都会在这里失败）。
      - name: Build release binary
        run: cargo build --release

      - name: Verify the release artifact exposes no debug or docs endpoints
        run: |
          set -euo pipefail
          binary=target/release/axum-service-scaffold
          for needle in "/api-doc/openapi.json" "swagger-ui" "dev-login" "dev-transfer"; do
            if strings -a "$binary" | grep -q -- "$needle"; then
              echo "::error::release artifact must not contain '$needle'"
              exit 1
            fi
          done
          echo "release artifact contains no debug or docs endpoints"

      # 只有 release 构建才会编译 artifact_tests 里的产物断言（debug 下它们被 cfg 裁掉）
      - name: Run release artifact tests
        run: cargo test --release --test artifact_tests --verbose

      - name: Run release tests with the docs feature
        run: cargo test --release --features docs --verbose
```

---

## 3. Cargo.toml 配置

- 必须包含完整的包元数据（满足可发布到 [crates.io](https://crates.io) 的要求），例如：
  - `name`、`version`、`edition`、`authors`、`description`、`license`、`repository` 等。
- 依赖项必须**归类**，使用 `#` 注释说明每组依赖的用途。
- 每个依赖必须使用 `version = "x.y.z"` **锁定具体版本**（使用 `=` 号），不得使用范围限定符。
- 使用 `edition = "2024"` 以及环境中的 Rust 版本，例如:`rust-version = "1.99.0"`

示例结构：

```toml
[package]
name = "my_crate"
version = "0.1.0"
edition = "2024"
rust-version = "1.99.0"
authors = ["Your Name <email@example.com>"]
description = "A short description"
license = "MIT OR Apache-2.0"
repository = "https://github.com/your/repo"

# 核心依赖
[dependencies]
# 序列化
serde = { version = "=1.0.210", features = ["derive"] }

# 异步并发
tokio = { version = "=1.42.0", features = ["full"] }

# 开发依赖
[dev-dependencies]
# 基准测试
criterion = { version = "=0.5.1" }

# 编译优化配置
[profile.dev]
opt-level = 1
[profile.dev.package."*"]
opt-level = 3
```

---

## 4. 代码格式化（.rustfmt.toml）

项目根目录必须包含 `.rustfmt.toml`，内容如下：

```toml
edition = "2024"
max_width = 100
tab_spaces = 4
reorder_imports = true
reorder_modules = true
newline_style = "Auto"
match_block_trailing_comma = true
```

所有代码必须通过 `cargo fmt --all -- --check` 检查。

---

## 5. Clippy 配置（.clippy.toml）

项目根目录必须包含 `.clippy.toml`，内容如下：

```toml
# ── Clippy Configuration ──
# 本文件只配置「lint 的阈值 / 开关」；阈值只有在对应 lint 被启用时才会生效，
# 已启用的 lint 列在 Cargo.toml 的 [lints.clippy]（作用于本包全部 target）。

# clippy::cognitive_complexity 的阈值（已启用）
cognitive-complexity-threshold = 15
# clippy::too_many_arguments 的阈值（complexity 组，clippy 默认即启用）
too-many-arguments-threshold = 5
# clippy::unwrap_used 在测试代码中放行（已启用）
allow-unwrap-in-tests = true

msrv = "1.99.0"
```

启用位置（**必须**与 `.clippy.toml` 的注释保持一致）：

- `src/lib.rs` 顶部的 `#![warn(clippy::cognitive_complexity, clippy::unwrap_used)]` —— 覆盖全部生产代码
  与 `src/` 内的单元测试；`tests/` 集成测试不适用（用例的 setup / 断言天然长而分支多）。
- **只写阈值不启用 lint 等于阈值不生效**：`cognitive_complexity` 属于 nursery、`too_many_lines` 属于
  pedantic、`unwrap_used` 属于 restriction，三者默认都是 `allow`。新增阈值时必须同时在
  `src/lib.rs` 启用对应 lint，否则就是一份“看起来很严”的假配置。
- 故意**不启用**的两个 lint：`too_many_lines`（pedantic，30 行阈值会把配置快照、中间件装配
  这类线性代码全部判为超标，拆分反而降低可读性）与 `expect_used`（restriction，在集成测试的
  辅助函数里噪声过大）。改代码而不是调阈值。

所有代码必须通过 `cargo clippy --all-targets --features docs -- -D warnings` 检查，无警告
（与 CI 一致；`docs` feature 必须打开，否则文档代码不在检查范围内）。`--all-features` 会额外
编译 `preset` / `http-client` 两个可选依赖 bundle，pre-commit 钩子用它做本地兜底。

---

## 6. README.md

- 每次完成任务后需及时更新 `README.md`，至少包含：
  - 项目简介
  - 构建与运行说明
  - 主要功能或使用示例
  - 贡献指南（引用本 AGENTS.md）

---

## 7. .gitignore

必须排除以下内容（示例）：

```
# Rust
/target/
**/*.rs.bk
*.pdb

# macOS
.DS_Store

# IDE
.vscode/
.idea/
*.swp
```

---

## 8. 项目结构

- **使用DDD+洋葱结构，严格遵循此结构开发**
- **禁止单`mod.rs`文件写入大量代码，代码较多时候将代码拆分到更小的带具体名称的代码文件中(如 utils.rs)**
- **保持单代码文件简洁和更细致的 crate 划分以加速增量编译**

---

## 9. 通用原则

- **保持本文件（AGENTS.md）更新**：每次修正代码或引入新规范后，请同步更新此文档。
- **所有变更**必须通过 CI 检查（格式、lint、构建、测试）。
- **版本锁定**：工具链版本统一使用环境中的版本，但需>=1.99.0（如 CI 和 clippy 配置所示）。
- **遵循设计**：改动必须遵循原有结构设计，不得私自添加和修改，除非用户发出明确重构指令。
- **后续开发追加 AGENTS.md 内容**：写入第 10 章节。
- **测试**：编写单元测试，如果已经安装`cargo-llvm-cov`则检测测试覆盖率>=80%。

---

### 10.6 HTTP 中间件

- 所有中间件集中在 `src/middleware/stack.rs`，用 `tower::ServiceBuilder` 一次装配，
  通过 `middleware::apply(app, &config)` 实施；controller 与其他层不得再自行包中间件。
  **唯一例外**是路由模板捕获层（`middleware::route_slot::capture_route_templates`）：它无状态，
  且必须位于路由匹配之内才能读到 `MatchedPath`，因此用 `Router::layer` 逐路由施加；新增此类例外
  必须同时满足「无状态 + 必须在路由匹配之内」两个条件。
- **中间件栈必须包在整个 Router 外层**（`apply` 返回 `Router::new().fallback_service(stack)`）。
  禁止把有状态层（`ConcurrencyLimit`、`RateLimit`）交给 `Router::layer`：`PathRouter::layer`
  会给每条路由复制一份 layer，全局语义会被放大成「每路由一份」。
- `create_app` 的公开签名保持 `pub fn create_app(container: Arc<Container>) -> Router`，
  中间件在函数内部施加，`main.rs` 与集成测试的调用方式不变。
- Tower 的错误类型必须用 `axum::error_handling::HandleErrorLayer` 吸收（axum 要求 `Error = Infallible`），
  处理函数写在与 `stack.rs` 同目录的 `error_response.rs` 中，类型下转用
  `err.is::<tower::load_shed::error::Overloaded>()`。
- 非 `Clone` 的 Tower service（如 `RateLimit`）用 `BufferLayer` 兜住以满足 `axum::serve` /
  `fallback_service` 的 `Clone` 约束。Buffer 只是固定长度队列，克隆共享同一 worker，不增加并发度，
  也不得当作业务任务队列使用。
- 容量参数只允许定义在 `MiddlewareConfig`（`src/infrastructure/config/middleware.rs`）并从 `.env` 读取；
  中间件代码中不得出现字面量。新增参数必须同步 `.env`、`.env-example`、`tests/config_tests.rs`；
  新增开关必须同步 `MiddlewareSwitches` 与「生产环境不得关闭安全关键项」的校验清单。
- 每一层是否装配由 `MiddlewareConfig.switches`（`MIDDLEWARE_*_ENABLED`）控制，用
  `ServiceBuilder::option_layer` 实现；关闭某一层不得改变其余层的顺序与语义。
  `CompressionLayer` 与 `RequestBodyLimit` 会改变请求/响应体类型，无法用 `Option<L>` 表达关闭，
  因此必须常驻栈中并用「配置值表达关闭」（不压缩 / 上限 `usize::MAX`），
  其外侧统一由 `NormalizeBodyLayer` 把响应体收敛回 `Body`。
- 框架层拒绝（`tower-http` 的超时 `408` 空体、`RequestBodyLimit` 的 `413` 纯文本）必须由
  `RejectionBodyLayer`（`src/middleware/rejection.rs`）改写为 `ApiResponse`；
  该层必须位于压缩层之外，且压缩谓词必须跳过 4xx/5xx，否则改写前响应已被压缩。
- 拒绝原因（`reason` 标签）是固定枚举，取值只能在 `src/middleware/error_response.rs` 与
  `src/server/**` 中定义，文档表格见 `docs/MIDDLEWARE.md` 的「拒绝原因与日志级别」，
  新增原因必须同步该表与 `tests/metrics_tests.rs`。
- 代码兜底默认值必须与 `.env-example` 保持一致（当前：并发 256、背压队列 256、限流 32768 req/s、
  超时 10s、Body 2 MiB、周期 1s）；修改任一侧都必须同步另一侧与 `docs/MIDDLEWARE.md` 的「中间件配置」表格。
- 中间件的错误响应必须复用 `ApiResponse::error(status, message)`，与业务错误 `AppError`
  的响应格式保持一致；不要在中间件里手写 JSON。
- 并发限制与限速是两个维度：并发限制约束在途请求数，限速约束单位时间请求数；
  两者均为**进程级**实现，多实例部署需在网关或共享存储层补齐分布式限流。
- **CORS 也必须在栈里**：`CorsLayer` 由 `middleware/stack.rs` 的 `cors_layer(config)` 装配
  （栈的最内层），来源白名单读 `MIDDLEWARE_CORS_ALLOWED_ORIGINS`；禁止在 `create_app`
  或 controller 里直接挂 `CorsLayer::permissive()`，默认必须是不发任何 CORS 头。
- 调整执行顺序、新增或移除中间件时，必须同步更新 `docs/MIDDLEWARE.md` 的「中间件栈」章节与
  `docs/CHANGELOG.md`，并在 `tests/middleware_tests.rs` 补对应断言（至少覆盖超时、Body 超限、
  背压 503、限流 429、`request_id` 关联）。
- 需要断言日志内容或指标内容的测试必须单独成进程（如 `tests/middleware_log_tests.rs`、
  `tests/metrics_tests.rs`）：`tracing` 的
  callsite interest 缓存是进程级的，其他测试线程若在没有订阅者时命中过 tower-http 的 span
  callsite，该 callsite 会被缓存为 `never`，而 `tracing::subscriber::set_global_default` 并不重建
  这个缓存（需显式调用 `tracing::callsite::rebuild_interest_cache()`）。
- 依赖约束：`tower` 必须是正式依赖（`buffer`/`limit`/`load-shed`）；`tower-http` 需包含
  `cors`、`trace`、`request-id`、`limit`、`timeout`、`set-header`、`compression-gzip`。
- 限流（`429`）与背压（`503`）触发时是批量拒绝，逐条告警无信息量：
  `src/middleware/error_response.rs` 不得为这两类拒绝打印逐条 `warn` 日志，
  每次请求（含被拒绝的请求）由 `TraceLayer` 的访问日志（INFO）覆盖；
  `internal()` 等真正的内部错误仍须保留 `error!` 日志。

### 10.7 日志分批写入

- 文件日志采用内存缓冲分批落盘（`src/logging.rs` 的 `BatchFileWriter` / `BatchGuard`）：
  满 `LOG_BATCH_MAX_EVENTS`（默认 50）条或每 `LOG_BATCH_FLUSH_INTERVAL_SECS`（默认 3）秒
  落盘一次，降低高频场景下逐条写文件的 CPU 开销；控制台层保持实时输出，不得加入缓冲。
- 每条日志必须先格式化为完整一行再写入缓冲（`LineFormat` 包一层 `FormatEvent`），
  保证「条数」计数与日志行一一对应，禁止直接计数 fmt 层的分段写入。
- 进程退出时必须停止定时刷盘线程并落盘剩余缓冲（`BatchGuard::drop`），不得丢日志。
- 批量参数属于日志配置（`LoggingConfig`），走 `.env`；新增/调整时必须同步 `.env`、
  `.env-example`、`tests/config_tests.rs`（默认值、覆盖项、0 值校验）、`docs/LOGGING.md` 的
  「日志写入：分批落盘」章节与 `docs/CHANGELOG.md`；任一项为 `0` 拒绝启动。
- 依赖约束：不再依赖 `tracing-appender`（0.2.5 只有时间轮转与文件数量保留，**没有单文件大小
  上限**，持续过载下单文件仍可涨到 GB 级）。轮转由 `src/logging/file_writer.rs` 的
  `LogFileWriter` 自行实现：时间轮转 + 单文件大小上限（`LOG_MAX_FILE_BYTES`）+ 文件数量保留
  （`LOG_MAX_LOG_FILES`）。不得为分批写入引入额外的异步通道/缓冲依赖
  （用 `std::thread` + `std::sync::mpsc` 实现定时刷盘）。
- 轮转/清理失败不得让写入失败（否则会重复写入同一批内容），失败原因记在写入器内，
  由刷盘线程与 `BatchGuard::drop` 在**锁外**上报；不得在 `BatchWriter::write` 里上报
  （会在 tracing 内部重入）。
- `LOG_FORMAT` 支持 `TEXT` / `JSON`；JSON 模式下事件字段平铺、span 字段在 `span` 对象内。
  **两层必须各持一份 span 字段缓存**（`format::LogFields<FileFieldsMarker>` 与
  `<ConsoleFieldsMarker>`）：`tracing` 的 fmt 层把 span 字段格式化结果缓存在按
  `FormatFields` 类型区分的扩展里，两层共用同一类型时会读到对方的缓存（控制台带 ANSI 的文本
  会被 JSON 格式化器当作 JSON 解析而 panic）。字段格式化器必须与事件格式化器配套。
- 日志量必须有上限：`LOG_MAX_EVENTS_PER_SEC` 限制每秒落盘条数（`0` 表示不限速），
  超出的条目被丢弃并**写一条汇总行**，不得静默丢弃。

### 10.8 版本发布标签

- 版本发布必须打 annotated tag（`git tag -a`），message 风格与历史保持一致
  （如 `v0.4.0`/`v0.3.0`）：首行 `vX.Y.Z <一句功能标题>`，随后逐条列出本次主要变更，
  末行 `详见 docs/CHANGELOG.md`。
- 已推送的 tag 如需补充或修正 message，先删除本地与远程 tag 再重打重推
  （`git tag -d` + `git push origin :refs/tags/<tag>`），避免 tag 内容与实际发布不符。

### 10.9 OpenAPI 文档（`docs` feature）

- Swagger UI 与 OpenAPI 文档（`src/docs.rs`、`utoipa`/`utoipa-swagger-ui`）统一由
  `cfg(feature = "docs")`（Cargo 的 `docs` feature）门控，**禁止**回到
  `cfg(debug_assertions)`：`docs` feature 不进 `default`，release 与默认构建均不编译
  utoipa 及其文档代码。开发时用 `cargo run --features docs` 访问 `/swagger-ui` 与
  `/api-doc/openapi.json`。
- 仅调试专用的开发行为（dev-login / dev-transfer 路由注册、DTO 的 `force_fail` 字段
  与 `From` 实现等）仍使用 `cfg(debug_assertions)`，与文档开关相互独立。
- controller / DTO 上的 `#[cfg_attr(debug_assertions, utoipa::path(...))]`、
  `derive(ToSchema/IntoParams)`、`schema(example=...)` 等注解改投 `feature = "docs"`；
  CI 的 clippy 与 test 步骤使用 `--features docs`（见第 2 节），确保文档代码也在检查范围内。
- `system` 响应的 `docs_enabled` 字段跟随 `cfg!(feature = "docs")`。
- **`docs` feature 与 `debug_assertions` 正交，四种组合都必须可编译**：
  `utoipa::path` 仅为实际编译的 handler 生成 `__path_*`，因此仅调试存在的
  handler（`dev_login` / `dev_transfer`）的文档必须放在 `src/docs.rs` 里
  单独 `#[cfg(debug_assertions)]` 的 `DevApiDoc`，再通过
  `OpenApi::merge_from` 合入主文档；禁止直接写进主 `ApiDoc` 的 `paths(...)`
  （release + docs 下会因找不到 `__path_*` 编译失败）。CI 的
  `cargo check --release --features docs` 步骤（见第 2 节）负责守住这条约束。

**本文件是项目的“开发宪法”，所有 pull request 和代码审查均应参照其内容。**

## 10. 其他追加内容

### 10.1 测试组织与覆盖率

- 单元测试直接写在源码文件的 `#[cfg(test)] mod tests` 模块中（如 `src/error.rs`、`src/util/password.rs`）。
- 集成测试放在 `tests/` 目录，按职责拆分文件：
  - `tests/api_tests.rs`：通过 `create_app` + `tower::ServiceExt::oneshot` 覆盖 HTTP 接口层（controllers、DTO、extractor、JWT 鉴权、Swagger 文档、404/405 统一响应体、Host 白名单、URI 超长、安全响应头）。
  - `tests/config_tests.rs`：覆盖 `AppConfig::from_env` 的默认值、覆盖项与全部校验分支（含生产环境启动门禁）。
  - `tests/middleware_tests.rs`：覆盖中间件栈行为（超时、Body 超限、限流、背压、排队等待超时、开关生效、框架拒绝响应体）。
  - `tests/server_tests.rs`：用**真实 TCP** 覆盖连接层（慢速发头超时、空闲连接回收、请求头超量、连接数上限与许可回收、优雅关闭与关闭上限）。
  - `tests/metrics_tests.rs`：覆盖指标端点鉴权与内容、路由模板标签、拒绝原因标签（独占进程）。
  - `tests/artifact_tests.rs`：覆盖产物暴露门禁（生产配置 / release 构建 / `docs` feature 组合）。
  - `tests/client_ip_tests.rs`：覆盖可信代理链下的真实客户端 IP 解析（真实 TCP + 真实日志，独占进程）。
  - `tests/logging_tests.rs` / `tests/logging_batch_tests.rs` / `tests/logging_interval_tests.rs` / `tests/logging_rotation_tests.rs` / `tests/logging_json_tests.rs`：覆盖日志初始化、分批与定时落盘、大小轮转与保留、JSON 可解析与脱敏。
  - `tests/middleware_log_tests.rs` / `tests/middleware_log_level_tests.rs`：覆盖请求 ID 关联与拒绝日志级别策略（须独占进程，原因见 10.6）。
- 集成测试共用 `tests/common/mod.rs` 构造 `AppConfig`：新增配置字段时只改这一处，
  避免各测试文件各自漂移。
- 环境变量是进程级全局状态，依赖环境变量的测试必须通过共享 `Mutex` 串行执行，并在结束后恢复原值。
- 集成测试使用临时 SQLite 文件数据库，每个测试独立装配 `Container`，互不共享状态。
- 覆盖率检测命令：`cargo llvm-cov --summary-only`，要求行覆盖率 ≥ 80%。

### 10.2 jsonwebtoken 加密后端

- `jsonwebtoken` 从 v11 起默认不启用任何加密后端，直接调用签发/校验会在运行时 panic。
- `Cargo.toml` 中必须保持 `jsonwebtoken = { version = "=11", features = ["aws_lc_rs"] }`，与 rustls 使用的 `aws-lc-rs` 后端保持一致。
- 升级 `jsonwebtoken` 时需确认其加密后端特性仍然被显式启用。

### 10.3 双许可协议

- 项目采用 `MIT OR Apache-2.0` 双许可，使用者可任选其一遵循。
- `Cargo.toml` 的 `license` 字段必须保持 `MIT OR Apache-2.0`。
- 许可证文本分文件存放：`LICENSE-MIT`（MIT 全文）与 `LICENSE-APACHE`（Apache-2.0 全文），不要合并进单个 LICENSE 文件。
- README 的「许可证」章节需说明双许可及各自文本文件的位置。

### 10.4 统一响应结构

- 所有带响应体的接口统一返回 `ApiResponse<T>`，JSON 结构固定为 `{code, message, data?, timestamp}`，`data` 为空时省略该字段。
- `code` 由 HTTP 状态码派生，唯一真值是 `ApiResponse` 内部私有的 `status: StatusCode`；禁止引入能独立设置 `code` 的构造入口。
- 需要非 200 语义时使用 `ApiResponse::with_status(StatusCode, message, data)`，不要新增绕过状态码的构造器。
- controller 直接返回 `ApiResponse<T>`（已实现 `IntoResponse`），不要再手动包裹 `Json`。
- `204 No Content` 等不带响应体的状态码不属于 `ApiResponse` 职责，由 handler 直接返回 `StatusCode`。
- `domain` 层不引入 `http::StatusCode`，错误状态码仍以 `AppError::http_code() -> u16` 表达，在外层 `src/error.rs` 才映射为 `StatusCode`。
- 框架级拒绝（`404` / `405` / `408` / `413` / `414` / `421` / `429` / `503`）也必须走
  `ApiResponse`：`404`/`405` 由 `src/api/fallbacks.rs` 提供，中间件拒绝由
  `src/middleware/error_response.rs` 的 `Rejection` 提供，`tower-http` 自身产生的空体 `408`
  与纯文本 `413` 由 `src/middleware/rejection.rs` 的 `RejectionBodyLayer` 改写。
  `431`（请求头超量/缓冲超限）由 hyper 在进入栈之前返回、响应体为空，是唯一例外，
  无需也不应强行为其生成结构化响应。
- 修改响应结构时必须同步更新 `docs/RESPONSE.md` 的「统一响应结构」章节与 `docs/CHANGELOG.md`。

### 10.5 事务与持久化

- **事务边界只允许出现在 `services` 层**：`begin` / `commit` / `rollback` 不得出现在 controller、仓储或 `domain` 中。
- 事务必须显式开启与结束：成功路径显式 `commit`，失败路径显式 `rollback`，禁止依赖隐式提交或静默兜底。
- 回滚失败不得覆盖原始业务错误，只记录 `warn` 日志。
- 仓储方法签名统一接收 `C: ConnectionTrait`：传入 `&DatabaseConnection` 表示不进事务，传入 `&DatabaseTransaction` 表示在调用方事务内执行；禁止为事务单独复制一套方法。
- 仓储接口因引用 `sea_orm::ConnectionTrait` 而归入 `infrastructure/repositories`，`domain/repositories` 只保留边界说明；`domain` 不得依赖 `sea-orm`。
- 含泛型方法的 trait 无法作为 `dyn` 使用，仓储通过泛型参数注入 `services`；对外用例接口（`domain/services`）保持 `dyn` 兼容，由 `container.rs` 装配。
- 读改写场景必须开启写事务：SQLite 不支持在事务内切换隔离级别（`IsolationLevel` 会被
  sea-orm 忽略并打 warn），必须用 `begin_with_options(TransactionOptions { sqlite_transaction_mode: Some(SqliteTransactionMode::Immediate), .. })`；
  金额、数量用整数最小单位表示（如 `amount_cents: i64`），禁止浮点。
- 需要并发控制的表维护自增 `version` 列，更新时以 `WHERE id = ? AND version = <读取时的值>`
  做条件更新并自增 `version`；影响行数为 0 表示期间被其他事务改过，必须返回 `AppError::Conflict`（409），
  不得把 `update_many` 的「0 行」当成成功而静默覆盖。
- 新增实体放在 `src/entities` 下独立文件，并在 `migrations/` 目录新增对应的 sqlx 迁移文件
  （`CREATE TABLE`，由 `databases::run_migrations` 以 `sqlx::migrate!` 执行）；
  种子数据必须幂等（`NOT EXISTS` 或等价写法）。
- **当前只支持 SQLite**：迁移文件与 `schema.rs` 的补列/索引逻辑都是 SQLite 语法
  （`pragma_table_info`）。`DATABASE_URL` 必须在 `AppConfig::from_env` 阶段校验为 `sqlite:` 开头，
  非法 scheme 拒绝启动；要换 MySQL/PostgreSQL 必须同时把迁移与补列逻辑重写成后端无关的写法。
- 每个事务用例至少覆盖「提交成功」与「中途失败回滚后数据无残留」两类测试，回滚测试需断言失败前状态未被改变。
- 唯一约束冲突属于客户端可控冲突，在 `From<DbErr>` 中统一映射为 `AppError::Conflict`（409），
  响应文案用稳定描述（如 `resource already exists`），**不得**把数据库原始 detail（含表名、列名）
  透给客户端，只写 `debug` 日志；也不得当作 500 处理。幂等键一类业务唯一列必须建唯一索引，
  不能只靠先查后写。
- 数据库文件不得入库（`.gitignore` 排除）：建表、索引与种子数据必须由启动流程自动完成且幂等，表结构变更需同时提供老库的自动补齐路径。
- 参考实现：`src/services/transaction.rs`、`src/infrastructure/repositories/transaction.rs`。

### 10.10 连接层防护

- 禁止回到 `axum::serve`：它从不设置 hyper 的 `Timer`，`header_read_timeout` 因此永不生效
  （慢速发头可无限占用连接），也无法限制请求头数量、请求头缓冲与并发连接数。
  服务必须由 `src/server` 接管：`Server::bind` / `run` / `run_until_signal`，
  连接构建器集中在 `src/server/connection.rs`。
- 连接层参数只允许定义在 `ServerConfig`（`src/infrastructure/config/server.rs`）并从 `.env` 读取：
  `SERVER_MAX_CONNECTIONS`、`SERVER_HTTP1_HEADER_READ_TIMEOUT_SECS`、`SERVER_HTTP1_MAX_HEADERS`、
  `SERVER_HTTP1_MAX_BUF_SIZE`、`SHUTDOWN_TIMEOUT_SECS`。新增参数必须同步 `.env`、`.env-example`、
  `tests/config_tests.rs`、`docs/DEPLOYMENT.md` 的「连接层防护」表格。
- `SERVER_HTTP1_MAX_BUF_SIZE` 不得小于 hyper 的内部下限（`MIN_HTTP1_BUF_SIZE = 8192`），
  低于该值 hyper 会直接 panic，必须在配置阶段拒绝启动；`SERVER_HTTP1_HEADER_READ_TIMEOUT_SECS`
  必须显式设置 `Timer`（`TokioTimer`）后才生效，二者是同一个约束的两半。
- 连接级许可（`Semaphore` + `OwnedSemaphorePermit`）与请求级并发限制是两件事：
  连接可以长期不发请求，永远不会进入请求级中间件。超限连接必须被立即关闭并计入
  `http_rejections_total{reason="connection_limit"}`，断开后许可必须归还。
- 连接任务一律集中在 `ConnectionSet`（`src/server/connections.rs`）里，不得游离 spawn：
  关闭时需要 `GracefulShutdown` 排空 + `JoinSet` 回收；**连接任务 panic 必须显式记录 `error!`**，
  不得静默丢弃（当前不引入 `catch_unwind`，panic 只影响该连接，属已知风险项）。
- `SHUTDOWN_TIMEOUT_SECS` 必须小于编排层的终止宽限期（如 Kubernetes
  `terminationGracePeriodSeconds`），否则进程会在落盘最后一批日志前被 SIGKILL。
- 应用只提供 HTTP/1.1：**不得**为 hyper/hyper-util 启用 `http2` feature。
  对外 HTTPS/HTTP2 与按 IP 限流属于反向代理责任，见 `docs/DEPLOYMENT.md`。
- 集成测试中涉及连接生命周期的用例必须走**真实 TCP**（`tests/server_tests.rs`），
  不能用 `oneshot`：`ConnectInfo`、accept 循环与 hyper 解析阶段的拒绝（`431`）在 `oneshot`
  路径上都不存在。

### 10.11 指标与访问日志

- 指标出口为 `src/metrics.rs`，采集端点 `GET /metrics`（`metrics::METRICS_PATH`）与业务接口
  **同端口**暴露，用 `Authorization: Bearer <METRICS_TOKEN>` 鉴权；令牌不匹配返回 `404`
  （不暴露端点存在），`METRICS_ENABLED=false` 时端点完全不注册。`/metrics` 是进程级基础设施
  端点，不得挂到 `/api/v1` 业务前缀下。
- **指标标签只允许使用路由模板**（`MatchedPath`，未匹配路由统一为 `unmatched`）与固定状态码；
  禁止把原始 URI、路径参数、查询串、请求 ID、用户 ID 作为标签（基数会随流量形态爆炸）。
  路由模板经 `RouteSlot`（`route_slot.rs`）从路由匹配层传到栈外的可观测层。
- 全局 recorder 每进程只能安装一次：`MetricsHandle::install` 必须复用 `OnceLock` 缓存的句柄，
  并在「检查并安装」之间持锁，否则并发装配容器会因第二次 `set_global_recorder` 失败而报错。
- 观察到的拒绝必须计入 `http_rejections_total{reason}`，`reason` 为固定枚举；
  指标与日志分工明确：日志负责单请求审计（含请求 ID），指标负责聚合，两者不得互相替代。
- 访问日志的级别策略（`src/middleware/observability.rs`）：
  真正的 5xx 为 `ERROR`；限流 `429` 与背压/排队超时 `503` 为 `DEBUG`（批量拒绝不逐条告警）；
  其余 4xx 为 `WARN`；成功为 `INFO`。请求在应答前被取消（连接中断/进程关闭）时只记
  `reason="cancelled"`，不得以 `status=0` 计入请求总数与延迟直方图。
  策略变更必须同步 `docs/MIDDLEWARE.md` 的「拒绝原因与日志级别」与
  `tests/middleware_log_level_tests.rs`。
- 访问日志不得记录任何请求头（含 `Authorization`、`Cookie`）或请求体；
  启动配置快照中的 `JWT_SECRET` / `METRICS_TOKEN` 只输出字符数，数据库连接串口令必须脱敏
  （见 `tests/logging_json_tests.rs` 的脱敏断言）。

### 10.12 生产环境启动门禁

- `APP_ENV=production` 时 `AppConfig::from_env` 必须执行以下校验，任一项不满足即**拒绝启动**：
  1. 构建画像不是 debug 构建、且未启用 `docs` feature（调试登录接口可伪造任意身份）；
  2. 安全关键中间件全部开启（超时、Body 限制、并发限制、背压、限流、安全响应头、request_id）；
  3. `HTTP_ALLOWED_HOSTS` 非空且不含 `*`；
  4. `JWT_SECRET`、`METRICS_TOKEN` 不是 `.env-example` 中的示例值。
- 两类豁免开关语义不同，不得合并：
  `APP_ALLOW_INSECURE_PRODUCTION` 仅豁免第 1 条（调试与文档路由**仍然不注册**），
  `MIDDLEWARE_ALLOW_INSECURE_PRODUCTION` 仅豁免第 2 条。
- 调试路由（`dev-login` / `dev-transfer`）必须是**双门控**：
  编译期 `#[cfg(debug_assertions)]` + 运行期 `api::debug_routes_enabled(config)`
  （`cfg!(debug_assertions) && !config.is_production()`）。
  只靠编译期隔离不算数——用 `cargo run` 部署到生产就是完整鉴权绕过。
- 文档路由（Swagger UI / OpenAPI）除 `cfg(feature = "docs")` 外，还必须在
  `create_app` 里跳过生产环境；`docs` feature 与 `debug_assertions` 正交，
  四种组合都必须可编译（CI 的 `cargo check --release --features docs` 负责守住）。
- 产物层面的回归由 CI 守住：release 二进制不得包含 `/api-doc/openapi.json`、`swagger-ui`、
  `dev-login`、`dev-transfer` 字符串，并有 release 模式下的 `tests/artifact_tests.rs` 断言。
  新增调试/文档端点时必须同步 `.github/workflows/rust-ci.yml` 的产物检查清单。
- 新增的启动校验必须有 `tests/config_tests.rs` 用例（含「豁免后可通过」与「不豁免即拒绝」两侧），
  并同步 `docs/DEPLOYMENT.md` 的「生产环境启动校验」章节与 `docs/CHANGELOG.md`。
- 部署配套：`deploy/nginx/` 下的反向代理片段与 `docs/DEPLOYMENT.md` 必须与应用侧参数对齐
  （代理的 `client_header_timeout` / `keepalive_timeout` 小于
  `SERVER_HTTP1_HEADER_READ_TIMEOUT_SECS`，`client_max_body_size` 不小于
  `MIDDLEWARE_MAX_BODY_BYTES` 等）；调整任一侧数值都要同步另一侧与文档。
