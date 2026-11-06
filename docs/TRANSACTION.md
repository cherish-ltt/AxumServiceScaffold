# 事务示例

> 本文档详解脚手架内置的「启动事务 → 读写数据 → 提交事务」参考实现，接口清单见 [README](../README.md)。

## 事务示例：启动事务 → 读写数据 → 提交事务

一次转账完整演示了事务的正确用法，是新增业务模块时最值得照抄的部分：

```text
services/transaction.rs
  begin_with_options(sqlite_transaction_mode = IMMEDIATE)
    ├─ 读：查询转出/转入账户余额（事务内）
    ├─ 校验：账户存在、非同一账户、金额 > 0、余额充足
    ├─ 写：扣减转出余额、增加转入余额、写流水、写审计日志
    └─ 任一环节失败 → rollback（以上写入全部撤销）
  commit
```

接口：

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| `POST` | `/api/v1/transactions/dev-transfer` | 执行转账事务，仅调试构建注册 |
| `GET` | `/api/v1/transactions/{id}` | 流水详情 + 同一事务写入的审计日志 |
| `GET` | `/api/v1/transactions` | 可选 `request_id` 精确过滤后分页（`page` 1~10000、`size` 上限 100） |

调试构建下的完整调用流程：

```bash
TOKEN=$(curl -s -X POST http://127.0.0.1:8080/api/v1/auth/dev-login \
  -H 'Content-Type: application/json' \
  -d '{"username":"demo-admin"}' | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["access_token"])')

curl -s -X POST http://127.0.0.1:8080/api/v1/transactions/dev-transfer \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"from_account_id":"acc_alice","to_account_id":"acc_bob","amount_cents":25000,
       "remark":"示例转账","request_id":"req-20260927-0001"}'
```

响应（`data` 部分）：

```json
{
  "record_id": "019680cc-7e1c-7ec0-b7b8-4b4f8e9dff10",
  "request_id": "req-20260927-0001",
  "from_account_id": "acc_alice",
  "to_account_id": "acc_bob",
  "amount_cents": 25000,
  "from_balance_after_cents": 75000,
  "to_balance_after_cents": 125000,
  "committed": true,
  "rolled_back": false
}
```

查询流水与审计日志：

```bash
curl -s "http://127.0.0.1:8080/api/v1/transactions?page=1&size=10" -H "Authorization: Bearer $TOKEN"
curl -s "http://127.0.0.1:8080/api/v1/transactions/<record_id>" -H "Authorization: Bearer $TOKEN"
```

### 响应丢失后按业务请求 ID 查结果

调用方应在提交转账前保存业务 `request_id`。如果写入响应丢失，可使用同一个键查询：

```bash
curl -s --get 'http://127.0.0.1:8080/api/v1/transactions' \
  -H "Authorization: Bearer $TOKEN" \
  --data-urlencode 'request_id=req-20260927-0001' \
  --data-urlencode 'page=1' --data-urlencode 'size=1'
```

`data.total=1` 时，`data.items[0]` 返回已提交的流水、金额、提交后余额和 `record_id`；
再使用现有详情接口查询其审计记录。此查询不修改账户余额、版本、流水或审计。

- `request_id` 按原值等值匹配，区分大小写和前后空格，不做 trim、模糊匹配或格式转换；
  使用 URL 编码传递特殊字符。显式传入空字符串返回统一的 `400`，不等同于未传参数。
- 过滤在分页前执行，`total` 与 `items` 使用相同条件；未知或已回滚的键返回 `total=0` 和
  `items=[]`。超出结果页时仍返回真实 `total`，只是 `items` 为空。
- 查不到记录只说明查询时没有可见的匹配流水，**不能证明原写请求已经完成或已回滚**。
  重复 POST 仍返回 `409` 并回滚重复事务，不会因查询功能改为重放首次成功响应。
