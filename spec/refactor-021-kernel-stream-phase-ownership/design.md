# Kernel Stream Phase Ownership Design

Status: Completed
Owner: agent
Created: 2026-05-17
Updated: 2026-05-17

## 背景

`refactor-019` 已经定义了统一 timeline 模型：`Reasoning/Thinking`、`ToolCall/Acting`、`AssistantText/Commentary`、`AssistantText/FinalAnswer`。`refactor-020` 又把 public stream 收敛到 `TimelineEventEnvelope`。从协议形态看，Talos 已具备从用户输入、思考、执行工具到最终回答的分阶段表达能力。

当前问题出现在阶段所有权没有贯穿到 kernel stream。执行循环里确实有阶段判断：

- `execute_general_turn_loop` 按模型 step、tool batch、finalization 组织 turn。
- `execute_general_turn_step` 在模型 stream 完成后根据 `tool_calls.is_empty()` 得到 `GeneralTurnStepDisposition::ContinueWithTools | FinalResponse`。
- `finalize_general_turn_artifacts` 只在 turn 结束时产出最终 `OutputText`。

但流式展示链路绕过了这个阶段判断：`stream_round` 收到 provider `TextDelta` 后立即发出 `KernelStreamEventType::OutputDelta`，`orchestration/agent` 又把所有 `OutputDelta` 直接投影为 `AssistantText/FinalAnswer`。这意味着模型在同一轮中先输出“我先搜索/我先看源码”，再发起 tool call 时，前一段过程说明已经被错误标记为最终回答。

Codex 的实现提供了一个关键参考：它不是 delta-first，而是 item-first。模型流先产生 `ResponseEvent::OutputItemAdded(ResponseItem)`，其中 `ResponseItem::Message` 可携带 `MessagePhase::Commentary | FinalAnswer`；后续 `OutputTextDelta` 必须挂到当前 active item，缺少 active item 会 fail fast。也就是说，Codex 的 phase 属于 item 生命周期，而不是由裸文本 delta 推断。

## 问题

Talos 当前实现的核心缺陷不是缺少阶段枚举，而是 **raw provider delta 被过早投影成 timeline phase**。

```text
当前链路

Provider stream
  |
  +-> TextDelta("我先看源码")
  |     |
  |     v
  |   KernelStreamEvent::OutputDelta
  |     |
  |     v
  |   Timeline AssistantText/FinalAnswer   <-- 过早判定
  |
  +-> ToolCallStart / ToolArgsDelta
        |
        v
      Timeline ToolCall/Acting
```

这个链路有三个结构性问题：

1. `OutputDelta` 名称和语义过宽，既可能是工具前过程说明，也可能是最终回答。
2. `orchestration/agent` 在收到 `OutputDelta` 时看不到 `GeneralTurnStepDisposition`，无法可靠判断是否进入最终回答阶段。
3. `TimelineBuilder::start_commentary()` 已存在，但 `KernelStreamEvent::OutputDelta -> TalosTimelineEvent` 的映射没有使用它，现有测试还把 `commentary_count == 0` 固化为期望。

因此，在 Agent 层局部缓存 `OutputDelta` 虽然能缓解症状，但仍然让 phase ownership 留在错误层级。正确修复应把阶段所有权收回到 kernel 的 turn/step 状态机。

## 目标

- 让 `agent-kernel` 成为 `Commentary` 与 `FinalAnswer` 的唯一判定者。
- 让模型文本 stream 从裸 `OutputDelta` 收敛为 item 生命周期事件。
- 保持 `TalosTimelineEvent` / `TimelineEventEnvelope` 对外协议方向不变，优先修正 kernel 内部事件语义。
- 保证工具执行前后的过程说明不会进入最终回答正文。
- 保证最终 assistant answer 只在 `FinalResponse` 或 finalization 阶段创建。
- 对齐 Codex 的 item-first 流式模型，但不照搬其所有历史兼容事件。

## 非目标

- 不引入 intent classifier、输入前缀路由或 `direct_tool_mode` 分流。
- 不重做 planner 模式，仍保留 `BuiltIn` 和 `PlanReAct`。
- 不把 phase 判定下放到 WebUI、Desktop、TUI 或 transport adapter。
- 不重写 `refactor-019` / `refactor-020` 的 public timeline 模型。
- 不为了兼容旧 `message_*` 协议增加双轨协议。

## 当前实现事实

