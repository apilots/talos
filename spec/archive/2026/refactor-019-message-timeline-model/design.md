# Message Timeline Model Design

Status: Archived
Owner: agent
Created: 2026-05-12
Updated: 2026-05-12

## 背景

当前流式协议仍以 `message_*` 事件表达 assistant 输出、reasoning、tool call 和执行进度。事件粒度缺少统一的 item 生命周期，也没有由服务端明确标识最终回答 phase，导致 WebUI/Desktop 需要根据事件顺序和最后一个 block 类型推断 Thinking、tool call、过程说明和 final answer 的边界。

这会带来三个问题：

- `TextDelta` 在 kernel 中直接映射为输出 delta，过程文本和最终回答边界不清晰。
- WebUI 当前按事件名追加 block，过程输出容易混入 assistant final answer。
- TUI 已有 thinking/assistant/tool 交互模型，但消费的是旧流式事件，后续协议和 WebUI 很难保持一致。

本重构用统一 timeline 消息模型替换旧 `message_*` 流式事件。服务端明确输出 `item_id`、`kind`、`phase`、`status` 和时间戳；客户端只按 item 渲染，不再猜测 phase 或边界。

## 目标

- WebUI/Desktop 展示完整 timeline：`User input -> Thinking/Acting timeline -> Final answer`。
- 中间 `Thinking/Acting timeline` 包含 reasoning、过程说明和 tool action；工具参数、审批状态、执行状态、progress 和结果都增量更新到同一个 tool timeline item。
- TUI 保持现有交互形态，但消费新的 timeline event。
- final answer 必须由服务端 phase 明确标识，禁止前端根据 tool call 顺序推断。
- 不做旧协议兼容；MVP 未发布，直接替换旧模型。

## 非目标

- 不改变 planner 路线，不引入 intent classifier。
- 不重做 TUI 视觉交互。
- 不引入双协议迁移层。
- 不把 phase 判定下放到 WebUI/Desktop/TUI。

## 目标模型

统一事件只描述 turn 与 timeline item 生命周期：

```rust
pub enum TalosTimelineEventType {
    TurnStarted,
    ItemStarted,
    ItemDelta,
    ItemCompleted,
    TurnCompleted,
    TurnError,
}

pub enum TalosTimelineItemKind {
    UserInput,
    Reasoning,
    AssistantText,
    ToolCall,
    ExecutionSummary,
    SystemNotice,
}

pub enum TalosTimelinePhase {
    Input,
    Thinking,
    Acting,
    Commentary,
    FinalAnswer,
    System,
}

pub enum TalosTimelineItemStatus {
    Running,
    WaitingApproval,
    Completed,
    Failed,
    Cancelled,
}

pub struct TalosTimelineItem {
    pub item_id: String,
    pub parent_item_id: Option<String>,
    pub kind: TalosTimelineItemKind,
    pub phase: TalosTimelinePhase,
    pub status: TalosTimelineItemStatus,
    pub title: Option<String>,
    pub started_at_ms: u64,
    pub completed_at_ms: Option<u64>,
    pub content: TalosTimelineContent,
}
```

核心 payload：

```rust
pub struct TalosTurnStartedEvent {
    pub request_id: String,
    pub turn_id: String,
    pub started_at_ms: u64,
}

pub struct TalosItemStartedEvent {
    pub request_id: String,
    pub turn_id: String,
    pub item: TalosTimelineItem,
}

pub struct TalosItemDeltaEvent {
    pub request_id: String,
    pub turn_id: String,
    pub item_id: String,
    pub delta: TalosTimelineDelta,
}

pub struct TalosItemCompletedEvent {
    pub request_id: String,
    pub turn_id: String,
    pub item_id: String,
    pub status: TalosTimelineItemStatus,
    pub completed_at_ms: u64,
    pub final_content: Option<TalosTimelineContent>,
}
```

`TalosTimelineContent` 和 `TalosTimelineDelta` 必须是强类型 enum，不允许用灵活 metadata JSON 承载业务结构：

