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
          toolchain: "1.98.1"
          components: rustfmt, clippy

      - name: Show rustup info
        run: rustup show

      - name: Cache Cargo dependencies
        uses: Swatinem/rust-cache@v2

      - name: Check formatting
        run: cargo fmt --all -- --check

      - name: Run Clippy (lints)
        run: cargo clippy --all-targets -- -D warnings

      - name: Build the project
        run: cargo build --verbose

      - name: Run tests
        run: cargo test --verbose

```

---

## 3. Cargo.toml 配置

- 必须包含完整的包元数据（满足可发布到 [crates.io](https://crates.io) 的要求），例如：
  - `name`、`version`、`edition`、`authors`、`description`、`license`、`repository` 等。
- 依赖项必须**归类**，使用 `#` 注释说明每组依赖的用途。
- 每个依赖必须使用 `version = "x.y.z"` **锁定具体版本**（使用 `=` 号），不得使用范围限定符。
- 使用 `edition = "2024"` 以及环境中的 Rust 版本，例如:`rust-version = "1.95"`

示例结构：

```toml
[package]
name = "my_crate"
version = "0.1.0"
edition = "2024"
rust-version = "1.95"
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
cognitive-complexity-threshold = 15
too-many-arguments-threshold = 5
too-many-lines-threshold = 30
allow-unwrap-in-tests = true
msrv = "1.98.1"
```

所有代码必须通过 `cargo clippy --all-targets -- -D warnings` 检查，无警告。

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
- **版本锁定**：工具链版本统一使用环境中的版本，但需>=1.98.1（如 CI 和 clippy 配置所示）。
- **遵循设计**：改动必须遵循原有结构设计，不得私自添加和修改，除非用户发出明确重构指令。
- **后续开发追加 AGENTS.md 内容**：写入第 10 章节。
- **测试**：编写单元测试，如果已经安装`cargo-llvm-cov`则检测测试覆盖率>=80%。

---

### 10.6 HTTP 中间件

- 所有中间件集中在 `src/middleware/stack.rs`，用 `tower::ServiceBuilder` 一次装配，
  通过 `middleware::apply(app, &config.middleware)` 实施；controller 与其他层不得再自行包中间件。
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
- 容量参数只允许定义在 `MiddlewareConfig`（`src/infrastructure/config.rs`）并从 `.env` 读取；
  中间件代码中不得出现字面量。新增参数必须同步 `.env`、`.env-example`、`tests/config_tests.rs`。
- 代码兜底默认值必须与 `.env-example` 保持一致（当前：并发 512、背压队列 512、限流 8192 req/s、
  超时 10s、Body 2 MiB、周期 1s）；修改任一侧都必须同步另一侧与 README「中间件配置」表格。
- 中间件的错误响应必须复用 `ApiResponse::error(status, message)`，与业务错误 `AppError`
  的响应格式保持一致；不要在中间件里手写 JSON。
- 并发限制与限速是两个维度：并发限制约束在途请求数，限速约束单位时间请求数；
  两者均为**进程级**实现，多实例部署需在网关或共享存储层补齐分布式限流。
- 调整执行顺序、新增或移除中间件时，必须同步更新 `README.md` 的「中间件栈」章节与
  `docs/CHANGELOG.md`，并在 `tests/middleware_tests.rs` 补对应断言（至少覆盖超时、Body 超限、
  背压 503、限流 429、`request_id` 关联）。
- 需要断言日志内容的测试必须单独成进程（如 `tests/middleware_log_tests.rs`）：`tracing` 的
  callsite interest 缓存是进程级的，其他测试线程若在没有订阅者时命中过 tower-http 的 span
  callsite，该 callsite 会被缓存为 `never`，而 `tracing::subscriber::set_global_default` 并不重建
  这个缓存（需显式调用 `tracing::callsite::rebuild_interest_cache()`）。
- 依赖约束：`tower` 必须是正式依赖（`buffer`/`limit`/`load-shed`）；`tower-http` 需包含
  `cors`、`trace`、`request-id`、`limit`、`timeout`、`set-header`、`compression-gzip`。

**本文件是项目的“开发宪法”，所有 pull request 和代码审查均应参照其内容。**

## 10. 其他追加内容

### 10.1 测试组织与覆盖率

- 单元测试直接写在源码文件的 `#[cfg(test)] mod tests` 模块中（如 `src/error.rs`、`src/util/password.rs`）。
- 集成测试放在 `tests/` 目录，按职责拆分文件：
  - `tests/api_tests.rs`：通过 `create_app` + `tower::ServiceExt::oneshot` 覆盖 HTTP 接口层（controllers、DTO、extractor、JWT 鉴权、Swagger 文档）。
  - `tests/config_tests.rs`：覆盖 `AppConfig::from_env` 的默认值、覆盖项与全部校验分支。
  - `tests/logging_tests.rs`：覆盖日志初始化与配置校验。
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
- 修改响应结构时必须同步更新 `README.md` 的「统一响应结构」章节与 `docs/CHANGELOG.md`。

### 10.5 事务与持久化

- **事务边界只允许出现在 `services` 层**：`begin` / `commit` / `rollback` 不得出现在 controller、仓储或 `domain` 中。
- 事务必须显式开启与结束：成功路径显式 `commit`，失败路径显式 `rollback`，禁止依赖隐式提交或静默兜底。
- 回滚失败不得覆盖原始业务错误，只记录 `warn` 日志。
- 仓储方法签名统一接收 `C: ConnectionTrait`：传入 `&DatabaseConnection` 表示不进事务，传入 `&DatabaseTransaction` 表示在调用方事务内执行；禁止为事务单独复制一套方法。
- 仓储接口因引用 `sea_orm::ConnectionTrait` 而归入 `infrastructure/repositories`，`domain/repositories` 只保留边界说明；`domain` 不得依赖 `sea-orm`。
- 含泛型方法的 trait 无法作为 `dyn` 使用，仓储通过泛型参数注入 `services`；对外用例接口（`domain/services`）保持 `dyn` 兼容，由 `container.rs` 装配。
- 读改写场景必须开启 `IsolationLevel::Serializable` 事务；金额、数量用整数最小单位表示（如 `amount_cents: i64`），禁止浮点。
- 需要并发控制的表维护自增 `version` 列，更新时一并自增，不做静默覆盖。
- 新增实体放在 `src/entities` 下独立文件，并在 `infrastructure/databases/schema.rs` 补对应的 `CREATE TABLE`；种子数据必须幂等（`NOT EXISTS` 或等价写法）。
- 每个事务用例至少覆盖「提交成功」与「中途失败回滚后数据无残留」两类测试，回滚测试需断言失败前状态未被改变。
- 唯一约束冲突属于客户端可控冲突，在 `From<DbErr>` 中统一映射为 `AppError::Conflict`（409）并透出可读原因，不得当作 500 处理；幂等键一类业务唯一列必须建唯一索引，不能只靠先查后写。
- 数据库文件不得入库（`.gitignore` 排除）：建表、索引与种子数据必须由启动流程自动完成且幂等，表结构变更需同时提供老库的自动补齐路径。
- 参考实现：`src/services/transaction.rs`、`src/infrastructure/repositories/transaction.rs`。
