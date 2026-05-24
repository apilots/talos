# Application Timeline Recovery Stream Design

Status: Archived
Owner: agent
Created: 2026-05-13
Updated: 2026-05-14

## 背景

`refactor-019` 将流式消息事件从旧 `message_*` 模型替换为 `TalosTimelineEvent`。这个对象适合表达 timeline 业务事实，但不足以独立承担故障恢复协议。

改动前以下能力已经是产品语义的一部分：

- 网络断线后通过 event cursor 增量恢复。
- Desktop sidecar / AppServer 重启后 replay 已落库事件并接续 live stream。
- WebUI 刷新或切换 session 后恢复 session history，并接续 running turn。
- approval / dynamic tool result 等 waiting user action 状态可以恢复并继续同一个 turn。
- retry 失败消息后，客户端可以从新的 accepted message 接续事件流。

这些能力不应由 HTTP server、AppServer runtime、Desktop controller、TS SDK、TUI 各自实现 replay/live 拼接。`application-service` 是多端服务端共享层，应统一承载 timeline recovery stream 能力；协议层只做 transport adapter。

## 目标

- 对外 stream API 统一提供 `TimelineEventEnvelope`，不再拆成“裸 `TalosTimelineEvent` 流”和“恢复 envelope 流”两套 public event 语义。
- `TalosTimelineEvent` 保持内部 domain event，只表达 timeline 内容。
- `TimelineEventEnvelope` 成为唯一 public stream event DTO，表达 event cursor、持久化序号、时间戳、session/turn/message 归属和内部 timeline event。
- AppServer 多 subscription 只在 notification 外层增加 `subscription_id`，不污染 `TimelineEventEnvelope`。
- `application-service` 统一提供 replay、recover、subscribe、cursor gap、active turn snapshot 能力。
- HTTP SSE、AppServer JSON-RPC、Desktop sidecar、Rust SDK、TS SDK、TUI/WebUI 都复用同一 application-service recovery 能力。

## 非目标

- 不恢复旧 `message_*` 事件协议。
- 不把 cursor、subscription、watermark 等恢复字段塞进 `TalosTimelineEvent.payload`。
- 不在客户端重新实现 replay/live 合并状态机。
- 不保留多套语义重复的 public event DTO。
- 不引入按输入前缀、intent classifier 或 planner 分流逻辑。
- 不改变 session/message/tool approval 的业务语义。

## 核心原则

1. `TalosTimelineEvent` 是 domain event，不直接作为 public stream DTO。
2. `TimelineEventEnvelope` 是唯一 public stream event DTO。
3. event log replay、live stream、active turn snapshot、cursor gap 判断统一在 `application-service`。
4. HTTP/AppServer/Desktop 只做 transport 映射，不做业务状态机。
5. 客户端可以保存 cursor，但不能决定 replay/live 如何合并。
6. session history refresh + running turn 接续也属于故障恢复场景。
7. 极简优先：只保留必要的 domain event、public envelope、subscription notification 三层，不增加重复语义对象。

## 目标模型

```rust
pub struct TimelineEventEnvelope {
    pub event_id: TimelineEventId,
    pub event_seq: i64,
    pub created_at_ms: i64,
    pub session_id: String,
    pub turn_id: String,
    pub message_id: String,
    pub event: TalosTimelineEvent,
}

pub struct TimelineSubscriptionNotification {
    pub subscription_id: TimelineSubscriptionId,
    pub event: TimelineEventEnvelope,
}

pub struct TimelineReplay {
    pub events: Vec<TimelineEventEnvelope>,
    pub high_water_mark_event_id: Option<TimelineEventId>,
    pub last_replay_seq: i64,
    pub cursor_status: TimelineCursorStatus,
}

pub enum TimelineCursorStatus {
    Current,
    FromBeginning,
    Gap,
}

pub struct TimelineRecoverySnapshot {
    pub replay: TimelineReplay,
    pub active_turn: Option<ActiveTurnRecord>,
}

pub struct TimelineSubscription {
    pub session_id: String,
    pub high_water_mark_event_id: Option<TimelineEventId>,
    pub replay: TimelineReplay,
    pub events: TimelineEnvelopeReceiver,
    pub has_live_turn: bool,
}
```

`TalosTimelineEvent` 不再承担 public cursor 字段职责。即使实现阶段为了迁移暂时保留 `event_id/event_seq/created_at_ms` 字段，也必须视为兼容残留，最终 public 恢复语义以 `TimelineEventEnvelope` 为准。

`TimelineSubscription` 由 `application-service` 返回，不持有 AppServer JSON-RPC 的 `subscription_id`。`subscription_id` 只属于 AppServer notification 外层，用于多订阅路由和 `event.unsubscribe`，不得进入 `TimelineEventEnvelope` 或 application-service domain 模型。

`TimelineSubscription.events` 是 application-service 统一拼好的 recoverable envelope stream，已经包含 replay、live 过滤、去重、cursor gap 判断和 terminal close 策略。HTTP/AppServer/Desktop/TUI/Web 不应再读取裸 `live_events` 后各自拼接 replay。

