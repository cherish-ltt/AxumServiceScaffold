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
- 请求保护的 408、413、429、503 和 panic 500 使用同一响应结构；框架的 400/404/405/413/415/422
  非 JSON 错误统一成稳定 JSON，不回显解析细节；已有业务 JSON 不改写，405 保留 Allow 响应头。
- 响应 x-request-id 可关联服务日志；客户端速率拒绝带 Retry-After（秒），并发拒绝不猜测等待时间。
- 此约定针对应用内错误；网关、HTTP 解析层、TLS 失败和响应流中断不保证有统一 JSON。
