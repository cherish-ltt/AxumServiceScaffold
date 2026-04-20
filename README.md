# Axum Service Scaffold

一个面向 `axum + sea-orm` 的 Rust 空白脚手架，已经改成参考 `demo` 的洋葱架构组织方式，但保留了当前项目原本的技术选型：

- Web 框架仍然是 `axum`
- ORM 仍然是 `sea-orm`
- 鉴权仍然是 `jsonwebtoken`
- OpenAPI 仍然是 `utoipa + swagger-ui`
- 新增默认密码哈希工具 `argon2`
- 新增并预置 `rayon` 与一组常用库备用

这个仓库的目标不是提供完整业务，而是给你一个可以直接复制的新项目基础骨架。

## 为什么改成洋葱架构

这个脚手架现在采用 4 层加装配层的方式：

```text
api -> services -> domain
         |
         v
   infrastructure
```

对应目录：

```text
src
├─ api                # HTTP 入口层：路由、控制器、DTO、提取器
├─ services           # 用例实现层：组合领域规则与基础设施能力
├─ domain             # 领域内核：模型、错误、接口抽象、常量
├─ infrastructure     # 外部实现：配置、数据库、JWT、仓储适配器
├─ util               # 通用工具：如 Argon2 密码哈希
├─ container.rs       # 依赖装配中心
├─ create_app.rs      # Axum app factory
├─ error.rs           # 把领域错误映射成 HTTP 响应
├─ response.rs        # 统一 API 返回包装
├─ docs.rs            # Swagger 聚合定义
└─ main.rs            # 启动入口
```

### 对比 Java 常见的 `controller -> service -> dao`

很多 Java 项目里的 `controller/service/dao` 在小项目里上手快，但一旦业务变复杂，常见问题是：

- `service` 既管业务规则，又直接拼 ORM 查询，又顺手处理 HTTP DTO
- `dao` 很容易被上层直接拿去用，导致业务规则绕过 service
- controller 层本该只处理输入输出，最后却夹带鉴权、参数转换、异常映射
- 换数据库、换鉴权、接 MQ、接第三方 API 时，修改范围会一路渗透进业务代码

洋葱架构的优势在于依赖方向被强制收紧：

- `domain` 不依赖 `axum`、`sea-orm`、JWT 库这些外部框架
- `services` 只面向领域接口和模型写用例，不把 HTTP 层细节带进去
- `api` 只负责输入输出适配，不承载业务规则
- `infrastructure` 只是外部能力实现，未来可以替换而不改核心业务
- `container.rs` 集中装配依赖，避免到处散落“手动注入”

一句话概括：

- Java 风格三层更像“按职责分文件”
- 洋葱架构更强调“按依赖方向隔离变化”

对于脚手架来说，后者更适合长期扩展，因为你一开始就把“业务核心”和“外部框架”拆开了。

## 当前目录结构

```text
src
├─ api
│  ├─ controllers
│  │  ├─ auth_controller.rs
│  │  ├─ example_controller.rs
│  │  └─ system_controller.rs
│  ├─ dto
│  │  ├─ auth.rs
│  │  ├─ example.rs
│  │  └─ system.rs
│  ├─ extractors
│  │  └─ current_user.rs
│  └─ mod.rs
├─ domain
│  ├─ models
│  │  ├─ auth.rs
│  │  ├─ example.rs
│  │  └─ system.rs
│  ├─ services
│  │  ├─ auth.rs
│  │  ├─ example.rs
│  │  └─ system.rs
│  ├─ constants.rs
│  ├─ error.rs
│  └─ repositories
│     └─ mod.rs
├─ infrastructure
│  ├─ databases
│  │  └─ mod.rs
│  ├─ repositories
│  │  └─ mod.rs
│  ├─ services
│  │  └─ jwt.rs
│  ├─ config.rs
│  └─ mod.rs
├─ services
│  ├─ auth.rs
│  ├─ example.rs
│  └─ system.rs
├─ util
│  └─ password.rs
├─ container.rs
├─ create_app.rs
├─ docs.rs
├─ entities
│  └─ mod.rs
├─ error.rs
├─ lib.rs
├─ logging.rs
├─ main.rs
└─ response.rs
```

## 每层负责什么

### 1. `api`

最外层，处理 HTTP 相关内容：

- `controllers`：接收请求、调用用例、返回统一响应
- `dto`：请求体和响应体
- `extractors`：例如 JWT 当前用户提取

这一层不做核心业务判断，只做协议适配。

### 2. `services`

应用服务层，也就是“用例实现层”。

这一层负责：

