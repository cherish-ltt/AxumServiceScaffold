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

这样可以同时确认：

- 启用 `docs` feature 时 Swagger 与 OpenAPI 文档正常（开发用 `cargo run --features docs`）；
- 发布模式下不会暴露开发登录接口；
- `docs` feature 与 `debug_assertions` 的四种组合都能编译（二者是正交开关，
  release + docs 下调试接口的文档元数据会被自动裁剪）；
- 所有 feature 和 release 构建均可通过；
- 测试和 Clippy 检查通过。

应用启动时通过 `sqlx::migrate!` 执行 `migrations/` 目录下的版本化迁移（记录写进 `_sqlx_migrations` 表），
在此基础上补齐老库缺列、建索引并幂等播种演示账户。生产环境建议在部署阶段执行明确的迁移脚本，
不要使用示例 JWT_SECRET。

回滚这类“写了一半”的行为应当有测试兜底：可参考
`services/transaction.rs` 的单测（在事务内主动失败后断言数据库无残留）与
`tests/api_tests.rs` 中的 `transfer_rolls_back_every_write_when_force_fail_is_on`。

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
  tests/middleware_log_tests.rs 独立进程覆盖低噪声日志、路由模板和 query 不泄漏。
- 覆盖率使用 `cargo llvm-cov --features docs --summary-only`，要求行覆盖率 ≥80%。

新增接口时按同样方式补一条 `oneshot` 用例即可，无需启动服务或 mock 端口。