```rust
pub enum TalosTimelineContent {
    Empty,
    Text { text: String },
    ToolCall {
        call_id: String,
        name: String,
        arguments: String,
        progress: Vec<TalosToolProgressEntry>,
        result: Option<TalosToolResult>,
    },
    ExecutionSummary {
        summary: String,
    },
    SystemNotice {
        level: TalosTimelineNoticeLevel,
        message: String,
    },
}

pub enum TalosTimelineDelta {
    Text { text: String },
    ToolArguments { arguments_delta: String },
    ToolProgress {
        progress_kind: TalosToolProgressKind,
        text_delta: String,
    },
    ToolResult {
        content: Vec<TalosContentItem>,
        is_error: bool,
    },
    Status { status: TalosTimelineItemStatus },
}

pub struct TalosToolProgressEntry {
    pub progress_kind: TalosToolProgressKind,
    pub text: String,
    pub timestamp_ms: u64,
}

pub struct TalosToolResult {
    pub content: Vec<TalosContentItem>,
    pub is_error: bool,
}
```

## Phase Rules

| Item | Phase | 规则 |
| --- | --- | --- |
| `UserInput` | `Input` | turn 开始即创建，用户输入完成后立即 completed。 |
| `Reasoning` | `Thinking` | provider/framework reasoning，只进入 Thinking timeline。 |
| `AssistantText` | `Commentary` | 过程中模型给用户看的说明，不进入最终回答正文。 |
| `ToolCall` | `Acting` | 工具 action 主体，包含 name、arguments、call_id、approval 状态、执行状态、progress 和结果；所有进展都更新同一个 item。 |
| `AssistantText` | `FinalAnswer` | 最终回答，只有服务端可创建。 |
| `ExecutionSummary` / `SystemNotice` | `System` | 系统摘要、错误、审批提示等。 |

没有 active item 的 delta 必须 fail fast，不能创建匿名文本块。客户端也不能把未知 delta 拼到最后一个 block。

## Server Flow

```text
User request
  |
  v
TurnStarted
  |
  +-> ItemStarted(UserInput/Input)
  +-> ItemCompleted(UserInput)
  |
  v
provider stream loop
  |
  +-> reasoning delta      -> Reasoning/Thinking item
  +-> tool call lifecycle  -> ToolCall/Acting item
  +-> tool progress        -> ToolProgress delta on the same ToolCall/Acting item
  +-> model commentary     -> AssistantText/Commentary item
  +-> final response       -> AssistantText/FinalAnswer item
  |
  v
TurnCompleted
```

`agent-kernel` 增加 `TimelineBuilder`，集中负责：

- 分配稳定 `item_id`。
- 维护 active item。
- 判定 `kind`、`phase`、`status`。
- 写入 started/completed 时间。
- 生成 turn 内有序 timeline event。

`orchestration agent`、`coordinator`、`gateway` 只做类型映射和透传，不重新判定 phase。

## Client Mapping

中间流程不是单独的 `Reasoning/Thinking` 区域，而是完整的 `Thinking/Acting timeline`。WebUI/Desktop 必须把 `Reasoning/Thinking`、`AssistantText/Commentary` 和 `ToolCall/Acting` 都作为可折叠 timeline item 渲染；tool progress 只是 `ToolCall/Acting` item 的增量状态，不单独创建 item。final answer 单独渲染为最终 assistant answer。

| Timeline item | WebUI/Desktop | TUI |
| --- | --- | --- |
| `UserInput/Input` | 用户输入块，立即稳定显示 | 保持当前用户输入显示 |
| `Reasoning/Thinking` | Thinking 分组，可折叠，显示耗时 | 现有 thinking block |
| `AssistantText/Commentary` | 中间 timeline 文本 item，可折叠，显示耗时 | 现有 assistant 过程文本 |
| `ToolCall/Acting` | Acting timeline tool item，可折叠，显示参数、审批状态、执行进度、结果和耗时 | 现有 tool block；右侧只展示调用过的工具概览 |
| `AssistantText/FinalAnswer` | 最终 assistant answer | 最终 assistant block |
| `SystemNotice/System` | status/error block | 现有系统提示 |

WebUI/Desktop reducer 以 `item_id` 为唯一更新键，不再用最后一个 block 的 kind 拼接。Desktop 共享 WebView，因此只实现一套 timeline UI。

TUI 右侧区域暂不承载 tool progress 展开内容，只作为本轮调用过哪些工具的概览区，例如工具名、状态和耗时。工具参数、progress 和结果以新的 `ToolCall/Acting` timeline item 投影到主 transcript。

