# 统一响应结构

> 本文档详解所有带响应体接口的统一返回结构，接口清单见 [README](../README.md)。

所有带响应体的接口共用同一结构：

```json
{
  "code": 200,
  "message": "Success",
  "data": {},
  "timestamp": 1713179523000
}
```

约定：

- `code` 由 HTTP 状态码派生，二者永远一致。`ApiResponse` 内部只保存 `StatusCode` 这一份真值，序列化时才写出 `code`。
- `data` 为空时该字段不会出现在 JSON 中。
- `timestamp` 为 Unix epoch 毫秒（UTC 基准，与时区无关）。
- 需要非 200 语义（例如 `201 Created`）时使用 `ApiResponse::with_status(StatusCode::CREATED, ...)`。
- `204 No Content` 这类不带响应体的状态码不属于 `ApiResponse` 的职责，由 handler 直接返回 `StatusCode`。

## 错误响应

错误响应复用同一结构，只保留 `code` / `message` / `timestamp`，不出现 `data`：

```json
{ "code": 404, "message": "Route not found", "timestamp": 1713179523000 }
```

`message` 是**稳定文案**，不包含数据库原始错误详情（表名、列名、SQL），便于客户端按
`code` 分支、按 `message` 记录日志。

## 框架级错误也走同一结构

以下响应不是业务 handler 产生的，但格式与业务错误一致，客户端不需要为它们写第二套解析：

| 场景 | 状态码 | `message` |
| --- | --- | --- |
| 路由不存在（`fallback`） | `404` | `Route not found` |
| 方法不允许（`method_not_allowed_fallback`，保留 `Allow` 头） | `405` | `Method not allowed` |
| 请求整体超时（`TimeoutLayer`） | `408` | `Request timeout` |
| 请求体超限（`BodyLimitLayer`） | `413` | `Request body too large` |
| URI 超长（`UriLimitLayer`） | `414` | `URI too long` |
| Host 缺失（`HostCheckLayer`） | `400` | `Host header is required` |
| Host 不在白名单（`HostCheckLayer`） | `421` | `Host is not allowed` |
| 背压队列满 / 并发过载 | `503` | `Service is busy, please retry later` |
| 并发门排队超时 | `503` | `Service is busy, please retry later` |
| 限流额度用尽 | `429` | `Too many requests, please retry later` |
| 栈内未识别错误（`HandleErrorLayer`） | `500` | `Internal server error` |

**例外**：HTTP 解析阶段的拒绝（请求头超量、请求头缓冲区超限）由 hyper 在进入中间件栈之前
直接返回 `431`，响应体为空且不受本结构约束——此时请求尚未进入应用逻辑，无法生成结构化响应。
采集端点 `/metrics` 成功时返回 Prometheus 文本格式（`text/plain; version=0.0.4`），
同样不属于本结构。
