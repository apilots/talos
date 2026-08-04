# Talos 统一响应与错误设计

本文定义 Talos 对外应用响应和错误的目标规范。适用范围包括 HTTP、JSON-RPC、SSE timeline event、Rust/TypeScript SDK、Web、TUI 和 CLI。

## 1. 设计结论

Talos 只维护三个共享应用类型：

- `TalosError`：内部错误控制流，只在服务端和 Rust 内部边界使用。
- `ApiError`：唯一公开应用错误对象，所有对外协议复用。
- `ApiResponse<T>`：request-response 场景的统一应用响应。

JSON-RPC 原生 error 是协议错误对象，不是 Talos 应用错误。SSE 不新增 `EventError`；timeline error payload 直接复用 `ApiError`。

所有错误和响应必须由强类型对象生成。禁止使用任意 JSON、map、`params`、`details` 或字符串约定承载错误语义。

## 2. 类型 Owner

类型归属固定如下：

| 类型或能力 | Owner |
| --- | --- |
| `ErrorCode`、`ApiError`、`TalosError`、`TalosResult<T>` | `talos-error` |
| `ApiResponse<T>`、timeline event 类型 | `talos-types` |
| JSON-RPC protocol error 和 JSON-RPC envelope | `talos-app-protocol` |
| TypeScript 和其他非 Rust SDK 公开类型 | protocol/schema generator |
| HTTP status 映射和 Axum response adapter | `talos-gateway-http` |

`talos-error` 不依赖 HTTP、JSON-RPC、SSE、SDK 或 UI。HTTP status 不能定义在 `talos-error` 或 `talos-types` 中。

公开错误类型只有一个来源：

- 服务端和 Rust SDK 直接复用 `talos-error` 中的 `ErrorCode`、`ApiError`。
- `ApiResponse<T>` 和 timeline event 类型由 `talos-types` 统一提供给服务端和 Rust SDK。
- TypeScript 和其他非 Rust SDK 的公开 wire 类型必须由同一份 protocol/schema 生成，不能手写一套 `ErrorCode`、`ApiError` 或 `ApiResponse<T>`。
- protocol/schema generator 只能从上述共享 Rust 类型生成客户端类型，不能定义新的错误语义。

公开转换的实现 owner 固定如下：

| 场景 | 实现 Owner |
| --- | --- |
| HTTP request-response | `talos-gateway-http::server::errors` |
| JSON-RPC application result | `talos-app-server::application::TalosApplicationAdapter` |
| Timeline turn error event | `talos-agent-kernel::timeline` |

`talos-error` 只提供确定性的 `TalosError -> ApiError` 脱敏转换，不记录日志。上述 owner 在转换前记录 `Internal`，并保证同一错误只记录一次。Service 只产出业务响应或 `TalosError`；timeline error event 必须由统一的 timeline construction boundary 构造，Service 不能直接拼装公开 error payload。

## 3. 核心类型

### 3.1 `ApiError`

```rust
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
}
```

- `code` 是稳定、机器可读的应用错误码，wire 使用 `camelCase`。
- `message` 是后端返回的安全英文 fallback，可直接展示，但客户端不能解析它做逻辑判断。
- `message` 由 `ErrorCode` 唯一决定；后端不根据 locale 生成多语言文案，也不允许业务调用点自由覆盖。
- 多语言由前端或客户端根据 `code` 查找国际化模板完成。

`ApiError` 禁止包含：

- 任意 JSON、map、params 或 details；
- source、stack trace、文件路径、依赖错误或程序位置；
- request、trace、session、turn、message 或 event ID。

### 3.2 `ApiResponse<T>`

```rust
#[serde(tag = "status", rename_all = "camelCase")]
pub enum ApiResponse<T> {
    Success {
        data: T,
    },
    Error {
        error: ApiError,
    },
}
```

成功响应：

```json
{
  "status": "success",
  "data": {
    "sessionId": "session-1"
  }
}
```

错误响应：

```json
{
  "status": "error",
  "error": {
    "code": "resourceNotFound",
    "message": "Resource not found"
  }
}
```

约束：

- success 只有 `data`，error 只有 `error`。
- `T` 必须是命名的强类型响应对象，不能是 `serde_json::Value` 或自由 map。
- 无返回字段的操作使用命名空 `XxxResponse`，wire 为 `{}`。
- `ApiResponse<T>` 只用于 request-response，不包裹 timeline stream event。
- HTTP request ID、trace ID 和协议元数据不进入 `ApiResponse<T>`。