## 模块接口清单

| 模块 | 职责 |
| --- | --- |
| `common/types::talos` | 定义 timeline event、item、phase、delta、content 类型。 |
| `agent-kernel` | 生成 timeline item，维护 phase 状态机。 |
| `orchestration/agent` | kernel event 到 orchestrator event 的无损映射。 |
| `coordinator` | orchestrator event 到 gateway event 的无损映射。 |
| `gateway/app-protocol` | 暴露统一 notification method 和 schema。 |
| `gateway/http` | HTTP SSE 使用同一 application event envelope。 |
| `ts-sdk` | 生成并导出 timeline TypeScript 类型。 |
| `web/features/chat` | timeline reducer 和 session stream 状态。 |
| `web/widgets/message-list` | timeline 渲染、折叠、耗时展示。 |
| `ui-tui` | 适配 timeline event 到现有 TUI transcript。 |

## 核心 API 伪代码

```rust
impl TimelineBuilder {
    fn start_turn(&mut self, request_id: RequestId, turn_id: TurnId) -> TalosTimelineEvent;

    fn start_item(
        &mut self,
        kind: TalosTimelineItemKind,
        phase: TalosTimelinePhase,
        parent_item_id: Option<String>,
        content: TalosTimelineContent,
    ) -> TalosResult<TalosTimelineEvent>;

    fn append_delta(
        &mut self,
        item_id: &str,
        delta: TalosTimelineDelta,
    ) -> TalosResult<TalosTimelineEvent>;

    fn complete_item(
        &mut self,
        item_id: &str,
        status: TalosTimelineItemStatus,
        final_content: Option<TalosTimelineContent>,
    ) -> TalosResult<TalosTimelineEvent>;
}
```

```ts
function applyTimelineEvent(
  state: ChatTimelineState,
  event: TalosTimelineEvent,
): ChatTimelineState {
  switch (event.type) {
    case 'item_started':
      return insertByItemId(state, event.item)
    case 'item_delta':
      return appendDeltaByItemId(state, event.item_id, event.delta)
    case 'item_completed':
      return completeByItemId(state, event.item_id, event.status, event.final_content)
    default:
      return updateTurnState(state, event)
  }
}
```

## 替换策略

这是替换式重构，不做旧协议兼容。迁移时应删除或替换这些旧事件语义：

- `message.started`
- `message.reasoning.delta`
- `message.execution.summary`
- `message.output.delta`
- `message.tool_call.*`
- `message.completed`
- `message.error`

新协议统一使用 timeline event。HTTP SSE 与 JSON-RPC notification 必须使用同一强类型结构，不能各自维护业务字段。

## 验证矩阵

| 层级 | 验证 |
| --- | --- |
| Rust 单测 | `reasoning -> tool -> progress -> final answer` 生成正确 item/phase 顺序。 |
| Rust 单测 | 无 active item 的 delta 返回错误。 |
| Rust 映射测试 | kernel -> agent -> coordinator 不丢 `item_id/kind/phase/status`。 |
| SDK/schema | 重新生成类型，确认 HTTP/JSON-RPC notification 结构一致。 |
| TS/Vitest | Web reducer 按 `item_id` 合并 delta，final answer 不混入 commentary。 |
| WebUI 测试 | 用户输入不消失；Thinking/Acting timeline 可折叠；tool action 的状态/progress/结果更新同一个 item；耗时显示；final answer 独立。 |
| TUI 测试 | 新 timeline event 输入下，现有 thinking/assistant/tool 交互保持。 |
| 集成测试 | `tests/integration/run_e2e.sh` 覆盖 CLI/server 主路径。 |

## 边界与风险

- 旧协议直接删除会触发跨 Rust、SDK、WebUI、TUI 的编译错误，必须按阶段推进。
- final answer phase 只能由服务端创建，客户端实现中出现 phase 推断应视为回归。
- Tool approval、dynamic tool request 和 tool progress 必须更新同一个 `ToolCall/Acting` item，否则 UI 会出现重复工具块或状态不同步。
- 若 Desktop 只受 WebView 共享代码影响，最终仍需 packaged smoke；若改到 desktop main/preload 或 sidecar packaging，则必须重新打 DMG 验证。