## Server Flow

```text
Gateway stream
  |
  v
TimelineEventService
  |
  +-> persist TalosTimelineEvent
  |      |
  |      v
  |   TimelineEventEnvelope(event_id, event_seq, created_at_ms, event)
  |
  +-> update ActiveTurnStore
  |
  v
Application TimelineRecoveryService
  |
  +-> replay_after(session_id, after_event_id)
  +-> recover_session(session_id, after_event_id)
  +-> subscribe(session_id, after_event_id)
  |
  +----------------------+----------------------+
  |                      |                      |
  v                      v                      v
HTTP SSE adapter     AppServer adapter      SDK/TUI helpers
```

## Recovery Scenarios

### 1. 网络断线后增量恢复

```text
Client has last_event_id=ev_120
  |
  v
recover_session(session_id, ev_120)
  |
  +-> replay envelope events ev_121..ev_N
  +-> return active_turn snapshot
  +-> subscribe live after ev_N
```

如果 cursor 已过期，application-service 返回明确 `TimelineCursorStatus::Gap` 或 `ResourceExpired`，客户端必须刷新完整 session history，不能静默从最新事件继续。

### 2. 切换或刷新 session history

```text
Client opens session
  |
  v
recover_session(session_id, last_known_event_id?)
  |
  +-> replay timeline envelopes
  +-> return active turn if running/waiting_user_action/cancelling
  +-> client rebuilds blocks from envelope.event + active snapshot
```

这不是普通查询，也不是客户端本地拼接；它属于 timeline recovery。

### 3. running turn 接续

```text
ActiveTurnStore says WaitingUserAction
  |
  v
recover_session
  |
  +-> replay persisted tool item state
  +-> active_turn exposes pending approval/dynamic action
  +-> resume command continues same turn/tool item
```

approval resume 不能创建第二个 `ToolCall/Acting` item，也不能把 pending approval 表达成独立 completed item。

### 4. Desktop sidecar / AppServer restart

```text
Desktop controller reconnects to AppServer
  |
  v
event.subscribe(session_id, since_event_id)
  |
  v
AppServer adapter calls application TimelineRecoveryService
  |
  +-> replay TimelineSubscriptionNotification
  +-> continue live subscription notifications
```

AppServer notification 必须保留 `subscription_id`，但 `subscription_id` 只属于 notification 外层，不属于 `TimelineEventEnvelope`。

## Transport Mapping

| Transport | Payload Shape | 说明 |
| --- | --- | --- |
| HTTP SSE `/v1/sessions/:id/events` | `TimelineEventEnvelope` | SSE `id` 等于 `TimelineEventEnvelope.event_id`。HTTP 是 session-scoped stream，可以没有 subscription id。 |
| AppServer JSON-RPC `event.subscribe` | `TimelineSubscriptionNotification` | notification 外层包含 `subscription_id`，内部 `event` 是 `TimelineEventEnvelope`。 |
| Rust SDK request-bound helper | `TimelineEventEnvelope` stream | helper 可过滤 turn、处理 terminal，但不能投影成另一种 DTO。 |
| TS SDK session events | `TimelineEventEnvelope` | WebUI 用 `event_id` 做 cursor，用 `event` 渲染 timeline。 |
| WebUI/Desktop reducer | `TimelineEventEnvelope` | 渲染读取 `envelope.event`，恢复控制读取 `envelope.event_id/event_seq`。 |
| TUI/CLI | `TimelineEventEnvelope` stream | 可以隐藏 cursor 展示细节，但恢复能力不在 TUI/CLI 内重造。 |

## 核心 API 伪代码

```rust
#[async_trait]
pub trait TimelineRecoveryService: Send + Sync {
    async fn replay_after(
        &self,
        actor: &Actor,
        session_id: &str,
        after_event_id: Option<&TimelineEventId>,
        limit: usize,
    ) -> TalosResult<TimelineReplay>;

    async fn recover_session(
        &self,
        actor: &Actor,
        session_id: &str,
        after_event_id: Option<&TimelineEventId>,
        limit: usize,
    ) -> TalosResult<TimelineRecoverySnapshot>;

    async fn subscribe(
        &self,
        actor: &Actor,
        params: TimelineSubscribeParams,
    ) -> TalosResult<TimelineSubscription>;
}

pub struct TimelineSubscribeParams {
    pub session_id: String,
    pub after_event_id: Option<TimelineEventId>,
    pub replay_limit: usize,
    pub close_when_idle: bool,
    pub close_on_terminal: bool,
}
```

`close_when_idle` / `close_on_terminal` 是 transport 行为策略：

- HTTP SSE 和 request-bound helper 可以在无 live turn 或 terminal event 后关闭。
- AppServer session-scoped subscription 默认保持打开，terminal event 只代表当前 turn 完成，不代表该 subscription 生命周期结束。

HTTP adapter:

```rust
async fn subscribe_timeline_events_handler(...) -> Result<Response, ApiError> {
    let subscription = app.timeline_recovery.subscribe(actor, params).await?;
    stream_envelopes_as_sse(subscription)
}
```

