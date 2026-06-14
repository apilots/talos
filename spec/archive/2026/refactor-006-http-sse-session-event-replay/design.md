# HTTP SSE Session Event Replay Design

## 1. 背景

当前 Talos 的 HTTP SSE 能支持单次消息流式返回，但它本质上仍绑定在：

```text
POST /v1/sessions/{session_id}/messages
```

也就是说，客户端提交消息后拿到一条临时 SSE 响应流。该模型对“当前页面持续在线”可用，但对 webui 刷新、网络断开、TUI 重连、未来桌面端多窗口观察都不够。

现状中的关键问题：

1. `TalosEvent.sequence` 只在单次 stream 内递增，不是 session 级 cursor。
2. 服务端没有持久化 stream event log，断线后无法补齐中间事件。
3. webui 断线后只能轮询 session/turns，无法继续按执行状态增量渲染。
4. TUI 断线后只能结束当前 stream 或重新发起消息，存在重复提交风险。
5. 运行中工具状态、pending approval、tool progress 等中间态无法通过历史 turns 完整恢复。
6. 当前 `Idempotency-Key` 只覆盖非 stream 内存缓存，不能支撑刷新重试后的同一 turn 去重。

本文目标是先基于 HTTP SSE 完善服务端能力，不引入 gRPC。后续 desktop/appserver 可以在同一套事件存储与事件总线上增加 gRPC transport。

---

## 2. 目标

1. 服务端提供 session 级事件订阅接口，支持从 cursor 增量获取事件。
2. 客户端刷新页面后可以继续订阅同一 session，并补齐断线期间事件。
3. 运行中的 turn 可以被多个客户端观察，不要求重新提交消息。
4. 工具执行状态、approval 等中间态必须可重放。
5. 历史 turns 仍作为完成态历史，event replay 负责运行态和增量状态。
6. HTTP API 保持浏览器友好，webui 优先接入。

---

## 3. 非目标

1. 本文不设计 gRPC 接口。
2. 本文不引入 WebSocket。
3. 本文不重构 planner/kernel 执行策略。
4. 本文不解决所有历史 transcript 兼容问题。
5. 本文不允许客户端通过重发 message 来恢复断线中的 turn。
6. 本文不修改 `Gateway` trait，不新增 `Gateway::submit_async` 一类入口。

---

## 4. 核心判断

### 4.1 当前 SSE 问题不是 transport 本身

HTTP SSE 可以继续使用。真正缺失的是：

- session 级稳定事件 ID
- durable event log
- subscribe/replay API
- active turn 状态查询
- 客户端恢复协议

因此短期应先补服务端事件模型，而不是直接切换到 gRPC。

### 4.2 `messages stream` 与 `events stream` 必须解耦

当前：

```text
submit message -> response body carries stream events -> stream closed
```

目标：

```text
submit turn -> returns turn_id/message_id ack
subscribe events -> receives durable session events
```

`POST /messages` 不再提供 response-body stream 或同步 `TalosMessageResponse` 路径。webui/tui/desktop 统一通过 ack + `/events` 观察执行过程，不做旧客户端兼容。

重要约束：

1. 不扩展 `Gateway` trait。HTTP 层继续复用现有 `handle_request_stream`。
2. 新的 async ack 是 HTTP gateway 本地适配层能力，不向 `talos_gateway::Gateway` 扩散。
3. 事件持久化发生在 HTTP stream forwarder 中：HTTP handler 获取现有 stream receiver 后，把 receiver 中的 `TalosEvent` 全量写入 `SessionEventService.record_and_publish`。
4. 项目当前阶段不考虑旧客户端兼容，迁移完成后要删除废弃 API、旧 cache 和双轨逻辑。

---

## 5. 总体方案

### 5.1 新增 Event Plane

保留现有 Command Plane：

```text
POST   /v1/sessions
POST   /v1/sessions/{session_id}/messages
POST   /v1/sessions/{session_id}/tool-approvals
POST   /v1/sessions/{session_id}/dynamic-tool-results
POST   /v1/sessions/{session_id}/cancel
GET    /v1/sessions/{session_id}/turns
```

新增 Event Plane：

```text
GET    /v1/sessions/{session_id}/events?after_event_id={event_id}
GET    /v1/sessions/{session_id}/active-turn
```

推荐客户端流程：

```text
1. 创建或打开 session
2. 订阅 /events?after_event_id=last_seen_event_id
3. 发送 message，拿到 ack
4. 所有执行状态都从 /events 增量渲染
5. 刷新或断线后继续用 last_seen_event_id 订阅
```

### 5.2 ASCII 流程图