- 不传参数时保持原分页行为，包含历史 `request_id=NULL` 流水；现有唯一索引已支持查询，
  本次不新增迁移。接口继续要求 Bearer 鉴权，无效令牌返回 `401`，数据库故障返回安全 `500`。
- 业务键与诊断响应头 `x-request-id` 是两套标识，不会自动互相复制。
  当前示例中的已认证用户共享演示数据，不提供按用户或租户隔离的授权。

Rust 接口兼容性：`TransferQuery` 新增 `request_id: Option<String>`；
`TransferUseCase::list_transfers` 在 `page`、`size` 后新增 `Option<String>`，
`TransferRepository::count_records` 与 `list_records` 末尾新增 `Option<&str>`。
自定义实现和直接调用者需补上参数；不需要过滤时传 `None`。

约定：

- **事务边界只出现在 `services` 层**：`TransferService::transfer` 是唯一调用 `begin` / `commit` / `rollback` 的地方，仓储与 controller 都不开事务。
- **仓储方法接收 `C: ConnectionTrait`**：传入 `&DatabaseConnection` 表示不进事务，传入 `&DatabaseTransaction` 表示在调用方事务内执行，同一套方法两处复用。
- **写入全部完成后才提交**：任何一步返回 `Err` 都会回滚，包括余额不足这类业务错误。
- **金额用整数分**：`amount_cents: i64`，不使用浮点。
- **乐观锁**：账户表用自增 `version` 做条件更新（`WHERE id = ? AND version = <读取时的值>`），
  影响行数为 0 时返回 `409 Conflict`，不做静默覆盖。
- **并发安全**：写事务以 `SqliteTransactionMode::Immediate` 开启（即 `BEGIN IMMEDIATE`），
  写锁在事务开始时就取得，避免 DEFERRED 下「先读余额再扣减」的锁升级冲突窗口。
  注意 `IsolationLevel` 在 SQLite 上**不生效**（sea-orm 只打一条 warn 后忽略），
  控制并发必须用事务模式而不是隔离级别；锁等待时长由 sqlx 默认的
  `busy_timeout = 5s` 提供（`SqliteConnectOptions::new()` 的默认值）。
- **幂等键 `request_id`**：可选。传入时写入 `transfer_records.request_id`（唯一索引），重复提交会命中约束冲突并返回 `409 Conflict`，事务内已完成的余额更新与审计写入一起回滚。

验证回滚是否真的生效（调试构建）：

```bash
# 在事务内写入全部完成后主动失败，接口返回 500
curl -s -X POST http://127.0.0.1:8080/api/v1/transactions/dev-transfer \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"from_account_id":"acc_alice","to_account_id":"acc_bob","amount_cents":30000,"force_fail":true}'

# 流水分页仍为 0，余额也未被改动，说明事务内写入全部被撤销
curl -s "http://127.0.0.1:8080/api/v1/transactions" -H "Authorization: Bearer $TOKEN"
```

`force_fail` 字段与 `/dev-transfer` 路由都只在 debug 构建存在，release 构建下不存在该入口。

验证唯一约束冲突是否会留下半截数据：

```bash
# 首次提交成功，Alice 余额 100000 -> 80000
curl -s -X POST http://127.0.0.1:8080/api/v1/transactions/dev-transfer \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"from_account_id":"acc_alice","to_account_id":"acc_bob","amount_cents":20000,"request_id":"req-dup-1"}'

# 换金额但复用同一个幂等键：约束冲突发生在余额更新之后，接口返回 409
curl -s -X POST http://127.0.0.1:8080/api/v1/transactions/dev-transfer \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"from_account_id":"acc_alice","to_account_id":"acc_bob","amount_cents":5000,"request_id":"req-dup-1"}'

# 流水仍只有 1 条，Alice 余额仍是 80000：第二次的写入被整体回滚
curl -s "http://127.0.0.1:8080/api/v1/transactions" -H "Authorization: Bearer $TOKEN"
```

这也是「跨表写入必须放在一个事务里」的直接证据：约束冲突不会只回滚失败的那一步。
