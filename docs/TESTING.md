# 测试与构建检查

> 本文档详解测试组织方式与构建验证流程，快速上手见 [README](../README.md)。

## 构建检查

建议执行：

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --features docs -- -D warnings
cargo check --all-targets
cargo check --release
cargo check --release --features docs
cargo test --features docs
```

（CI 同款命令见 `.github/workflows/rust-ci.yml`；`default` feature 为空，
`--all-features` 等价于启用 `preset`、`http-client`、`docs` 三个可选 feature，
因此 `--all-features` 会比 CI 多编译一批预置库。）

CLI 进程测试分别验证 debug/release 与 docs 开关的四种组合：

```bash
cargo test --test cli_tests
cargo test --features docs --test cli_tests
cargo test --release --test cli_tests
cargo test --release --features docs --test cli_tests
```

这样可以同时确认：

- 启用 `docs` feature 时 Swagger 与 OpenAPI 文档正常（开发用 `cargo run --features docs`）；
- 发布模式下不会暴露开发登录接口；
- `docs` feature 与 `debug_assertions` 的四种组合都能编译（二者是正交开关，
  release + docs 下调试接口的文档元数据会被自动裁剪）；
- 所有 feature 和 release 构建均可通过；
- 测试和 Clippy 检查通过。

应用启动时通过 `sqlx::migrate!` 执行 `migrations/` 目录下的版本化迁移（记录写进 `_sqlx_migrations` 表），
在此基础上补齐老库缺列、建索引并幂等播种演示账户。部署时可先运行
`cargo run --release -- --init-db`，确认退出码为 0 后再无参数启动服务；普通启动仍自动初始化。
该命令包含演示账户播种，要求完整有效配置且只接受持久 SQLite 数据库，失败不保证全局回滚。
不要使用示例 JWT_SECRET；详细说明见 [CLI 命令](CLI.md)。

回滚这类“写了一半”的行为应当有测试兜底：可参考
`services/transaction.rs` 的单测（在事务内主动失败后断言数据库无残留）与
`tests/api_tests.rs` 中的 `transfer_rolls_back_every_write_when_force_fail_is_on`。

## CLI 进程测试

`tests/cli_tests.rs` 在独立临时目录启动真实二进制，清理业务环境变量并使用隔离的 SQLite 文件，
不读取开发者的 `.env`。它覆盖：

- `--init-db` 新库、老库补列与索引、幂等种子、保留业务数据、内存库与失败退出；服务端口被占用时仍能初始化。
- `--check-config` 与正常启动复用静态拒绝规则，dotenv 与进程环境优先级、密钥不回显，以及既有文件不变。
- `--export-openapi` 在缺失或无效业务配置时输出纯 JSON，与相同构建的 HTTP 文档完全一致；验证 docs
  开关、开发路径裁剪、互斥参数及 Unix 标准输出写入失败。
- 无参数启动的真实 HTTP 健康响应，以及 Unix 下的 Ctrl-C/SIGTERM 优雅关闭。

`tests/database_tests.rs` 同时持有两条连接验证受支持内存库的迁移与种子均可见，并验证非法或私有内存 URL
在连接前返回错误。CLI 测试保留覆盖率运行器的 `LLVM_PROFILE_FILE`，以记录子进程执行路径。

## HTTP 黑盒测试（`oneshot`）

`tests/api_tests.rs` 是接口层的黑盒测试：不直接调用 controller 或 service，
而是用 `tower::ServiceExt::oneshot` 把 `create_app(container)` 产出的 `Router`
当成一次性 `Service` 驱动，直接断言请求进、响应出，无需监听真实端口。

约定：

- 每个测试用 `setup_app()` 独立装配一套 `Container` 与临时 SQLite 文件库，互不共享状态。
- `send()` 返回 `(StatusCode, Value)`，断言同时覆盖 HTTP 状态码与统一响应结构；
  需要检查响应头时用 `send_response()` 取原始 `Response`。
- `Router` 在 `oneshot` 后即被消费，需要多次请求时先 `app.clone()`。
- 黑盒测试锁定的是对外契约：HTTP 状态码、`{code, message, data?, timestamp}` 统一响应结构、
  CORS 预检与 `allow` 响应头，以及 400/401/404/405/409/415/422 等错误语义。
- 请求必须注入真实或测试 ConnectInfo<SocketAddr>；缺少对端时保护层失败关闭，返回 500。
- 框架的 400/404/405/413/415/422 错误也归一化为 JSON，不回显原始解析内容。
- tests/client_protection_tests.rs 验证代理防伪、IP 额度隔离、跨路由共享与取消释放；
  tests/middleware_log_tests.rs 独立进程覆盖低噪声日志、路由模板、query 不泄漏，
  并用真实 SQLite 连接池耗尽验证 db.ready 中断诊断。
- 覆盖率使用 `cargo llvm-cov --features docs --summary-only`，要求行覆盖率 ≥80%。

新增接口时按同样方式补一条 `oneshot` 用例即可，无需启动服务或 mock 端口。