```text
┌────────┐        POST /messages         ┌──────────────┐
│ webui  │ ────────────────────────────> │ HTTP gateway │
└───┬────┘                               └──────┬───────┘
    │                                           │
    │         202 / message ack                 │
    │ <─────────────────────────────────────────┘
    │                                           │
    │ GET /events?after_event_id=ev_102         │
    │ ─────────────────────────────────────────>│
    │                                           │
    │                         ┌─────────────────▼──────────────┐
    │                         │ SessionEventService             │
    │                         │ - replay events after cursor     │
    │                         │ - attach live subscriber         │
    │                         └─────────────────┬──────────────┘
    │                                           │
    │                         ┌─────────────────▼──────────────┐
    │                         │ SessionEventStore               │
    │                         │ durable event log per session    │
    │                         └─────────────────┬──────────────┘
    │                                           │
    │ SSE: ev_103, ev_104, ...                  │
    │ <─────────────────────────────────────────┘
    │
    │ browser refresh
    │
    │ GET /events?after_event_id=ev_140
    │ ─────────────────────────────────────────> replay gap + live tail
```

---

## 6. 数据模型

### 6.1 SessionEventRecord

新增强类型事件记录，不使用灵活 metadata json。

```rust
pub struct SessionEventRecord {
    pub event_seq: i64,
    pub event_id: SessionEventId,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub message_id: String,
    pub sequence: u64,
    pub event_type: TalosEventType,
    pub payload: TalosEventPayload,
    pub created_at: i64,
}
```

约束：

1. `event_seq` 是唯一可信 cursor，由数据库自增序列生成，必须全局严格单调。
2. `event_id` 是对外暴露的稳定字符串，由 `event_seq` 派生，例如 `ev_00000000000000000123`。禁止使用 UUIDv7 作为 replay 比较水位。
3. 所有 replay 查询必须先把 `after_event_id` 解析为 `event_seq`，再用 `event_seq` 比较和排序。
4. `sequence` 是同一 turn/message 内顺序，保留现有语义，只用于渲染去重和调试，不作为恢复 cursor。
5. `payload` 继续复用 `TalosEventPayload` enum。
6. `turn_id` 由 HTTP 层为本次提交生成，用于把事件与完成态 turn 关联。
7. `MessageCompleted` 和 `MessageError` 都是 turn 级终止事件。客户端收到任一事件后，应把当前 active turn 标记为终态；SSE 连接可以继续保持以等待后续 turn。

### 6.2 ActiveTurnState

用于刷新后快速判断当前 session 是否仍有执行中任务。

```rust
pub enum ActiveTurnStatus {
    Running,
    WaitingUserAction,
    Cancelling,
    Completed,
    Failed,
}

pub struct ActiveTurnState {
    pub session_id: String,
    pub turn_id: String,
    pub message_id: String,
    pub status: ActiveTurnStatus,
    pub last_event_id: Option<SessionEventId>,
    pub pending_user_action: Option<PendingUserAction>,
    pub updated_at: i64,
}
```

状态约束：

1. `Running` 可以带 `pending_user_action = None`。
2. `WaitingUserAction` 必须带 `pending_user_action = Some(...)`。
3. 进入 `Cancelling` 时必须把 `pending_user_action` 置为 `None`，UI 只按 `status` 渲染取消中状态。
4. `Completed` / `Failed` 必须带 `pending_user_action = None`。

`PendingUserAction` 合并 tool approval 和 dynamic tool result 两类阻塞状态：

```rust
pub enum PendingUserAction {
    ToolApproval(PendingToolApproval),
    DynamicToolResult(PendingDynamicToolResult),
}

pub struct PendingToolApproval {
    pub call_id: String,
    pub tool_name: String,
    pub prompt: String,
    pub arguments: serde_json::Value,
}

pub struct PendingDynamicToolResult {
    pub call_id: String,
    pub tool_name: String,
}
```

说明：`arguments` 目前工具参数仍是 JSON value，后续若工具 schema 强类型化，再同步收敛。

---

## 7. HTTP API 设计

### 7.1 提交消息

新增推荐形态：

```text
POST /v1/sessions/{session_id}/messages
```

请求：

```json
{
  "input": [
    { "type": "text", "text": "..." }
  ]
}
```

响应改为启动执行并获得首个事件后返回 message ack：

```json
{
  "request_id": "req_...",
  "message_id": "msg_...",
  "session_id": "sess_...",
  "turn_id": "turn_...",
  "status": "accepted"
}
```

要求：

1. 不阻塞等待模型完成。
2. 不依赖 response body 承载执行事件。
3. 支持持久化 `Idempotency-Key`，避免刷新或重试造成重复 turn。

当前实现直接移除旧同步响应和 `stream` 请求字段，webui/tui 新链路必须切到 ack + `/events`。

幂等要求：

1. 当前 `messages.rs` 中的内存 `idempotency_cache` 不满足 async ack 要求，因为它不持久、不覆盖 stream 路径、重启后丢失。
2. async ack 模式必须新增持久化 `message_idempotency_keys` 表，唯一键为 `(session_id, idempotency_key)`。
3. 重复 POST 且 request hash 相同，必须返回同一个 `turn_id` / `message_id` / `status`。
4. 重复 POST 但 request hash 不同，必须返回 `409 Conflict`，禁止复用旧 turn。
5. 第一阶段若只支持单实例，也必须把幂等记录落到 SQLite；不能用内存 HashMap 代替。

HTTP 层复用 Gateway 的执行方式：

```text
1. validate request and reserve idempotency key
2. call state.gateway.handle_request_stream(gateway_request)
3. wait for the first TalosEvent to learn message_id
4. persist the first event through SessionEventService
5. finalize and return ack with turn_id/message_id
6. spawn a forwarder task for the remaining receiver events
```

