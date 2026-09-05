# 更新日志

本项目的所有重要变更都会记录在本文件中。

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本号遵循 [语义化版本 2.0.0](https://semver.org/lang/zh-CN/)。

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
