# 测试与构建检查

> 本文档详解测试组织方式与构建验证流程，快速上手见 [README](../README.md)。

## 构建检查

建议执行：

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check --all-targets
cargo check --release
cargo test --all-features
```

这样可以同时确认：

- 调试模式下 Swagger 正常；
- 发布模式下不会暴露开发登录接口；
- 所有 feature 和 release 构建均可通过；
- 测试和 Clippy 检查通过。

应用启动时会创建 `_schema_migrations` 表，为后续版本化迁移保留入口；同时创建事务示例的三张表并
幂等播种演示账户。生产环境建议在部署阶段执行明确的迁移脚本，不要使用示例 JWT_SECRET。

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
- 请求体未通过 JSON 提取器时（400/415/422）响应是 axum 生成的纯文本，不属于统一响应结构，
  这类用例只断言状态码。

新增接口时按同样方式补一条 `oneshot` 用例即可，无需启动服务或 mock 端口。