说明：

1. `MessageCommandHandler` 是 HTTP server 内部 handler，不是新的 `Gateway` trait 方法。
2. 第一条事件等待时间由 `SessionEventLimitsConfig.first_event_timeout_ms` 控制，默认 `5000ms`。
3. 如果第一条事件超时，必须调用 `Gateway::cancel_active_turn(session_id)`，启动 drain forwarder 消费并落库后续 terminal/error event，然后写入 failed idempotency 状态并返回明确错误。
4. 首事件超时后不允许留下孤儿 turn；如果 cancel 失败，也必须写入 `MessageError` 事件并把 active turn 标记为 `Failed`。
5. HTTP 层生成 `turn_id`，并在持久化事件时附加到 `SessionEventRecord`；底层 `TalosEvent` 不需要新增 `turn_id` 字段。

### 7.2 订阅事件

```text
GET /v1/sessions/{session_id}/events?after_event_id=ev_...
Accept: text/event-stream
```

行为：

1. 先从 `SessionEventStore` 读取 `after_event_id` 之后的事件并按序发送。
2. replay 完成后挂到 live subscriber。
3. 新事件到达后继续发送。
4. session 当前无 active turn 时，可以保持连接一段时间或发送 heartbeat。

SSE event 格式：

```text
id: ev_...
event: message.output.delta
data: {"event_seq":123,"event_id":"ev_...","session_id":"...","message_id":"...","sequence":12,"payload":{...}}
```

要求：

1. 必须设置 SSE `id`，让浏览器和 SDK 能记录 last event id。
2. 服务端同时支持 query 参数 `after_event_id`，不能只依赖浏览器 `Last-Event-ID` header。
3. 如果 query 参数和 `Last-Event-ID` header 同时存在，以 query 参数为准。
4. replay 与 live tail 之间不能丢事件，需要在同一服务内完成 cursor 边界控制。
5. 如果 `after_event_id` 已因 GC 不存在，返回 `410 Gone`，客户端必须走 `/turns` + `/active-turn` 全量恢复，禁止静默从最新事件开始。
6. 单次 replay 必须有 batch limit，超过 limit 后继续分批发送，不能一次读取无限事件。
7. 分批是服务端 flush 边界，客户端无需 ack；客户端只需按 `event_seq` 单调递增校验。

浏览器鉴权约束：

1. webui 不使用原生 `EventSource`，因为原生 API 不能携带 `Authorization` header。
2. webui 继续使用 fetch-based SSE adapter，例如当前 `@microsoft/fetch-event-source`，通过 fetch header 携带 bearer token。
3. 如果后续需要原生 `EventSource`，必须新增短期 query token 或 cookie + same-site 方案，并确保 access log 中 token 被 redact。

### 7.3 查询 active turn

```text
GET /v1/sessions/{session_id}/active-turn
```

响应：

```json
{
  "session_id": "sess_...",
  "turn_id": "turn_...",
  "message_id": "msg_...",
  "status": "running",
  "last_event_id": "ev_...",
  "pending_user_action": null,
  "updated_at": 1790000000000
}
```

没有 active turn：

```json
{
  "session_id": "sess_...",
  "turn_id": null,
  "status": "completed",
  "last_event_id": "ev_...",
  "pending_user_action": null,
  "updated_at": 1790000000000
}
```

用途：

1. webui 首次进入 session 时判断是否需要自动恢复订阅。
2. TUI 断线后判断当前 turn 是否仍在运行。
3. 工具审批浮层刷新后能恢复。

### 7.4 业务 retry attempt

```text
POST /v1/sessions/{session_id}/message-attempts
```

请求：

```json
{
  "client_message_id": "client_...",
  "retry_of_message_id": "msg_...",
  "input": [
    { "type": "text", "text": "..." }
  ],
  "workspace_id": null
}
```

语义：

1. 该接口只表示“用户明确要求重新执行同一条失败回答”，不用于网络断线恢复；断线恢复仍优先 `/events` + `/active-turn`。
2. 服务端必须用 `client_message_id + retry_of_message_id` 派生稳定 `Idempotency-Key`，即使客户端传入不同 header，也不能导致第二次启动 kernel。
3. 如果原 turn 仍为 `Running` / `WaitingUserAction` / `Cancelling`，返回 `409 retry_active_turn_running`，客户端应继续订阅原事件流。
4. 如果原 turn 已 `Completed`，返回 `409 retry_source_completed`，客户端应刷新历史，不允许重复执行成功回答。
5. 仅当 active turn 为 `Failed` 且 `message_id == retry_of_message_id` 时允许启动新 attempt；重复 POST 必须返回同一个 ack。
6. WebUI retry 前必须清理本地 live blocks，再以同一用户输入开启新 attempt，避免旧失败残留和新回答混在一起。

---

## 8. 服务端模块设计

### 8.0 分层约束

新增 event 持久化栈必须遵守 API → Service → Store 分层，不复用历史上 handler 直连 storage 或业务状态散落在 HTTP 层的写法。

分层职责：