| 模块 | 当前事实 | 问题 |
| --- | --- | --- |
| `agent-kernel/src/general_turn/loop_run.rs` | `GeneralTurnStepDisposition` 在 step 完成后才可知。 | 阶段判断晚于裸 `OutputDelta` 发出时间。 |
| `agent-kernel/src/general_turn/step_execute.rs` | provider `TextDelta` 立即发 `KernelStreamEventType::OutputDelta`。 | 文本 delta 没有 item id，也没有 phase。 |
| `agent-kernel/src/general_turn/finalize.rs` | finalization 结束时可能补发 `OutputDelta` 作为 fallback。 | fallback 与 provider 文本使用同一种事件，语义混淆。 |
| `orchestration/agent/src/agent.rs` | 所有 `OutputDelta` 都映射到 `TimelineTextItem::FinalAnswer`。 | Agent 层无法知道该文本是不是最终回答。 |
| `agent-kernel/src/timeline.rs` | 已有 `start_commentary()` 与 `start_final_answer()`。 | API 具备能力，但 kernel stream 没有传递 phase。 |
| `web/src/features/chat/stream-events.ts` | 前端按 `item.phase === final_answer` 独立渲染 assistant。 | 前端行为是正确的，错误来自服务端 phase。 |

## Codex 对照结论

Codex 的关键设计不是“有阶段”本身，而是 **item-first + active item**：

```text
Codex stream

response.output_item.added(ResponseItem::Message { phase })
  |
  v
TurnItem::AgentMessage { phase }
  |
  +-> response.output_text.delta(item_id)
  |
  v
response.output_item.done(item)
```

对 Talos 的启发：

- 文本 delta 必须挂到已开始的 item 上。
- phase 应是 item 的属性，不应由 delta 类型隐式表达。
- 没有 active item 的文本 delta 应 fail fast。
- `Commentary` 是一等 assistant message phase，不是 UI fallback。
- `FinalAnswer` 只能由明确的最终回答 item 承载。

## 设计决策

### 1. Kernel stream 改为 phase-aware item stream

新增 kernel 内部事件，替代裸 `OutputDelta` 作为 timeline 投影输入：

```rust
pub enum KernelStreamEventType {
    ThinkingDelta,
    AssistantTextStarted,
    AssistantTextDelta,
    AssistantTextCompleted,
    ToolCallStart,
    ToolArgsDelta,
    ToolQueued,
    ToolExecuting,
    ToolProgressDelta,
    ToolResult,
    ExecutionSummary,
    PromptWarning,
    ConfirmationRequest,
    DynamicToolRequest,
}

pub struct AssistantTextStartedStreamData {
    pub item_key: KernelStreamItemKey,
    pub phase: KernelAssistantTextPhase,
    pub round: Option<u32>,
}

pub struct AssistantTextDeltaStreamData {
    pub item_key: KernelStreamItemKey,
    pub delta: String,
}

pub struct AssistantTextCompletedStreamData {
    pub item_key: KernelStreamItemKey,
}

pub enum KernelAssistantTextPhase {
    Commentary,
    FinalAnswer,
}
```

`OutputDelta` 必须删除，不保留 fallback 或兼容投影路径。所有 timeline 文本必须来自 `AssistantTextStarted/Delta/Completed`。

### 2. `execute_general_turn_step` 只产出 pending model text

provider `TextDelta` 不再立即判定 `FinalAnswer`。`stream_round` 负责收集本轮文本，并在 tool call 明确出现后将该轮文本标为 commentary。

核心规则：

- 同一 model sampling 中只要出现 tool call，该 sampling 的 assistant text 是 `Commentary`。
- 没有 tool call 且 `GeneralTurnStepDisposition::FinalResponse` 时，该 sampling 的 assistant text 是 `FinalAnswer`。
- forced finalization 与 empty final retry 的输出天然属于 `FinalAnswer`。

```rust
async fn execute_general_turn_step(...) -> GeneralTurnStepResult {
    let round_response = stream_round(...).await?;
    let disposition = if round_response.tool_calls.is_empty() {
        GeneralTurnStepDisposition::FinalResponse
    } else {
        GeneralTurnStepDisposition::ContinueWithTools
    };

    emit_assistant_text_for_step(
        stream_tx,
        round,
        &round_response.content,
        disposition,
    ).await;

    GeneralTurnStepResult { text, tool_calls, disposition, ... }
}

async fn emit_assistant_text_for_step(..., disposition) {
    if text.trim().is_empty() {
        return;
    }
    let phase = match disposition {
        ContinueWithTools => KernelAssistantTextPhase::Commentary,
        FinalResponse => KernelAssistantTextPhase::FinalAnswer,
    };
    emit AssistantTextStarted { phase };
    emit AssistantTextDelta { delta: text };
    emit AssistantTextCompleted;
}
```

这会牺牲工具前说明的逐 token 展示，但换来 phase 正确性。后续若需要恢复逐 token，可在 provider 支持 output item phase 时走真正 item-first streaming。

### 3. 工具事件保持实时，文本阶段延后到 step disposition

