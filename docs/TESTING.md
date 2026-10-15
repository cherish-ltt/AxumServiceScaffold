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
cargo test --release --features docs
# 产物门禁：release 二进制不得包含调试/文档路由字符串（CI 同款）
strings -a target/release/axum-service-scaffold | grep -E "swagger-ui|/api-doc/openapi.json|dev-login|dev-transfer"
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

## 测试文件分工

| 文件 | 覆盖内容 |
| --- | --- |
| `src/**`（`#[cfg(test)]`） | 纯函数与局部逻辑：配置解析与校验、密码哈希、CIDR/HSTS/Host 匹配、限流窗口、令牌定长比较等 |
| `tests/api_tests.rs` | HTTP 接口层黑盒测试（`oneshot`）：状态码、统一响应结构、鉴权、CORS、404/405、URI 超长、Host 白名单、`Cache-Control`、HSTS 组合 |
| `tests/middleware_tests.rs` | 中间件栈：超时、Body 超限、限流、背压、排队等待超时、框架拒绝响应体、开关生效、压缩、CORS |
| `tests/server_tests.rs` | **真实 TCP** 连接层：慢速发头超时、空闲连接回收、请求头超量（431）、连接数上限与许可回收、优雅关闭与关闭上限 |
| `tests/config_tests.rs` | `AppConfig::from_env` 的默认值、覆盖项与全部校验分支（含生产环境门禁） |
| `tests/metrics_tests.rs` | 指标端点鉴权与内容、路由模板标签、拒绝原因标签（独占进程：全局 recorder） |
| `tests/artifact_tests.rs` | 产物暴露门禁：生产配置、release 构建、`docs` feature 组合下调试/文档路由均不可达 |
| `tests/client_ip_tests.rs` | 可信代理链下的真实客户端 IP 解析（真实 TCP + 真实日志，独占进程） |
| `tests/logging_*_tests.rs` | 日志：初始化校验、分批落盘、定时落盘、单文件大小轮转与保留、JSON 可解析与脱敏、`429/503` 等级策略 |

**独占进程的原因**：`tracing` 的全局订阅者、callsite interest 缓存与 `metrics` 的全局 recorder
都是进程级状态，安装一次后无法替换。需要断言日志内容或指标内容的测试必须各占一个测试二进制文件
（Cargo 的 `tests/*.rs` 天然各是一个进程），否则会出现「某些用例单独跑通过、一起跑失败」。

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
