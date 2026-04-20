# Axum Service Scaffold

一个可直接复用的 Rust Web 服务空白脚手架，面向后续快速创建新的 Axum 项目。

当前脚手架已经集成以下基础设施：

- `axum`：HTTP 服务与路由组织
- `sea-orm`：数据库连接与健康检查
- `jsonwebtoken`：JWT 签发与校验
- `utoipa` + `utoipa-swagger-ui`：调试模式 Swagger 文档
- `tracing` + `tower-http`：日志与请求追踪
- `anyhow` + `thiserror`：启动错误与业务错误分层
- `serde`：请求与响应序列化
- 统一 API 响应结构
- 中文注释与模块化目录布局

## 设计目标

这个分支不是业务项目，而是“可复制的基础骨架”。它强调的是：

- 模块边界清晰
- 运行时代码与文档代码分离
- debug 构建可直接看 Swagger
- release 构建不依赖 Swagger 代码参与业务逻辑
- 新项目可以直接在现有模块上扩展

## 目录结构

```text
src
├─ auth
│  ├─ dto.rs
│  ├─ extractor.rs
│  ├─ handlers.rs
│  ├─ jwt.rs
│  ├─ mod.rs
│  └─ service.rs
├─ modules
│  ├─ example
│  │  ├─ dto.rs
│  │  ├─ handlers.rs
│  │  ├─ mod.rs
│  │  └─ service.rs
│  ├─ system
│  │  ├─ dto.rs
│  │  ├─ handlers.rs
│  │  ├─ mod.rs
│  │  └─ service.rs
│  └─ mod.rs
├─ app.rs
├─ config.rs
├─ db.rs
├─ docs.rs
├─ entities
│  └─ mod.rs
├─ error.rs
├─ lib.rs
├─ logging.rs
├─ main.rs
├─ response.rs
└─ state.rs
```

## 模块说明

### `src/main.rs`

程序入口。只负责：

- 加载环境变量
- 初始化日志
- 构建全局状态
- 启动 HTTP 服务

### `src/config.rs`

负责读取环境变量并构造成类型安全的配置结构。后续你新增缓存、消息队列、对象存储等配置，也建议继续集中放在这里。

### `src/state.rs`

负责组装共享状态，例如：

- 数据库连接
- JWT 服务
- 全局配置

所有 handler 使用的共享资源都应该从这里拿。

### `src/error.rs`

统一业务错误类型与 HTTP 错误响应映射。后续你新增模块时，尽量继续返回 `AppError`，不要在 handler 里到处手写错误响应。

### `src/response.rs`

统一 API 返回结构。建议后续新接口继续保持相同响应风格，避免同一个项目里出现多套返回格式。

### `src/docs.rs`

Swagger/OpenAPI 聚合定义。这个模块仅在 `debug_assertions` 下参与文档生成。

### `src/auth`

鉴权基础设施模块，包含：

- JWT 签发
- JWT 校验
- 当前用户提取器
- 示例登录接口
- 当前用户信息接口

这是后续接 RBAC、会话管理、刷新令牌、多租户等功能的基础位置。

### `src/modules/system`

系统接口模块，用于放：

- 首页
- 健康检查
- 就绪检查
- 版本与运行状态接口

### `src/modules/example`

示例业务模块，不依赖真实业务数据库表，主要用来示范：

- JSON 请求体
- 路径参数
- 查询参数
- JWT 保护接口
- Swagger 注解写法

### `src/entities`

预留给 SeaORM 实体。

建议你后续通过 `sea-orm-cli generate entity` 生成实体后直接放进这个目录，而不是把实体散落到业务模块中。

## 快速开始

### 1. 准备环境变量

复制模板：

```bash
cp .env-public .env
```

Windows PowerShell：

```powershell
Copy-Item .env-public .env
```

默认模板使用 SQLite：

```env
DATABASE_URL=sqlite://scaffold.db?mode=rwc
```

这样无需额外安装数据库即可快速启动。

### 2. 启动开发服务

```bash
cargo run
```

默认监听：

- `http://127.0.0.1:8080`

### 3. 打开 Swagger

调试构建下可访问：

- `http://127.0.0.1:8080/swagger-ui`

OpenAPI JSON：

- `http://127.0.0.1:8080/api-doc/openapi.json`

## 当前内置示例接口

### 系统接口

- `GET /`
- `GET /api/v1/system/health`
- `GET /api/v1/system/ready`

### 鉴权接口

- `POST /api/v1/auth/dev-login`
- `GET /api/v1/auth/me`

其中 `dev-login` 是脚手架示例接口，用于快速签发一个调试 JWT。正式项目中，你通常会把它替换成真正的账号登录逻辑。

### 示例业务接口

- `POST /api/v1/examples/echo`
- `GET /api/v1/examples`
- `GET /api/v1/examples/{id}`

## 推荐扩展方式

新增一个业务模块时，推荐按下面顺序做：

1. 在 `src/modules` 下创建新模块目录
2. 拆成 `dto.rs / handlers.rs / service.rs / mod.rs`
3. 在 `src/modules/mod.rs` 中注册模块
4. 在 `src/app.rs` 中把模块路由合并到主路由
5. 如果需要 Swagger，在 `src/docs.rs` 中注册 path 和 schema
6. 如果需要实体，在 `src/entities` 中新增 SeaORM entity

## 构建检查

每次改完建议至少运行：

```bash
cargo check
cargo check --release
```

这样可以同时保证：

- debug 模式下 Swagger 可用
- release 模式下也能干净通过构建

## 说明

当前脚手架使用 SQLite 只是为了降低启动门槛，并不限制你后续换成 MySQL / PostgreSQL。只要替换 `DATABASE_URL` 并启用对应 SeaORM 驱动能力即可。