工具调用、工具参数、执行进度和结果仍保持实时输出：

```text
Provider stream round
  |
  +-> reasoning delta        -> Reasoning/Thinking 实时输出
  +-> tool args/call/result  -> ToolCall/Acting 实时输出
  +-> assistant text delta   -> 仅累积到 round_response.content
  |
  v
Step completed
  |
  +-> has tool calls     -> AssistantText/Commentary
  +-> no tool calls      -> AssistantText/FinalAnswer
```

过程说明通常很短，延后到 step 完成后展示是可以接受的；工具执行本身仍然实时。这个决策避免使用 Agent 层缓存猜测 phase。

### 4. `orchestration/agent` 只做无损映射

`agent.rs` 不再从 `OutputDelta` 猜 phase，只映射明确 phase 的 kernel event：

```rust
match event.data {
    KernelStreamEventData::AssistantTextStarted(data) => {
        let phase = match data.phase {
            Commentary => TalosTimelinePhase::Commentary,
            FinalAnswer => TalosTimelinePhase::FinalAnswer,
        };
        timeline.start_item(
            TalosTimelineItemKind::AssistantText,
            phase,
            None,
            TalosTimelineContent::Text { text: String::new() },
        )
    }
    KernelStreamEventData::AssistantTextDelta(data) => {
        let item_id = state.item_id_for(data.item_key)?;
        timeline.append_delta(&item_id, TalosTimelineDelta::Text { text: data.delta })
    }
    KernelStreamEventData::AssistantTextCompleted(data) => {
        let item_id = state.item_id_for(data.item_key)?;
        timeline.complete_item(&item_id, TalosTimelineItemStatus::Completed, None)
    }
}
```

Agent 层保留 `KernelTimelineState` 只用于 kernel item key 到 timeline item id 的映射，不再负责 phase 决策。

### 5. `FinalAnswer` 不变量

必须在 Rust 单测中固化以下不变量：

- `AssistantText/FinalAnswer` 只能由 `GeneralTurnStepDisposition::FinalResponse` 或 finalization fallback 触发。
- `ContinueWithTools` step 的 `text` 只能进入 `AssistantText/Commentary`。
- 同一 turn 中可以有多个 `Commentary` item，但最终最多一个 live `FinalAnswer` item。
- 没有 active assistant text item 的 text delta 必须 fail fast。
- `ToolCall/Acting` item 不受 assistant text item 状态影响。

## 目标架构

```text
User input
  |
  v
agent-kernel general_turn state machine
  |
  +-> Prepare
  |
  +-> ModelStep(round N)
  |     |
  |     +-> provider reasoning delta --------------+
  |     |                                          v
  |     |                                    Reasoning/Thinking
  |     |
  |     +-> provider tool call/args ---------------+
  |     |                                          v
  |     |                                    ToolCall/Acting
  |     |
  |     +-> provider assistant text
  |            |
  |            v
  |       pending step text
  |
  +-> StepDisposition
  |     |
  |     +-> ContinueWithTools -> AssistantText/Commentary
  |     |
  |     +-> FinalResponse ----> AssistantText/FinalAnswer
  |
  +-> ToolBatch / Acting
  |
  +-> Finalization
        |
        v
      TurnCompleted
```

## 模块接口清单

| 模块 | 变更职责 |
| --- | --- |
| `agent-kernel/src/types.rs` | 新增 `AssistantTextStarted/Delta/Completed` kernel stream event 与 `KernelAssistantTextPhase`。 |
| `agent-kernel/src/general_turn/step_execute.rs` | provider text delta 只累积；step disposition 确认后发 phase-aware assistant text event。 |
| `agent-kernel/src/general_turn/finalize.rs` | finalization fallback 发 `AssistantText/FinalAnswer`，不再复用裸 `OutputDelta`。 |
| `orchestration/agent/src/agent.rs` | 按 kernel assistant text phase 创建 `AssistantText/Commentary` 或 `AssistantText/FinalAnswer` timeline item。 |
| `agent-kernel/src/timeline.rs` | 复用现有 `start_commentary()` / `start_final_answer()`；必要时补充 fail-fast 测试。 |
| `common/types/src/talos.rs` | 暂不改 public timeline 类型；仅在后续需要时补充更严格 schema 注释。 |
| `web/features/chat` | 不新增 phase 推断逻辑；保留按服务端 `item.phase` 渲染。 |
| `ui-tui` / `cli` | 消费已有 timeline phase；修正 commentary/final answer 展示差异测试。 |

## 核心 API 伪代码

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KernelStreamItemKey {
    pub round: Option<u32>,
    pub local_id: String,
}

pub enum KernelAssistantTextPhase {
    Commentary,
    FinalAnswer,
}