| 层级 | 责任 | 禁止事项 |
|---|---|---|
| API / Handler | HTTP 路由、鉴权 actor 提取、请求/响应 DTO、SSE 编码、状态码映射 | 禁止直接写 SQL；禁止直接维护 active turn 状态；禁止实现 replay 水位算法 |
| Access Service | 复用 `ResourceAccessService` 做 session 可见性、owner/admin 校验 | 禁止绕过现有 session 访问控制 |
| Event Service | 事件状态机、record-and-publish、replay/live gap 控制、active turn 转换、幂等状态流转、GC 调度 | 禁止拼 HTTP response；禁止依赖 axum 类型 |
| Store | SQLite schema、事务、cursor 查询、幂等记录、active turn 记录、GC 删除 | 禁止包含 HTTP 语义；禁止调用 gateway/kernel |

依赖方向只能是：

```text
HTTP handler
  -> ResourceAccessService
  -> SessionEventService
  -> SessionEventStore / ActiveTurnStore / MessageIdempotencyStore
  -> Storage
```

`Gateway::handle_request_stream` 的调用属于 HTTP command handler 的适配逻辑，但事件持久化和状态转换必须委托给 `SessionEventService`，不能在 handler 内散写 store。

### 8.1 模块接口清单

| 模块 | 类型 | 关键接口 | 说明 |
|---|---|---|---|
| `SessionEventStore` | trait | `append_event`, `list_events_after`, `latest_event_id` | durable event log |
| `SessionEventBus` | struct/trait | `publish`, `subscribe` | live subscriber fanout |
| `SessionEventService` | struct | `record_and_publish`, `replay_then_subscribe` | 统一 replay + live tail |
| `ActiveTurnStore` | trait | `upsert`, `get`, `clear` | 保存运行态 turn |
| `HttpEventHandler` | handler | `subscribe_session_events` | HTTP SSE adapter |
| `MessageCommandHandler` | handler | `submit_message` | HTTP 层提交 turn，不新增 Gateway trait |
| `MessageIdempotencyStore` | trait | `reserve`, `complete`, `get` | 持久化提交幂等 |
| `SessionEventAccessService` | service/helper | `ensure_session_event_access` | 复用 session 可见性校验 |

建议文件划分，避免单文件超过 1000 行：

| 文件 | 责任 |
|---|---|
| `server/events.rs` | HTTP SSE handler 和 response mapping |
| `server/message_commands.rs` | async ack message handler 和 stream forwarder |
| `server/idempotency.rs` | HTTP 层 idempotency store adapter |
| `server/event_access.rs` | HTTP actor 到 event service 的访问控制适配 |
| `crates/orchestration/session-store/src/events.rs` | `SessionEventStore` trait |
| `crates/orchestration/session-store/src/idempotency.rs` | `MessageIdempotencyStore` trait |
| `crates/orchestration/session-store/src/active_turn.rs` | `ActiveTurnStore` trait |
| `crates/orchestration/coordinator/src/event_service.rs` | `SessionEventService` |
| `crates/orchestration/coordinator/src/event_bus.rs` | 单实例 live fanout |

### 8.2 核心 API 伪代码

```rust
#[async_trait]
pub trait SessionEventStore: Send + Sync {
    async fn append_event(&self, event: NewSessionEvent) -> TalosResult<SessionEventRecord>;

    async fn list_events_after(
        &self,
        session_id: &str,
        after_event_id: Option<&SessionEventId>,
        limit: usize,
    ) -> TalosResult<Vec<SessionEventRecord>>;

    async fn latest_event_id(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<SessionEventId>>;

    async fn resolve_event_seq(
        &self,
        session_id: &str,
        event_id: &SessionEventId,
    ) -> TalosResult<Option<i64>>;
}
```

```rust
#[async_trait]
pub trait SessionEventBus: Send + Sync {
    async fn publish(&self, record: SessionEventRecord) -> TalosResult<()>;

    async fn subscribe(&self, session_id: &str) -> TalosResult<SessionEventReceiver>;
}
```

```rust
#[async_trait]
pub trait MessageIdempotencyStore: Send + Sync {
    async fn reserve(
        &self,
        session_id: &str,
        key: &str,
        request_hash: &str,
        turn_id: &str,
    ) -> TalosResult<IdempotencyReservation>;

    async fn complete(
        &self,
        session_id: &str,
        key: &str,
        message_id: &str,
        ack: &MessageCommandAck,
    ) -> TalosResult<()>;
}
```

```rust
pub struct SessionEventService {
    event_store: Arc<dyn SessionEventStore>,
    event_bus: Arc<dyn SessionEventBus>,
    active_turn_store: Arc<dyn ActiveTurnStore>,
}

impl SessionEventService {
    pub async fn record_and_publish(
        &self,
        event: NewSessionEvent,
    ) -> TalosResult<SessionEventRecord> {
        let record = self.event_store.append_event(event).await?;
        self.event_bus.publish(record.clone()).await?;
        Ok(record)
    }

    pub async fn replay_then_subscribe(
        &self,
        session_id: &str,
        after_event_id: Option<SessionEventId>,
    ) -> TalosResult<SessionReplayStream> {
        // Implementation must prevent the replay/live gap.
        todo!()
    }
}
```