AppServer adapter:

```rust
async fn handle_event_subscribe(params: EventSubscribeParams) {
    let subscription = app.timeline_recovery.subscribe(actor, params.into()).await?;
    let subscription_id = appserver_subscription_registry.insert(subscription.events);
    send_response(EventSubscribeResponse {
        subscription_id,
        session_id: subscription.session_id,
        high_water_mark_event_id: subscription.replay.high_water_mark_event_id,
    });
    spawn_jsonrpc_notifications(subscription_id, subscription);
}

fn to_notification(
    subscription_id: TimelineSubscriptionId,
    event: TimelineEventEnvelope,
) -> TimelineSubscriptionNotification {
    TimelineSubscriptionNotification {
        subscription_id,
        event,
    }
}
```

SDK request-bound helper:

```rust
async fn send_timeline_events(
    session_id: &str,
    request: TalosMessageRequest,
) -> Receiver<TimelineEventEnvelope> {
    let subscription = subscribe(session_id, since_event_id=None).await?;
    let ack = start_turn(session_id, request).await?;
    filter_subscription_by_turn(subscription, ack.turn_id)
}
```

UI reducer:

```ts
function applyTimelineEnvelope(envelope: TimelineEventEnvelope) {
  saveCursor(envelope.event_id);
  applyTimelineEvent(envelope.session_id, envelope.event);
}
```

## 模块接口清单

| 模块 | 职责 |
| --- | --- |
| `common/types::talos` | 定义 `TalosTimelineEvent` domain event 模型。 |
| `application-service` 或共享 app protocol types | 定义唯一 public `TimelineEventEnvelope` 和 subscription notification。 |
| `session-store::TimelineEventService` | 持久化事件、生成 persisted envelope、更新 active turn。 |
| `application-service::TimelineRecoveryService` | 统一 replay、recover、subscribe、cursor gap、active turn snapshot。 |
| `gateway/http` | 将 `TimelineEventEnvelope` 映射为 SSE。 |
| `gateway/app-server` | 将 `TimelineEventEnvelope` 包成 `TimelineSubscriptionNotification`。 |
| `apps/sdk` | 基于 envelope 实现 request-bound helper，不自建恢复状态机，不返回裸 event stream。 |
| `ts-sdk` | 暴露 envelope subscription/recovery API。 |
| `web` | 用 `envelope.event` 渲染 timeline，用 `envelope.event_id` 驱动 refresh/reconnect。 |
| `desktop` | 重连后通过 same application recovery API replay subscriptions。 |
| `ui-tui` / `cli` | 消费 SDK envelope helper，不复制 replay/live 合并逻辑。 |

## 状态机要求

- replay envelopes 必须先于 live envelopes 输出。
- live envelope 必须具备 persisted `event_seq/event_id`；没有 persisted sequence 的 event 不得进入 recovery stream。
- terminal event 只结束 request-bound helper，不默认关闭 session-scoped subscription，除非调用方请求 single-turn mode。
- active turn snapshot 必须与 event log 语义一致：
  - `Running`
  - `WaitingUserAction`
  - `Cancelling`
  - `Completed`
  - `Failed`
- waiting action 的 pending payload 来源于 timeline event delta，不由客户端猜测。
- cursor gap 必须显式返回，客户端刷新完整 session history。
- public stream API 不返回裸 `TalosTimelineEvent`，避免维护两套 event 语义。

## 验证策略

- Unit:
  - `TimelineRecoveryService::replay_after` 按 cursor 返回 envelope。
  - expired cursor 返回 gap / resource expired。
  - replay then live 不重复 envelope。
  - terminal event 不影响 session-scoped subscription。
  - request-bound helper 返回 `TimelineEventEnvelope`，只按 turn 过滤，不改变 DTO。
- Integration:
  - HTTP SSE reconnect after `Last-Event-ID`，data 为 `TimelineEventEnvelope`。
  - AppServer `event.subscribe` 多订阅同 session 可区分 `subscription_id`。
  - AppServer `event.unsubscribe` 只停止目标 subscription。
  - Desktop sidecar restart 后 replay missed envelopes。
  - WebUI refresh session history 后接续 running turn。
  - approval resume 继续同一个 tool item。
  - retry failed message 后订阅新 turn 并按 cursor 恢复。

## 与 refactor-019 的关系

`refactor-019` 定义 timeline event 内容模型；本 spec 定义 timeline event 的故障恢复传输模型。

两者边界如下：

- `refactor-019`: `UserInput/Input`、`Reasoning/Thinking`、`ToolCall/Acting`、`AssistantText/FinalAnswer` 如何表达。
- `refactor-020`: `TalosTimelineEvent` 如何被包装成唯一 public `TimelineEventEnvelope`，并如何落库、replay、订阅、重连、接续 running turn。

后续调整 `IMPLEMENTATION_PLAN.md` 时，应先实现 application-service 统一 recovery stream 和 public envelope，再让 HTTP/AppServer/SDK/Web/TUI 删除各自重复的 replay/live 拼接逻辑和裸 event stream public API。
