# 统一响应结构

> 本文档详解所有带响应体接口的统一返回结构，接口清单见 [README](../README.md)。

所有带响应体的接口共用同一结构：

```json
{
  "code": 200,
  "message": "成功",
  "data": {},
  "timestamp": 1713179523000
}
```

约定：

- `code` 由 HTTP 状态码派生，二者永远一致。`ApiResponse` 内部只保存 `StatusCode` 这一份真值，序列化时才写出 `code`。
- `data` 为空时该字段不会出现在 JSON 中。
- `timestamp` 为毫秒级本地时间戳。
- 需要非 200 语义（例如 `201 Created`）时使用 `ApiResponse::with_status(StatusCode::CREATED, ...)`。
- `204 No Content` 这类不带响应体的状态码不属于 `ApiResponse` 的职责，由 handler 直接返回 `StatusCode`。