第一阶段 `SessionEventBus` 明确只支持单实例进程内 fanout，可用 `tokio::sync::broadcast` 或 per-session broadcast map 实现。多实例部署需要数据库 notification、storage polling 或外部 pub/sub，本文不在第一阶段实现；实现前必须保持单实例部署约束。

### 8.3 replay/live gap 处理

不能使用以下不安全顺序：

```text
1. read historical events
2. subscribe live bus
```

因为第 1 步和第 2 步之间可能产生新事件并丢失。

推荐顺序：

```text
1. subscribe live bus first
2. resolve after_event_id to after_event_seq
3. read historical events where event_seq > after_event_seq
4. send historical events ordered by event_seq asc
5. drain buffered live events whose event_seq is greater than last sent event_seq
5. continue live tail
```

水位规则：

1. 只比较 `event_seq`，不比较 `event_id` 字符串。
2. 如果 historical replay 最后一条事件为 `event_seq = 150`，live buffer 中 `event_seq <= 150` 的事件必须丢弃，`event_seq > 150` 的事件继续发送。
3. 如果 `after_event_id` 无法解析且 session 仍存在，返回 `410 Gone`。

---

## 9. 与现有执行链路的集成

当前事件大致从：

```text
KernelStreamEvent -> OrchestratorEvent -> TalosEvent -> HTTP SSE
```

调整为：

```text
KernelStreamEvent
  -> OrchestratorEvent
  -> TalosEvent
  -> existing Gateway::handle_request_stream receiver
  -> HTTP stream forwarder
  -> SessionEventService.record_and_publish
  -> HTTP /events subscribers
```

`POST /messages` 的 async ack 路径只在 HTTP 层编排，不修改 `Gateway` trait：

```text
MessageCommandHandler
  -> validate request
  -> reserve persistent idempotency key
  -> create turn_id
  -> call Gateway::handle_request_stream
  -> wait for first TalosEvent to obtain message_id
  -> persist first event
  -> active_turn_store.upsert(Running)
  -> spawn forwarder task for remaining events
  -> return ack
```

HTTP handler 必须经过同一个 forwarder，把 Gateway receiver 的事件写入 event log 并发布给 `/events` 订阅者。提交请求本身只负责返回 ack，不再承担事件响应体。

执行完成：

```text
MessageCompleted
  -> append event
  -> persist canonical turn
  -> active_turn_store.clear or mark Completed
  -> update session progress
```

执行失败：

```text
MessageError
  -> append event
  -> active_turn_store.mark Failed
  -> complete idempotency record with failed terminal status
  -> do not silently swallow error
```

等待用户动作：

```text
MessageToolCallPendingApproval
  -> append event
  -> active_turn_store.upsert(WaitingUserAction::ToolApproval)

Dynamic tool pending event
  -> append event
  -> active_turn_store.upsert(WaitingUserAction::DynamicToolResult)
```

如果当前内核还没有显式 dynamic tool pending stream event，应先补齐该事件，否则 webui 刷新后无法准确恢复等待外部结果的 UI。该前置工作纳入阶段 1。

---

## 10. WebUI 恢复协议

### 10.1 正常发送

```text
1. Ensure event subscription is open.
2. POST /messages.
3. Render all status from /events.
4. Persist last_seen_event_id locally per session.
```

### 10.2 页面刷新

```text
1. Load session snapshot.
2. Load turns for completed history.
3. Read last_seen_event_id from local store.
4. GET /active-turn.
5. GET /events?after_event_id=last_seen_event_id.
6. Replay missing events into stream state.
7. Continue live rendering if active turn is running, cancelling, or waiting user action.
```

### 10.3 无 last_seen_event_id

如果本地没有 cursor：

1. 先加载 `/turns` 渲染完成态历史。
2. 再读取 `/active-turn`。
3. 若有 active turn，从 active turn 的 `last_event_id` 不足以重建状态，则服务端应允许按 `turn_id` 查询当前 turn 的事件：

```text
GET /v1/sessions/{session_id}/events?turn_id=turn_...
```

该查询只用于恢复当前运行 turn，不替代 session cursor。

约束：

1. `turn_id` 查询只允许 active turn。
2. 如果 `turn_id` 已 Completed 或 Failed，服务端返回 `409 Conflict`，客户端必须走 `/turns`。
3. 如果 `turn_id` 不存在或不属于该 session，服务端返回 `404 Not Found`。
4. `turn_id` 查询返回后，客户端仍应记录最后一个 `event_id`，后续恢复回到 `after_event_id` 协议。

---

## 11. 数据存储策略

### 11.1 第一阶段建议

优先使用现有 SQLite storage 新增事件表：

```sql
CREATE TABLE session_events (
  event_seq INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  turn_id TEXT,
  message_id TEXT NOT NULL,
  sequence INTEGER NOT NULL,
  event_type TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE INDEX idx_session_events_session_seq
  ON session_events(session_id, event_seq);

CREATE INDEX idx_session_events_turn
  ON session_events(session_id, turn_id);
```