### 3.3 `TalosError`

最终内部错误只保留一个公共类型，内部私有表示只保留两个分类：

```rust
pub struct TalosError {
    repr: TalosErrorRepr,
}

enum TalosErrorRepr {
    Public {
        error: ApiError,
        diagnostic: String,
        tool_failure: Option<TalosToolFailureDiagnostic>,
        location: &'static std::panic::Location<'static>,
    },
    Internal {
        context: String,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
        memory_kind: Option<TalosMemoryErrorKind>,
        location: &'static std::panic::Location<'static>,
    },
}
```

- `Public` 表示已经明确、安全、可公开的应用语义。
- `Internal` 保存服务端诊断信息，永远不序列化。
- `TalosToolFailureDiagnostic` 是工具执行层的内部诊断对象，只能通过 `TalosError` accessor 给内部调用方使用，不能进入 `ApiError`、schema 或 wire payload。
- `TalosMemoryErrorKind` 是 memory 层内部诊断对象，只能通过 `TalosError` accessor 给内部调用方使用，不能进入 `ApiError`、schema 或 wire payload。
- `TalosError` 本身不是公开 DTO，不实现公开协议序列化。
- `Public` / `Internal` 不作为下层随意 pattern match 的公开 variant 暴露；调用方通过受控构造器和 `ErrorCode`/内部 accessor 判断稳定语义。
- 内部构造器使用 `#[track_caller]` 捕获 location；调用方不手工填写位置。
- 未明确识别为公开业务语义的错误一律是 `Internal`。

下层仍返回：

```rust
Result<XxxResponse, TalosError>
```

`Result<T, TalosError>` 是内部控制流，用于 `?` 传播和上下文保留，不能被 `ApiResponse<T>` 替代。

下层通过受控构造器创建 `TalosError`，但禁止构造 `ApiResponse<T>`、HTTP status、JSON-RPC error 或 SSE frame。

## 4. 错误分类与公开边界

错误处理分为两个动作：

1. 错误来源负责分类：已知业务失败为 `Public`，未知或程序失败为 `Internal`。
2. 公开边界负责暴露：透传 `Public`，记录并脱敏 `Internal`。

Talos 只有两个公开应用边界：

| 场景 | 公开边界 |
| --- | --- |
| HTTP、JSON-RPC request-response | Gateway application boundary |
| SSE、JSON-RPC subscription timeline event | Timeline public-event construction boundary |

统一转换规则：

```text
TalosError classified as Public(error)
  -> ApiError unchanged

TalosError classified as Internal
  -> log once with available correlation IDs
  -> ApiError {
       code: internalError,
       message: "An internal error occurred"
     }
```

timeline event 构造是独立公开边界，因为事件会被持久化、回放并直接发送给多端。HTTP/SSE encoder 和 JSON-RPC runtime 只负责协议编码，不能再次分类或重复记录同一个错误。

禁止：

- log and rethrow 同一个错误；
- 从 message 或 `to_string()` 推断错误码；
- 把 `Internal` 的 context、source 或 location 复制进 `ApiError`；
- 使用 wildcard match 掩盖新增错误码或旧错误迁移遗漏。

## 5. `ErrorCode` 目录

`ErrorCode` 是 `talos-error` 中的统一 enum。以下目录覆盖当前已公开且对客户端行为、国际化或用户反馈有意义的语义。