pub enum KernelStreamEventData {
    ThinkingDelta(ThinkingDeltaStreamData),
    AssistantTextStarted(AssistantTextStartedStreamData),
    AssistantTextDelta(AssistantTextDeltaStreamData),
    AssistantTextCompleted(AssistantTextCompletedStreamData),
    ToolCallStart(ToolCallStartStreamData),
    ToolArgsDelta(ToolArgsDeltaStreamData),
    ToolResult(ToolResultStreamData),
    ExecutionSummary(ExecutionSummaryStreamData),
    // existing non-text variants...
}

impl StreamRound {
    fn assistant_text_phase(&self) -> KernelAssistantTextPhase {
        if self.tool_calls.is_empty() {
            KernelAssistantTextPhase::FinalAnswer
        } else {
            KernelAssistantTextPhase::Commentary
        }
    }
}

async fn emit_step_assistant_text(
    tx: Option<&mpsc::Sender<KernelStreamEvent>>,
    round: u32,
    text: &str,
    disposition: GeneralTurnStepDisposition,
) {
    if text.trim().is_empty() {
        return;
    }

    let item_key = KernelStreamItemKey {
        round: Some(round),
        local_id: format!("assistant_text_{round}"),
    };
    let phase = match disposition {
        GeneralTurnStepDisposition::ContinueWithTools => KernelAssistantTextPhase::Commentary,
        GeneralTurnStepDisposition::FinalResponse => KernelAssistantTextPhase::FinalAnswer,
    };

    emit AssistantTextStarted { item_key: item_key.clone(), phase, round: Some(round) };
    emit AssistantTextDelta { item_key: item_key.clone(), delta: text.to_string() };
    emit AssistantTextCompleted { item_key };
}
```

## 迁移步骤

1. 在 `agent-kernel` 增加 phase-aware assistant text kernel event，并同步删除旧 `OutputDelta` 类型入口。
2. 修改 `step_execute.rs`，让 provider `TextDelta` 不再发 `OutputDelta`，而是在 step disposition 后发 assistant text item events。
3. 修改 `finalize.rs`，fallback final answer 发 `AssistantText/FinalAnswer` events。
4. 修改 `orchestration/agent` 的 `emit_kernel_timeline_events`，支持新事件并停止把 `OutputDelta` 映射为 `FinalAnswer`。
5. 删除 `OutputDelta` 类型、`OutputDeltaStreamData`、`KernelOutputDeltaSource` 以及所有 `OutputDelta -> FinalAnswer` 映射代码，不保留兼容层。
6. 更新 CLI/TUI/WebUI 测试预期，确认客户端只消费服务端 phase，不做本地推断。

## 验证策略

Rust 单测：

- `ContinueWithTools` step 中的 assistant text 生成 `AssistantText/Commentary`。
- `FinalResponse` step 中的 assistant text 生成 `AssistantText/FinalAnswer`。
- 同一轮 `TextDelta -> ToolCallStart` 不会创建 `FinalAnswer`。
- finalization fallback 不再通过裸 `OutputDelta` 进入 timeline。
- missing assistant item delta 返回错误，不创建匿名 item。

集成测试：

- 触发工具调用的普通问题：工具运行期间不出现最终 assistant block。
- 工具完成后的最终总结：仅最终总结进入 `FinalAnswer`。
- approval / dynamic tool result 恢复：pending tool item 不受 assistant commentary 影响。

前端测试：

- WebUI `streaming.test.tsx` 确认 `commentary`、`acting`、`final_answer` 分离。
- `MessageList.test.tsx` 确认 timeline 内 commentary 可折叠，final answer 独立渲染。

建议命令：

```bash
cargo test -p talos-agent-kernel
cargo test -p talos-agent
pnpm --dir talos-app/web test -- streaming.test.tsx MessageList.test.tsx
```

## 风险与权衡

- 短期会降低 assistant text 的逐 token 可见性：工具前说明要等本轮 step 完成后才能确认 phase。该权衡优先保证语义正确。
- 如果某些 provider 已经提供 message phase，应后续直接接入 provider item phase，恢复真正逐 token 的 commentary/final answer streaming。
- 旧 `OutputDelta` 必须彻底清理；如果清理过程中发现仍有调用点，视为实现未完成，而不是保留兼容路径。
- TUI 当前可能把 `Commentary` 和 `FinalAnswer` 都 append 到 assistant 展示，需要同步检查展示语义，避免 TUI 出现同类混淆。

## Open Questions

- provider 支持 `MessagePhase` 时，是否应优先使用 provider phase，还是仍由 `GeneralTurnStepDisposition` 统一裁决？

推荐默认：

- 接受工具前说明延后展示。
- provider phase 只作为输入信号，最终 public phase 仍由 `agent-kernel` 裁决。