说明：

1. `payload_json` 是 enum payload 的序列化，不是开放 metadata。
2. `event_seq` 是唯一排序和 cursor 比较依据。
3. 数据库不存 `event_id`，对外序列化时由 `event_seq` 派生固定宽度字符串，例如 `ev_00000000000000000123`。
4. `after_event_id` 解析时反向提取 `event_seq`，解析失败返回 `400 Bad Request`；已过期返回 `410 Gone`。
5. 后续如果 storage 抽象支持 typed column，可再进一步拆分。
6. 事件表是 replay source，canonical turns 是完成态历史 source，两者职责不同。

新增幂等表：

```sql
CREATE TABLE message_idempotency_keys (
  session_id TEXT NOT NULL,
  idempotency_key TEXT NOT NULL,
  request_hash TEXT NOT NULL,
  turn_id TEXT NOT NULL,
  message_id TEXT,
  status TEXT NOT NULL,
  ack_json TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  expires_at INTEGER,
  PRIMARY KEY (session_id, idempotency_key)
);
```

幂等 TTL 与 GC：

1. 默认 TTL 为 24 小时，进入 config 模块，例如 `message_idempotency_ttl_hours: u32 = 24`。
2. 幂等 GC 复用 session event retention 清理任务，但作为独立步骤执行。
3. 过期 key 再次 POST 时按新请求处理，但必须记录 `expired_idempotency_key_hit` 日志，便于排查重复执行。
4. 未过期 key 且状态为 failed terminal 时，同 key 同 hash 重试必须返回原 failed ack，不重新执行 turn。
5. 未过期 key 且状态为 running / accepted 时，同 key 同 hash 重试返回当前 ack 或可观察状态，不启动第二个 turn。
6. `pending` 状态记录如果超过 `first_event_timeout_ms * 2` 仍没有关联首事件，GC 必须标记为 `failed_no_first_event`；同 key 同 hash 后续重试返回该 failed 终态，不重新执行 turn。
7. `reserve` 必须使用 `INSERT ... ON CONFLICT DO NOTHING` 等价语义。发生冲突的 caller 必须读取已有记录并按 request hash / status 返回结果，禁止第二次启动 kernel。

新增 active turn 表：

```sql
CREATE TABLE active_turns (
  session_id TEXT PRIMARY KEY,
  turn_id TEXT NOT NULL,
  message_id TEXT,
  status TEXT NOT NULL,
  last_event_id TEXT,
  pending_user_action_json TEXT,
  updated_at INTEGER NOT NULL
);
```

事务要求：

1. 首个事件 append、idempotency complete、active turn upsert 应在同一 storage 事务中完成。
2. `MessageCompleted` / `MessageError` append 和 active turn terminal update 应在同一 storage 事务中完成。
3. 存储抽象必须支持事务内写入并回读 `event_seq`；否则无法生成对外 `event_id` 和 ack，必须先补 storage transaction API。
4. 如果 storage 当前 trait 不支持事务，不能用多次独立写入假装原子。

### 11.2 Event Log GC

event log 必须有 GC，不能无限保留。它服务于断线恢复和运行态增量渲染，不是长期历史真源；长期历史由 canonical turns JSON 承担。

新增配置：

```rust
pub struct SessionEventRetentionConfig {
    pub retention_hours: u32,
    pub max_events_per_session: usize,
    pub gc_interval_minutes: u32,
    pub gc_batch_size: usize,
}
```

默认值：

```text
retention_hours = 24
max_events_per_session = 10000
gc_interval_minutes = 30
gc_batch_size = 1000
```

GC 规则：

1. event log 只保留最近一段时间的数据，默认 24 小时。
2. 单 session 事件数超过 `max_events_per_session` 时，也必须按 `event_seq` 从旧到新删除。
3. 清理前必须保证 completed turn 已持久化，否则不能删除该 turn 的事件。
4. active turn 的事件禁止删除，即使超过时间或数量限制。
5. GC 按 batch 删除，避免一次清理阻塞 storage。
6. GC 由服务端后台 maintenance task 定期执行，启动时也应执行一次轻量清理。

GC 与恢复规则：

1. GC 只能删除已完成 turn 对应的旧事件，不能删除 active turn 事件。
2. 如果客户端用已删除的 `after_event_id` 恢复，服务端必须返回 `410 Gone`。
3. `410 Gone` 响应应包含可机器识别错误码，例如 `event_cursor_expired`，客户端据此走 `/turns` + `/active-turn` 全量恢复。
4. 不能静默从最新事件开始，否则会破坏增量恢复契约。

session 删除规则：

1. `delete_session_record` 成功时必须级联删除 `session_events`、`active_turns`、`message_idempotency_keys`。
2. 如果底层 SQLite schema 没有 FK 强约束，删除责任必须在 session store 的 delete 实现里显式完成。

### 11.3 事件大小与分页

新增配置：

```rust
pub struct SessionEventLimitsConfig {
    pub first_event_timeout_ms: u64,
    pub max_event_payload_bytes: usize,
    pub replay_batch_size: usize,
    pub max_replay_bytes_per_flush: usize,
}
```

规则：