| Code | Default Message | HTTP Status |
| --- | --- | --- |
| `invalidRequest` | `Invalid request` | `400` |
| `invalidRetryRequest` | `Invalid retry request` | `400` |
| `unauthenticated` | `Unauthenticated` | `401` |
| `permissionDenied` | `Permission denied` | `403` |
| `resourceNotFound` | `Resource not found` | `404` |
| `sessionNotFound` | `Session not found` | `404` |
| `resourceExpired` | `Resource expired` | `410` |
| `conflict` | `Conflict` | `409` |
| `activeTurnRunning` | `Active turn is already running` | `409` |
| `retrySourceCompleted` | `Retry source is already completed` | `409` |
| `retrySourceMismatch` | `Retry source does not match` | `409` |
| `idempotencyConflict` | `Idempotency conflict` | `409` |
| `invalidCursor` | `Invalid event cursor` | `400` |
| `eventCursorExpired` | `Event cursor expired` | `410` |
| `fsDisabled` | `File system access is disabled` | `403` |
| `invalidPath` | `Invalid path` | `400` |
| `pathOutsideRoot` | `Path is outside the allowed root` | `403` |
| `symlinkNotSupported` | `Symbolic links are not supported` | `400` |
| `pathNotFound` | `Path not found` | `404` |
| `notDirectory` | `Path is not a directory` | `400` |
| `invalidName` | `Invalid name` | `400` |
| `alreadyExists` | `Resource already exists` | `409` |
| `uploadTooLarge` | `Upload is too large` | `413` |
| `unsupportedMediaType` | `Unsupported media type` | `415` |
| `attachmentLimitExceeded` | `Attachment limit exceeded` | `413` |
| `unsupportedAttachment` | `Unsupported attachment` | `400` |
| `fileInputRejected` | `File input was rejected` | `400` |
| `contextWindowExceeded` | `Context window exceeded` | `400` |
| `sessionBudgetExceeded` | `Session budget exceeded` | `429` |
| `usageLimitExceeded` | `Usage limit exceeded` | `429` |
| `contextCompactionFailed` | `Context compaction failed` | `500` |
| `providerUnavailable` | `Provider unavailable` | `503` |
| `toolExecutionFailed` | `Tool execution failed` | `502` |
| `timeout` | `Operation timed out` | `504` |
| `cancelled` | `Operation was cancelled` | `409` |
| `serviceUnavailable` | `Service unavailable` | `503` |
| `internalError` | `An internal error occurred` | `500` |

服务端 `ErrorCode` 是闭合集合。所有公开 code 必须来自服务端 schema 中登记的目录，禁止返回 `unknown`、未登记 code 或客户端本地扩展 code。

客户端 SDK 只接受服务端 schema 中定义的 code。收到未登记 code 表示服务端与客户端协议/schema 不匹配，应作为 protocol/schema decode failure 处理，不做兼容 fallback。

新增 code 只有在客户端需要独立行为或独立国际化时才允许。新增时必须同步默认 message、HTTP status、schema、SDK、前端国际化、测试和本文档。

## 6. Timeline Error Event

保留现有 `TimelineEventEnvelope`、`AgentEvent`、`AgentEventPayload` 和 `timeline.turn.error` 事件名，只修改 turn error payload：

```rust
pub struct TalosTurnErrorEvent {
    pub request_id: String,
    pub turn_id: String,
    pub error: ApiError,
    pub failed_at_ms: u64,
}
```

结构约束：

- `error` 必须是 `ApiError`，不能是裸字符串或自由 JSON。
- 所有 timeline error 必须先成为 `TalosError`，再由 timeline public-event construction boundary 转换为 `ApiError`。
- request、turn、session、message、event 等标识继续留在现有 event/envelope 中，不进入 `ApiError`。

SSE 和 JSON-RPC subscription 继续发送同一个 `TimelineEventEnvelope`，不定义 `EventError`。

## 7. 协议映射

### 7.1 HTTP

- 所有 `/v1` request-response success body 为 `ApiResponse::Success`。
- 所有 `/v1` error body 为 `ApiResponse::Error`。
- HTTP status 由 `talos-gateway-http` 对 `ErrorCode` 做一个穷尽映射。
- auth/lifecycle middleware、route/method rejection、body/query/path extraction 和 handler error 都必须返回统一结构。
- SSE 建连前的认证、路由和参数错误使用 HTTP `ApiResponse::Error`。
- 已建立 SSE stream 后只发送 timeline event；连接断开不是 `ApiError`。
- success 必须带 body；不使用 `204`。无数据操作返回命名空 response。
- `/health`、静态资源和非 API 页面不在 `ApiResponse<T>` 范围内。

### 7.2 JSON-RPC

JSON-RPC protocol error 使用强类型对象，不包含自由 `data`：

```rust
pub struct AppServerErrorBody {
    pub code: AppServerErrorCode,
    pub message: String,
}
```

`AppServerErrorCode` 在 wire 上序列化为固定 numeric code：

