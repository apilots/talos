# 错误模型

[English](ERROR_MODEL.md)

## 职责归属

`talos-error` crate 负责共享错误分类：

- `ErrorCode` 是稳定、机器可读的错误标识。
- `ApiError` 是结构化公共错误对象。
- `TalosError` 携带内部上下文和错误来源。
- `TalosResult<T>` 是内部 Result 别名。

协议适配器把这些对象映射为 HTTP 或 JSON-RPC envelope。Store、Service、SDK 和 UI
不能定义相互竞争的错误分类。

## 错误链路

```text
Infrastructure or domain failure
              |
              v
TalosError with context and stable ErrorCode
              |
              v
Service boundary returns ApiError-safe information
              |
              +-- HTTP status + JSON response
              |
              +-- JSON-RPC error response
              |
              `-- Timeline TurnError event
```

## 公共契约

公共错误应提供足够信息，使客户端可以确定性处理：

| 字段 | 用途 |
| --- | --- |
| `code` | SDK 和 UI 逻辑使用的稳定 `ErrorCode` |
| `message` | 从 `ErrorCode` 派生的安全英文兜底消息 |

客户端根据 `code` 分支，不能根据本地化 message 文本分支。用户可见本地化属于展示层。
内部 diagnostic、source error 和 caller location 保留在 `TalosError` 中，不序列化给客户端。

## 协议映射

- HTTP 根据 `ErrorCode` 选择状态码，并序列化共享 API error body。
- JSON-RPC 将 JSON-RPC 协议错误与 Talos 领域错误分开。
- Streaming failure 在进入终态前使用 typed Timeline `TurnError` event。
- SDK 暴露结构化错误，不暴露原始 transport response body。

Transport 映射集中在对应 Gateway adapter。Domain Service 不知道 HTTP status code
或 JSON-RPC numeric protocol code。

## 安全规则

- 在确认状态无效的边界尽早失败。
- 传播内部错误前补充操作和资源上下文。
- 为日志和 tracing 保留原始错误来源。
- 禁止暴露 secret、文件系统内部路径、SQL 文本或 Provider credential。
- 禁止把初始化、schema、权限或 transport failure 转成成功。
- 存在稳定且可操作的 code 时，不使用 `InternalError` 兜底。
- 新增 `ErrorCode` 必须进行协议生成以及 SDK/client 兼容性检查。

## 测试

错误测试验证完整公共对象或 envelope，包括稳定 code 和 transport mapping。Bug fix
应增加能够复现原错误链路的测试；除非 wording 本身是公共契约，否则测试不依赖 message 文案。