1. `first_event_timeout_ms` 默认值为 `5000`，统一进入 config 模块，不允许在 handler 中硬编码。
2. tool progress / stdout / stderr 这类大 payload 应在产生时拆成多个 delta event。
3. 单个 event 超过 `max_event_payload_bytes` 时，服务端应返回明确错误事件，不能写入不可 replay 的超大事件。
4. replay 按 `replay_batch_size` 和 `max_replay_bytes_per_flush` 分批发送，避免一次重连拉取几十 MB。
5. 被截断或拆分的事件必须保持顺序，客户端不能靠字符串拼接之外的隐式规则恢复状态。

---

## 12. 迁移与清理

### 12.1 推荐迁移顺序

1. 新增 event store / event service，并让 HTTP 层从现有 `Gateway::handle_request_stream` receiver 写入 event store。
2. 新增 `/events` 和 `/active-turn`。
3. webui 新增 `/events` 订阅和刷新恢复，提交请求只接收 ack。
4. 新增 HTTP async ack 模式和持久幂等表。
5. webui 切换到 ack + events。
6. TUI 接入同一套 events。
7. 删除 response-stream、同步 `POST /messages` 响应路径、内存 `idempotency_cache` 和旧客户端兼容测试。

### 12.2 必须清理的旧路径

迁移完成后必须删除：

1. `POST /v1/sessions/{id}/messages` 中返回 `TalosMessageResponse` 的同步阻塞路径。
2. `TalosMessageRequest.stream` 请求字段和 HTTP response body SSE 分支。
3. SDK 中直接从 `POST /messages` body 解析 SSE 的旧实现。
4. webui/tui 中依赖提交请求 response body 事件的状态机。
5. `messages.rs` 内存 `idempotency_cache`。
6. 仅为旧 stream response 保留的 e2e 测试。

删除标准：

1. webui 刷新恢复、TUI remote 恢复、HTTP replay e2e 全部通过。
2. 没有调用方继续传 `stream` 请求字段。
3. `POST /messages` 只返回 ack，执行状态只通过 `/events` 观察。

---

## 13. 测试计划

### 13.1 单元测试

1. `SessionEventStore` 按 cursor 返回增量事件。
2. `SessionEventService` replay 后继续 live tail，不丢事件。
3. `ActiveTurnStore` 在 running、waiting user action、cancelling、completed、failed 状态间转换正确。
4. `TalosEventPayload` 序列化/反序列化保持兼容。

### 13.2 HTTP 集成测试

新增 e2e 场景：

```text
talos-e2e/scenarios/03_messages_http_sse_replay.sh
```

覆盖：

1. 提交 stream turn。
2. 消费前 N 个 SSE event 后主动断开。
3. 使用 `after_event_id` 重连。
4. 断言后续 event 从 N+1 开始，不重复、不丢失。
5. 断言最终收到 `message.completed`。
6. 长时间断开超过 heartbeat 周期后重连，仍能补齐事件。
7. 多个 subscriber 同时订阅同一 active turn，均收到相同终止事件。
8. replay 过程中服务端产生新事件，断言 replay/live 水位切换不丢不重。
9. pending approval 状态下断线，重连后立刻恢复 `MessageToolCallPendingApproval`。后续阶段覆盖。
10. dynamic tool result 等待状态下断线，重连后恢复 `PendingUserAction::DynamicToolResult`。后续阶段覆盖。
11. 使用已 GC 的 `after_event_id` 返回 `410 Gone`。当前阶段已覆盖。
12. 重复 `Idempotency-Key` + 相同 request 返回同一个 ack；不同 request 返回 `409 Conflict`。当前阶段已覆盖。
13. 首次 ack 因首事件超时失败后，同 key 同 hash 重试返回原 failed 终态，且不重新执行 turn。后续阶段覆盖。

### 13.3 WebUI 测试

1. 刷新页面后恢复 running 状态。
2. 刷新页面后恢复 tool progress。
3. pending approval 状态刷新后仍显示审批 UI。
4. pending dynamic tool result 状态刷新后仍显示正确等待 UI。
5. completed 后 stream state 清理，历史 turns 正常展示。
6. fetch-based SSE 能携带 Authorization header，禁止使用原生 EventSource 回归。

---

## 14. 风险与约束

1. 如果事件只写内存，不满足目标，必须 durable。
2. 如果 cursor 只用 stream 内 sequence，不满足目标，必须 session 级。
3. 如果断线恢复靠重新 `POST /messages`，会造成重复执行，禁止。
4. 如果 replay 只恢复最终文本，不恢复工具状态，webui 仍会失真。
5. 如果 event log 和 canonical turn 双写不一致，需要以明确错误暴露，不能静默吞掉。
6. 如果 `event_id` 用 UUIDv7 或普通字符串比较作为 cursor，会出现水位漂移，禁止。
7. 如果浏览器端改用原生 `EventSource`，Authorization header 会失效，禁止作为 webui 默认实现。
8. 第一阶段 `SessionEventBus` 只支持单实例，多实例部署前必须补跨进程 pub/sub 或 storage polling。

---

## 15. 分阶段实施建议