- 组织一次完整业务流程
- 调用领域接口
- 调用基础设施能力
- 保持 controller 足够薄

这里可以理解成“真正写业务编排的地方”。

### 3. `domain`

最内层，定义系统核心抽象：

- 领域模型
- 领域错误
- service trait
- repository trait
- 常量

这里不直接依赖 `axum`、`sea-orm`、JWT 或数据库连接。

### 4. `infrastructure`

外部世界的实现层：

- 环境配置读取
- SeaORM 数据库连接
- JWT 签发与校验
- 未来的仓储适配器

这层是最容易变化的地方，所以应该被隔离在外圈。

### 5. `container.rs`

整个项目的依赖装配中心。当前默认装配：

- `SystemUseCase`
- `AuthUseCase`
- `ExampleUseCase`
- `AppConfig`
- `SeaORM DatabaseConnection`
- `JwtService`

后续接 Redis、邮件、对象存储、MQ，也建议继续在这里统一装配。

## 当前已经内置的能力

### 基础设施

- `SeaORM` 数据库连接与连通性检查
- `JWT` 调试登录与当前用户解析
- `Swagger UI`
- `Tracing` 请求日志
- 统一错误响应
- 统一 API 返回结构

### 默认示例接口

- `GET /`
- `GET /api/v1/system/health`
- `GET /api/v1/system/ready`
- `POST /api/v1/auth/dev-login`
- `GET /api/v1/auth/me`
- `POST /api/v1/examples/echo`
- `GET /api/v1/examples`
- `GET /api/v1/examples/{id}`

### 新增工具

- `src/util/password.rs`
  - `hash_password`
  - `verify_password`
- 默认密码算法：`argon2`

示例：

```rust
use axum_service_scaffold::util::password::{hash_password, verify_password};

let hashed = hash_password("S3cure-Password!")?;
let ok = verify_password("S3cure-Password!", &hashed)?;
```

## 预置备用依赖

为了让新项目开箱更快，`Cargo.toml` 里额外预置了几类常用依赖，并且加了中文分类注释：

- `argon2`：密码哈希
- `rayon`：CPU 密集型并行计算备用
- `reqwest`：对外 HTTP 调用
- `validator`：请求参数校验
- `regex`、`base64`、`once_cell`、`rand`：常用工具库

这些库暂时不一定全部在业务里直接使用，但作为脚手架是合理的储备。

## 快速开始

### 1. 准备环境变量

复制模板：

```bash
cp .env-public .env
```

PowerShell：

```powershell
Copy-Item .env-public .env
```

默认模板使用 SQLite：

```env
DATABASE_URL=sqlite://scaffold.db?mode=rwc
```

因此可以在不额外安装 MySQL/PostgreSQL 的前提下直接启动。

### 2. 启动服务

```bash
cargo run
```

默认监听：

- `http://127.0.0.1:8080`

### 3. 打开 Swagger

调试构建下可访问：

- `http://127.0.0.1:8080/swagger-ui`
- `http://127.0.0.1:8080/api-doc/openapi.json`

## 新模块建议怎么扩展

如果你要新增一个真实业务模块，建议按下面的顺序：

1. 先在 `domain/models` 定义领域模型
2. 在 `domain/services` 或 `domain/repositories` 定义抽象接口
3. 在 `services` 写用例实现
4. 如果要落库，在 `infrastructure/repositories` 写 SeaORM 适配器
5. 在 `container.rs` 注入实现
6. 最后在 `api/controllers` 和 `api/dto` 暴露 HTTP 接口

这样做的好处是：

- 先定业务边界，再接具体框架
- 先写抽象，再接外部实现
- 未来替换数据库或拆微服务时，冲击面更小

## SeaORM 放在哪里

这个脚手架没有改掉 SeaORM，而是把它放回更合理的位置：

- 实体统一放在 `src/entities`
- 数据库连接在 `src/infrastructure/databases`
- 未来的仓储实现放在 `src/infrastructure/repositories`

也就是说：

- 不把 ORM 直接塞进 controller
- 不让领域层直接依赖数据库细节
- 但也不牺牲 SeaORM 的使用体验

## 构建检查

建议至少执行：

```bash
cargo fmt
cargo check
cargo check --release
```

这样可以同时确认：

- 调试模式下 Swagger 正常
- 发布模式下也可以干净通过构建

## 备注

当前仓库还是“脚手架”，不是完整业务系统，所以：

- `domain/repositories` 和 `infrastructure/repositories` 目前只保留入口
- 示例模块仍然是演示性质
- 真实项目里你需要按业务补全实体、仓储、事务和测试

但骨架已经调整成更适合长期扩展的形态了。