| Code | Numeric Code | Default Message |
| --- | --- | --- |
| `ParseError` | `-32700` | `Parse error` |
| `InvalidRequest` | `-32600` | `Invalid Request` |
| `MethodNotFound` | `-32601` | `Method not found` |
| `InvalidParams` | `-32602` | `Invalid params` |
| `InternalError` | `-32603` | `Internal error` |
| `NotInitialized` | `-32001` | `Server not initialized` |
| `AlreadyInitialized` | `-32002` | `Server already initialized` |

除以上 code 外不定义 Talos application numeric code。

以下失败使用原生 JSON-RPC `error` envelope：

- JSON parse 或无效 JSON-RPC envelope/version；
- unknown method；
- params 反序列化失败；
- initialize/not-initialized/already-initialized 状态错误；
- response 无法序列化且连接仍可写。

transport 已无法继续时直接终止连接，由 SDK 作为本地 transport error 处理；此时不存在可发送的服务端错误对象。

只有 JSON-RPC envelope、method 和 params 都成功解码后，才进入 Gateway application boundary：

- success 放入 `result: ApiResponse::Success<XxxResponse>`；
- 认证、授权、业务校验和 Service 失败放入 `result: ApiResponse::Error`；
- JSON-RPC numeric code 与 `ErrorCode` 不互相转换。

`serde_json::Value` 只允许存在于 JSON-RPC envelope 的编码/解码过程；进入 application boundary 前必须解码为强类型 request，result 也必须先由强类型 response 和 `ApiResponse<T>` 构造。

### 7.3 SDK、Web、TUI 和 CLI

- Rust SDK 直接复用 `talos-error` 和 `talos-types` 中的公开应用类型，不重新定义等价 error/response 对象。
- TypeScript 和其他非 Rust SDK 通过 protocol/schema generator 获得 `ErrorCode`、`ApiError`、`ApiResponse<T>` 和 timeline event 类型。
- SDK 解码 `ApiResponse<T>`；application error 保留完整 `ApiError`。
- SDK 自身的 transport/protocol error 使用各语言已有的本地错误机制，不进入服务端 application wire schema。
- SDK 必须让调用方区分 application、transport 和 protocol failure，但本文不规定额外的跨语言 SDK error wire object。
- SDK 的语义固定为 application=`ApiError`、protocol=`AppServerErrorBody`、transport=本地连接错误；具体语言可以使用现有 class、enum 或 rejected promise 包装共享对象。
- TypeScript SDK 如需 `Error` 语义，只能提供一个 `TalosApiError` 运行时包装类；它必须只包装生成的 `ApiError`，不能新增错误字段或错误语义。
- SDK 禁止手写或本地扩展公开 `ErrorCode` 目录。
- timeline consumer 读取 `payload.error.code` 和 `payload.error.message`。
- 客户端通过 `ErrorCode` 做行为判断和国际化，禁止通过 message 分支。

## 8. 国际化、Tracing 与日志

- 后端不选择 locale，不渲染本地化文案。
- 后端 `message` 固定为英文 fallback；客户端通过 `ErrorCode` 查找本地化模板。
- `ApiResponse<T>` 和 `ApiError` 不增加 request ID、trace ID 或业务 ID。
- HTTP correlation 使用 headers/request context；JSON-RPC 使用 request ID/context；timeline 使用现有 envelope/event 标识。
- `Internal` 只在公开边界记录一次，日志包含可用 correlation ID、context、source 和 location。
- 预期内 `Public` 默认不按 error level 记录。
- transport encoder 不重复记录已经转换的应用错误。
- `talos-error` 的脱敏转换不记录日志；log-once 由本规范指定的三个公开转换 owner 负责。

## 9. 实现约束

- `ApiError` 是唯一公开应用错误对象。
- `ApiResponse<T>` 只能在公开 request-response 边界构造。
- timeline public-event construction boundary 可以构造包含 `ApiError` 的事件。
- 业务和 Service 代码只返回 `Result<T, TalosError>`。
- 禁止公开内部 `to_string()`。
- 禁止任意 JSON、map、params、details 和 message-based classification。
- 公开 wire error/response/event 类型必须生成并验证 schema 和 TypeScript 类型；内部 `TalosError` 不生成公开协议类型。
- wire code 使用 `camelCase`，并按 breaking change 同步更新所有消费者。
- 服务端 schema 是公开 `ErrorCode` 的唯一来源；客户端不定义 `Unknown` 或其他兼容 fallback。