## 阶段 1: Durable Event Log And Gateway Forwarder
**目标**: 新增 session event 存储、service、GC，补齐 dynamic tool pending stream event，并让 HTTP 层从 Gateway stream receiver 写入 event log。
**成功标准**: 每个 Gateway stream event 都有数据库自增 `event_seq` 和对外 `event_id`；event log 按 retention 配置自动 GC；同一 turn 的 Gateway receiver 事件序列与 `session_events` 表落库序列按 `(message_id, sequence)` 完全一致。
**前置**: 确认或补齐 `Storage::transaction` 能支持事务内写入并回读 `last_insert_rowid` / `event_seq`。
**测试**: storage transaction 探针测试、store 单测、payload roundtrip 单测、event log GC 单测、active turn 不被 GC 测试、dynamic tool pending event 单测、forwarder 一致性测试、`03_messages_http_sse.sh` 通过。
**状态**: 已完成

## 阶段 2: HTTP SSE Replay API
**目标**: 新增 `/v1/sessions/{id}/events` 和 `/active-turn`。
**成功标准**: 断线后使用 `after_event_id` 可补齐事件并继续 live tail。
**测试**: HTTP e2e replay、multi-subscriber、replay/live gap、410 Gone 测试。
**状态**: 已完成

## 阶段 3: Async Message Ack And Persistent Idempotency
**目标**: 在 HTTP 层新增 async ack 模式和持久幂等，不修改 `Gateway` trait。
**成功标准**: 重复 `Idempotency-Key` 返回同一 `turn_id/message_id`，不同 request hash 返回 `409 Conflict`。
**测试**: 幂等 e2e、首事件超时失败测试、forwarder 持久化测试。
**状态**: 已完成

## 阶段 4: WebUI Recovery
**目标**: webui 改为 last event cursor + `/events` 恢复。
**成功标准**: 页面刷新后运行中的基础回答事件继续增量渲染，不要求用户重试。pending user action 恢复单独进入后续阶段。
**测试**: webui hook 测试、fetch-based SSE auth 测试、浏览器手工刷新验证。
**状态**: 已完成

## 阶段 5: TUI Recovery
**目标**: TUI 使用同一套 `/events` 订阅恢复运行态。
**成功标准**: TUI 断线后可按 cursor 继续接收事件。
**测试**: TUI remote stream replay 测试。
**状态**: 已完成

## 阶段 6: Remove Legacy Message Streaming
**目标**: 删除旧 response SSE、同步 message response、`TalosMessageRequest.stream` 和内存幂等缓存。
**成功标准**: `POST /messages` 只返回 ack，所有运行态渲染只来自 `/events`；旧 request-bound stream API 已从 Rust SDK / TS SDK 公共导出移除；webui/tui 不再编译期引用 response-body event 路径。
**测试**: replay e2e、webui recovery、TUI recovery 全部通过；Rust SDK / TS SDK 编译验证旧 stream API 不再导出；webui/tui 类型检查确认无旧 stream-response 引用；旧 stream response 测试删除。
**状态**: 已完成

## 阶段 7: Pending User Action Recovery Edge Cases
**目标**: 补齐 approval / dynamic tool pending 场景下的断线重连恢复，并让前端能强类型读取 `pending_user_action`。
**成功标准**: approval pending 和 dynamic tool result pending 状态都能在刷新或断线重连后恢复；TS SDK 暴露强类型 `TalosPendingUserAction`；首事件超时后的同 key 同 hash 重试返回原 failed 终态且不重新执行 turn。
**测试**: `talos-session-store` pending action 状态机测试、`talos-gateway-http` `/active-turn` pending action 与首事件超时幂等测试、TS SDK `activeTurn` 测试、WebUI stream recovery 测试、TS SDK/WebUI typecheck。
**进展**: `message_completed` 不再覆盖同一 turn/message 的 `WaitingUserAction`；`/active-turn` 返回 approval / dynamic pending action；TS SDK 暴露强类型 `TalosPendingUserAction` 和 `sessions.activeTurn()`；WebUI 可从 active turn snapshot 恢复 pending approval 与 dynamic tool result 等待状态。
**状态**: 已完成

## 阶段 8: WebUI Event Subscription Consolidation
**目标**: 清理 webui `subscribeSessionEventsForMessage` 与 TS SDK `sendEvents` 的重复订阅、过滤和 abort 逻辑。
**成功标准**: webui 通过 TS SDK 统一订阅 `/events`；webui 不再复制按 `message_id` 过滤和 terminal event abort 的协议逻辑。
**测试**: webui typecheck、chat hook 单测、刷新恢复手工验证。
**状态**: 已完成

---

## 16. 结论

当前优先级不是增加 gRPC，而是把 HTTP SSE 从“一次性消息响应流”升级为“session 级事件订阅协议”。

最小正确演进路径：

```text
durable event log
  -> session cursor
  -> replay + live tail
  -> active turn state
  -> webui refresh recovery
```

这套能力完成后，未来 desktop/appserver 只需要复用同一组 `SessionEventStore`、`SessionEventService`、`ActiveTurnStore`，再增加 gRPC transport，而不是重新设计执行状态同步模型。
