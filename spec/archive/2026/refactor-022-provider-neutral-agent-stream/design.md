# Provider Neutral Agent Stream Design

Status: Archived
Owner: agent
Created: 2026-05-21
Updated: 2026-05-21

## 背景

Talos 已经通过 `refactor-019` 定义了统一 timeline 模型，通过 `refactor-020` 收敛了 public recovery stream，通过 `refactor-021` 将 `Commentary` 与 `FinalAnswer` 的判定责任从前端收回到 `agent-kernel`。这些设计让 public timeline 具备了 `Thinking`、`Acting`、`Commentary`、`FinalAnswer` 的表达能力，但 agent 与 LLM 交互这一层仍需要重新收敛。

最近手动验证暴露了两个问题：

- LLM 文本 delta 在 kernel 内被累积，直到 step 结束后一次性作为 assistant text 发出，WebUI 看起来不像 live streaming。
- `ExecutionSummary` / thinking-like timeline item 可能出现在 `FinalAnswer` 后，说明最终回答阶段与 turn 收尾阶段的顺序不变量没有固化。`ExecutionSummary` 仍属于 timeline System 信息，但必须在 `FinalAnswer` 之前完成，作为中间 timeline 的最后一个 system item。

对照 Claude Code 与 Codex，可以看到它们都不是靠自然语言猜测“这是不是最终回答”：

- Claude Code 主要消费 Anthropic Messages content block：`thinking`、`text`、`tool_use`、`tool_result`。工具阶段由 `tool_use` block 驱动；是否继续 loop 看本轮是否出现 tool use；它没有给普通 text 强行贴一个模糊 phase。
- Codex 主要消费 OpenAI Responses item stream：`output_item.added/done`、`output_text.delta`、`reasoning_delta`、`tool_call_delta`、`completed.end_turn`。message `phase` 是 provider 可选信号，有则使用，没有则不伪造。

Talos 的目标不是绑定某一家 provider 协议，而是定义一套自己的 provider-neutral model block，用来承载 OpenAI、Anthropic、Chat Completions 兼容协议的结构化输出。最终回答的确定性由两类信号提供：

1. provider 显式给出 final answer phase；
2. agent-kernel 在 tool loop 完成后主动进入 tools-disabled finalization call。

除此之外，普通 assistant text 都不应被猜成 final answer。

## 问题

当前实现的核心问题不是缺少 public timeline phase，而是 LLM 输出、agent loop 控制、最终回答判定混在一起。

```text
Provider text delta
  |
  v
Kernel stream text
  |
  v
Timeline AssistantText/FinalAnswer
```

这个链路有三个问题：

1. provider 普通 text 只说明“模型输出了可见文本”，不等于“模型输出了最终总结”。
2. `tool_calls.is_empty()` 只说明当前 round 没有工具调用，不等于 provider 明确声明 text 是 final answer。
3. 如果没有明确 finalization 阶段，服务端只能在 loop terminal 后把最后一段 text 当作可见输出；这不应被包装成高置信 `FinalAnswer`。

因此需要把三件事分开：

- provider 输出了什么 block；
- agent loop 是否继续工具调用；
- Talos 是否进入最终总结阶段。

## 目标

- 建立 Talos provider-neutral model block，统一承接 OpenAI、Anthropic、Chat Completions 兼容协议。
- Provider adapter 只负责把原始协议映射成 model block，不做产品级 final answer 判定。
- Agent kernel 负责 loop 控制、工具执行、finalization，以及 model block 到 timeline phase 的转换。
- 支持从 LLM 调用侧开始的增量展示：thinking、assistant text、tool call arguments、tool progress 都可以实时 delta。
- `FinalAnswer` 只允许来自 provider explicit final phase 或 kernel explicit finalization。
- 缺失 provider phase 的 assistant text 在 working loop 中展示为 `Commentary`，不引入 `Unknown` phase。
- `FinalAnswer` 后禁止再出现 `Thinking`、`Acting`、`Commentary` 主 timeline item。
- WebUI、Desktop、TUI、CLI 只消费统一 timeline event，不在客户端推断 phase。

## 非目标

- 不引入 intent classifier、用户输入前缀分流或规则路由。
- 不把 provider 私有事件直接暴露给 WebUI/Desktop/TUI。
- 不要求所有 provider 都支持 `final_answer` phase。
- 不通过 prompt 文本标签作为主协议，例如要求模型输出 `<final_answer>`。
- 不新增 `Unknown` / `VisibleOutput` 等 public timeline phase。
- 不重做 `refactor-019` / `refactor-020` 的 public timeline envelope。
- 不为了旧协议兼容保留 `message_*` 双轨流。

## 当前实现事实

| 模块 | 当前事实 | 问题 |
| --- | --- | --- |
| `agent-kernel/src/general_turn/step_execute.rs` | provider `TextDelta` 被收集到 round output，step 完成后再 emit assistant text item。 | phase 比旧实现更集中，但普通文本无法从 LLM 调用侧实时进入 UI。 |
| `agent-kernel/src/general_turn/finalize.rs` | finalization 可补发最终文本和 execution summary。 | final answer 与 execution summary 的顺序不变量不够明确。 |
| `agent-kernel/src/types.rs` | 已有 `AssistantTextStarted/Delta/Completed` 与 `KernelAssistantTextPhase`。 | 缺少 provider-neutral model block 层，kernel 仍直接面对 provider stream 语义。 |
| `common/types/src/talos.rs` | public timeline 已有 `Thinking/Acting/Commentary/FinalAnswer/System`。 | public phase 足够，但上游 phase 来源和 call mode 未固化。 |
| `orchestration/agent/src/agent.rs` | kernel event 映射为 timeline item。 | 应保持无损映射，不应参与 phase 决策。 |
| `web/features/chat/stream-events.ts` | 前端按 `item.phase` 渲染 final answer 与 timeline。 | 前端是 phase 消费方，不应补充推断逻辑。 |

## 设计原则

1. Provider adapter 是事实映射层：只解析 provider 原生 block、delta、tool call、finish/end signals。
2. model block 是统一承载层：表达模型输出结构，不表达产品最终阶段。
3. Agent kernel 是决策层：根据 call mode、provider phase hint、tool loop 状态决定 public timeline phase。
4. Timeline event 是展示与恢复层：只承载已确认的 Talos 产品语义。
5. Prompt 只能辅助模型行为，不能作为事件协议。
6. 没有 explicit final phase 时，最终总结应由 kernel 主动 finalization，而不是猜普通文本。

## 目标架构

```text
Provider raw stream
  |
  v
Provider adapter
  |
  v
ModelBlock stream
  |
  +-> Reasoning / Thinking
  +-> AssistantText
  +-> ToolCall
  +-> ToolResult
  +-> SystemNotice
  |
  v
agent-kernel general_turn state machine
  |
  +-> WorkingLoop call
  |     |
  |     +-> Reasoning              -> Timeline Reasoning/Thinking
  |     +-> AssistantText          -> Timeline AssistantText/Commentary
  |     +-> ToolCall               -> Timeline ToolCall/Acting
  |     +-> ToolResult             -> model context, continue loop
  |     +-> explicit final phase detected
  |           |
  |           +-> ExecutionSummary, System timeline item
  |           +-> AssistantText    -> Timeline AssistantText/FinalAnswer
  |           +-> TurnCompleted
  |
  +-> Loop terminal
  |     |
  |     +-> ExecutionSummary, System timeline item
  |     +-> Finalization call, tools disabled
  |           |
  |           +-> AssistantText    -> Timeline AssistantText/FinalAnswer
  |           +-> TurnCompleted
  |
  v
TimelineEventEnvelope
  |
  +-> HTTP SSE
  +-> AppServer JSON-RPC
  +-> CLI/TUI/WebUI/Desktop
```

## 核心模型

### Model Block

Model block 是 provider adapter 输出的统一中间层。它不是 provider 原始事件，也不是 public timeline event。

```rust
pub enum ModelBlockKind {
    Reasoning,
    AssistantText,
    ToolCall,
    ToolResult,
    SystemNotice,
}

pub enum AssistantTextPhaseHint {
    Commentary,
    FinalAnswer,
}

pub enum AssistantTextPhaseHintSource {
    ExplicitProviderPhase,
}

pub struct ModelBlockStarted {
    pub model_item_id: String,
    pub provider_item_id: Option<String>,
    pub kind: ModelBlockKind,
    pub assistant_text_phase_hint: Option<AssistantTextPhaseHint>,
    pub assistant_text_phase_hint_source: Option<AssistantTextPhaseHintSource>,
}

pub enum ModelBlockDelta {
    ReasoningText { delta: String },
    AssistantText { delta: String },
    ToolArguments { delta: String },
    ToolProgress { delta: String },
}

pub struct ModelBlockCompleted {
    pub model_item_id: String,
}
```

约束：

- `assistant_text_phase_hint=None` 表示 provider 没有给 phase，不表示 `Unknown`。
- `assistant_text_phase_hint=Some(FinalAnswer)` 只能来自 provider 显式 phase。
- Anthropic 普通 `text`、Chat Completions `delta.content`、OpenAI Responses 缺 phase 的 message 都映射为 `AssistantText` + `phase_hint=None`。
- Tool call、tool arguments、reasoning 必须用结构化 block 表达，不能塞进 assistant text。

### Kernel Call Mode

Kernel 通过 call mode 决定普通 assistant text 应进入哪个 timeline phase。

```rust
pub enum KernelModelCallMode {
    WorkingLoop,
    Finalization,
}

pub enum KernelAssistantTextPhase {
    Commentary,
    FinalAnswer,
}
```

Phase 决策表：

| Call mode | Model block | Provider phase hint | Timeline phase |
| --- | --- | --- | --- |
| `WorkingLoop` | `AssistantText` | `Some(Commentary)` | `Commentary` |
| `WorkingLoop` | `AssistantText` | `Some(FinalAnswer)` | emit `ExecutionSummary/System` first, then `FinalAnswer` |
| `WorkingLoop` | `AssistantText` | `None` | `Commentary` |
| `Finalization` | `AssistantText` | 任意 | `FinalAnswer` |

`WorkingLoop + None -> Commentary` 是产品定义，不是猜测最终阶段。含义是：工具仍可用、agent loop 尚未进入最终总结时，模型可见文本属于过程说明或中间说明。最终总结必须来自 explicit final phase 或 finalization call。

如果 provider explicit final phase 在 working loop 中出现，kernel 必须先完成所有 active non-final timeline item，emit `ExecutionSummary/System` 作为中间 timeline 的最后一个 system item，然后再 start `AssistantText/FinalAnswer`。这样可以保证 `FinalAnswer` 后没有任何主流程 timeline item。

### Public Timeline

Public timeline 继续使用已有模型：

```rust
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
```

第一阶段不新增 phase。客户端不需要识别 provider phase hint，也不需要知道 call mode。

## Provider 映射规则

### Anthropic Messages-compatible

| Anthropic event | model block | 说明 |
| --- | --- | --- |
| `content_block_start` `thinking` | `Reasoning` | thinking block |
| `content_block_delta` `thinking_delta` | `ReasoningText` delta | live thinking |
| `content_block_start` `text` | `AssistantText`, `phase_hint=None` | 普通可见文本 |
| `content_block_delta` `text_delta` | `AssistantText` delta | working loop 中进入 commentary |
| `content_block_start` `tool_use` | `ToolCall` | 结构化工具调用 |
| `content_block_delta` `input_json_delta` | `ToolArguments` delta | live tool args |
| next request `tool_result` | `ToolResult` | 工具结果进入下一轮模型上下文 |
| `message_stop` | loop control signal | 不直接决定 final answer |

Claude Code 的关键经验是：执行阶段由实际 `tool_use` block 驱动，`stop_reason == tool_use` 只能作为辅助信号，不应作为唯一依据。

### OpenAI Chat Completions-compatible

| Chat Completions signal | model block | 说明 |
| --- | --- | --- |
| `delta.content` | `AssistantText`, `phase_hint=None` | 普通可见文本 |
| supported reasoning delta fields | `ReasoningText` delta | live thinking |
| `delta.tool_calls` | `ToolCall` / `ToolArguments` | 结构化工具调用 |
| `finish_reason=tool_calls` | loop control fallback | 辅助判断工具阶段 |
| `finish_reason=stop` | loop control fallback | 不等同 final answer |

当前 OpenAI provider 使用 `/chat/completions` 兼容流，不解析 OpenAI Responses item lifecycle。Chat Completions 没有标准 phase，Talos 不应从非标准 `phase` / `message_phase` / `talos_phase` 字段伪造 explicit final answer。

### Bailian / DashScope Chat Completions-compatible

| Chat Completions signal | model block | 说明 |
| --- | --- | --- |
| `delta.content` | `AssistantText`, `phase_hint=None` | 普通可见文本 |
| `delta.reasoning_content` / compatible thinking fields | `ReasoningText` delta | live thinking |
| `delta.tool_calls` | `ToolCall` / `ToolArguments` | 结构化工具调用 |
| `finish_reason=tool_calls` | loop control fallback | 辅助判断工具阶段 |
| `finish_reason=stop` | loop control fallback | 不等同 final answer |

Chat Completions 兼容层没有 item phase。Talos 不应为它发明 phase；在 working loop 中普通 text 进入 commentary，最终总结通过 finalization call 产生。

### OpenAI Responses-compatible Non-goal

OpenAI Responses endpoint 当前明确不实现，不属于本 refactor 的目标范围。Talos 当前 OpenAI provider 只覆盖 Chat Completions-compatible stream。

如果未来单独引入 Responses provider mode，必须新增明确 endpoint / request adapter，并映射以下事件：

| OpenAI Responses event | model block | 说明 |
| --- | --- | --- |
| `response.output_item.added` reasoning | `Reasoning` | reasoning item |
| `response.reasoning_*delta` | `ReasoningText` delta | live thinking |
| `response.output_item.added` message with phase | `AssistantText`, `phase_hint=Some(...)` | provider explicit phase |
| `response.output_item.added` message without phase | `AssistantText`, `phase_hint=None` | 普通可见文本 |
| `response.output_text.delta` | `AssistantText` delta | 挂到 active assistant item |
| `response.output_item.added` function/custom tool call | `ToolCall` | 结构化工具调用 |
| `response.*tool_call*_delta` | `ToolArguments` delta | live tool args |
| `response.completed.end_turn` | loop control signal | 决定是否 follow-up，不等同 final phase |

Codex 的关键经验仍然保留：message `phase` 是 provider 可选字段。有 phase 时可以作为内容归属信号；没有 phase 时不伪造。

## Final Answer 规则

只有两种来源可以创建 `AssistantText/FinalAnswer`：

1. Provider explicit final phase
   例如未来 OpenAI Responses message item 明确带 `phase=final_answer`。

2. Kernel explicit finalization
   Agent loop 结束后，kernel 发起 tools-disabled finalization call。该 call 中所有 assistant text 都属于 `FinalAnswer`。

禁止规则：

- 禁止根据自然语言内容判断 final answer。
- 禁止把 `tool_calls.is_empty()` 直接等同于 final answer。
- 禁止把 `finish_reason=stop` 直接等同于 final answer。
- 禁止把 working loop 中 `phase_hint=None` 的普通 assistant text 追加到 final answer block。
- 禁止客户端把最后一个 assistant text block 本地改名为 final answer。
- 禁止 `FinalAnswer` item started 后继续 emit `Reasoning/Thinking`、`ToolCall/Acting` 或 `AssistantText/Commentary`；如果 provider 在 explicit final answer 后继续输出 tool call，应 fail fast 并记录 provider protocol violation。

## General Turn 状态机

```text
TurnStarted
  |
  v
UserInput/Input completed
  |
  v
WorkingLoop round N
  |
  +-> model block stream
  |     |
  |     +-> Reasoning                -> Thinking timeline
  |     +-> AssistantText phase=None -> Commentary timeline
  |     +-> AssistantText final_hint -> FinalAnswer timeline
  |     +-> ToolCall                 -> Acting timeline
  |
  +-> if tool calls emitted:
  |       execute tools
  |       append tool results to model context
  |       continue WorkingLoop round N+1
  |
  +-> if provider explicit final phase detected:
  |       complete active non-final items
  |       emit ExecutionSummary/System as final timeline item before final answer
  |       stream explicit final answer -> FinalAnswer timeline
  |       close turn
  |
  +-> if no pending tool call:
          loop terminal
          emit ExecutionSummary/System as final timeline item before final answer
          Finalization call, tools disabled
            |
            +-> AssistantText -> FinalAnswer timeline
          close turn
```

默认产品策略：交互式 WebUI/Desktop/TUI/CLI session 必须开启 finalization，即 `finalization_required=true`。这样最终总结不是猜出来的，而是 Talos 明确进入最终阶段后生成的。

低延迟内部 API 可以允许跳过 finalization，但其结果只能称为 `visible_output` 或 raw assistant output，不应在 public timeline 中标记为 `FinalAnswer`。

## Prompt 策略

Prompt 只用于约束模型行为，不作为协议边界。

可以要求模型：

- 工具调用前只输出必要的简短说明。
- 工具执行后根据结果继续分析。
- finalization 阶段给出面向用户的最终总结。

不应要求模型：

- 输出 XML/Markdown 标签标记 final answer。
- 自己声明当前阶段为 `thinking` / `acting` / `final_answer`。
- 用自然语言替代结构化 tool call。

对支持 Responses phase 的 provider，可以在 prompt 中说明 commentary 与 final answer 的使用期望；但 kernel 必须接受 phase 缺失的 provider，并通过 finalization 保证最终总结。

## Timeline 展示规则

| Model/kernel event | Public timeline | UI 展示 |
| --- | --- | --- |
| `Reasoning` | `Reasoning/Thinking` | 思考区域，增量展示，可折叠 |
| `AssistantText` in `WorkingLoop` with no final hint | `AssistantText/Commentary` | 中间说明，进入 timeline，不进入最终 answer |
| `ToolCall` | `ToolCall/Acting` | 工具名、参数、审批、进度、结果 |
| `AssistantText` in `Finalization` | `AssistantText/FinalAnswer` | 独立最终回答 block，增量展示 |
| provider explicit final answer | `AssistantText/FinalAnswer` | 独立最终回答 block，增量展示 |
| `ExecutionSummary` | `ExecutionSummary/System` | 中间 timeline 的最后一个 system item，必须在 final answer 之前显示 |

`ExecutionSummary` 属于 System 信息，保留在 timeline 中显示，但不参与 final answer 正文。它必须在 `FinalAnswer` item started 之前完成，并作为 thinking/acting/commentary timeline 的收尾 system item。`FinalAnswer` 之后不允许再追加 `ExecutionSummary` 或任何主流程 timeline item。

## 模块接口清单

| 模块 | 职责 |
| --- | --- |
| `llm-provider` / provider adapters | 把 OpenAI、Anthropic、Chat Completions 原始 stream 解析为 model block。 |
| `agent-kernel` | 维护 call mode、tool loop、finalization、model block 到 timeline phase 的转换。 |
| `agent-kernel/src/types.rs` | 定义 model block stream 与 phase hint 类型。 |
| `agent-kernel/src/general_turn` | 实现 working loop、tool execution、tools-disabled finalization。 |
| `agent-kernel/src/timeline.rs` | 生成 phase-aware timeline item，固化 final answer 后不再有主 timeline item 的不变量。 |
| `orchestration/agent` | 只做 kernel event 到 `TalosTimelineEvent` 的无损映射。 |
| `application-service` | 复用 `TimelineEventEnvelope` 持久化、恢复、订阅。 |
| `gateway/http` / `appserver` | 只做 SSE/JSON-RPC transport 映射。 |
| `ts-sdk` | 继续导出 timeline 类型。 |
| `web/features/chat` | 按 item lifecycle 渲染，不推断 phase；增加顺序保护。 |
| `ui-tui` / `cli` | 消费同一 timeline stream，展示 thinking/action/final answer。 |

## 核心 API 伪代码

```rust
pub enum KernelModelCallMode {
    WorkingLoop,
    Finalization,
}

pub enum KernelAssistantTextPhase {
    Commentary,
    FinalAnswer,
}

fn map_assistant_text_phase(
    call_mode: KernelModelCallMode,
    phase_hint: Option<AssistantTextPhaseHint>,
) -> KernelAssistantTextPhase {
    match call_mode {
        KernelModelCallMode::Finalization => KernelAssistantTextPhase::FinalAnswer,
        KernelModelCallMode::WorkingLoop => match phase_hint {
            Some(AssistantTextPhaseHint::FinalAnswer) => {
                KernelAssistantTextPhase::FinalAnswer
            }
            Some(AssistantTextPhaseHint::Commentary) | None => {
                KernelAssistantTextPhase::Commentary
            }
        },
    }
}

async fn run_general_turn() -> TalosResult<GeneralTurnOutput> {
    emit_user_input().await?;

    loop {
        let step = stream_model_round(KernelModelCallMode::WorkingLoop).await?;

        if step.has_tool_calls() {
            execute_tool_batch(step.tool_calls).await?;
            append_tool_results_to_context().await?;
            continue;
        }

        if step.has_provider_explicit_final_answer() {
            complete_turn().await?;
            break;
        }

        stream_finalization_call(KernelModelCallMode::Finalization).await?;
        complete_turn().await?;
        break;
    }
}
```

Provider adapter 伪代码：

```rust
fn map_anthropic_content_block(block: AnthropicBlock) -> ModelBlockStarted {
    match block.kind {
        AnthropicBlockKind::Thinking => ModelBlockStarted::reasoning(block.id),
        AnthropicBlockKind::Text => ModelBlockStarted::assistant_text(block.id, None),
        AnthropicBlockKind::ToolUse => ModelBlockStarted::tool_call(block.id, block.name),
    }
}

fn map_chat_completion_delta(choice: ChatCompletionChoiceDelta) -> Vec<ModelBlockStreamEvent> {
    // Chat Completions has no standard phase. Text maps to AssistantText with phase_hint=None.
    map_reasoning_text_and_tool_calls(choice)
}
```

## 迁移步骤

1. 定义 model block stream 类型，替代裸 provider delta 到 timeline 的直接投影。
2. 修改 provider adapter，把 Anthropic/OpenAI/Chat Completions stream 映射为 model block。
3. 在 `general_turn` 中引入 `KernelModelCallMode::{WorkingLoop, Finalization}`。
4. 修改 assistant text streaming：working loop 中缺 phase 的 text 实时进入 `Commentary`，finalization call 中 text 实时进入 `FinalAnswer`。
5. 增加 finalization policy：交互式 WebUI/Desktop/TUI/CLI session 默认 `finalization_required=true`，loop terminal 后执行 tools-disabled finalization。
6. 调整 `ExecutionSummary`：保留为 `ExecutionSummary/System` timeline item，但必须在 `FinalAnswer` 之前完成，并作为中间 timeline 的最后一个 system item。
7. 更新 `orchestration/agent` 映射测试，确保 agent 层不做 phase 推断。
8. 更新 WebUI/TUI/CLI reducer 与展示测试，确认客户端只消费服务端 phase。
9. 更新 integration e2e，覆盖 tool loop、无工具问答、provider phase 缺失、finalization streaming。

## 验证策略

Rust 单测：

- Anthropic `text + tool_use` 在 working loop 中生成 `AssistantText/Commentary`，不产生 `FinalAnswer`。
- Chat Completions `delta.content` 在 working loop 中映射为 `AssistantText/Commentary`。
- 当前 OpenAI provider 不从非标准 phase 字段伪造 `AssistantText/FinalAnswer`。
- Chat Completions `delta.content` 在 working loop 中映射为 `AssistantText/Commentary`。
- loop terminal 且无 provider explicit final answer 时，kernel 发起 tools-disabled finalization。
- finalization text 逐 delta 进入 `AssistantText/FinalAnswer`。
- `FinalAnswer` 后再出现 `Thinking/Acting/Commentary` 返回错误。
- 没有 active model block 的 delta fail fast。

前端/客户端测试：

- WebUI 收到 `Commentary` delta 时只更新 timeline，不追加到 final answer block。
- WebUI 收到 `FinalAnswer` delta 时逐字增量追加到最终回答。
- `ExecutionSummary/System` 在 final answer 之前显示，并且是中间 timeline 的最后一个 system item。
- TUI/CLI 以同一 timeline stream 展示 thinking/action/final answer。

集成验证：

- 普通无工具问答：finalization 生成最终总结并 live streaming。
- 工具调用问答：thinking/tool/progress 增量展示，工具完成后 finalization live streaming。
- provider phase 缺失模型：不会伪造 provider explicit final answer，但仍能通过 finalization 产出最终回答。
- 断线恢复：`TimelineEventEnvelope` replay 后仍保持 item 顺序和 phase。

建议命令：

```bash
cargo test -p talos-agent-kernel
cargo test -p talos-agent
cargo test -p talos-application-service
pnpm --dir talos-app/web test -- streaming.test.tsx MessageList.test.tsx
tests/integration/run_e2e.sh
```

## 风险与权衡

- 默认开启 explicit finalization 会多一次 LLM 调用，带来延迟和成本，但可以换取明确最终总结语义。
- Working loop 中缺 phase 的 assistant text 会被展示为 commentary；如果某些模型在无工具问题中直接给出完整答案，仍会再经过 finalization 形成最终回答，需要 prompt 控制 commentary 简短。
- provider explicit final answer 与 kernel finalization 同时存在时，应跳过 finalization，避免重复总结；但仍必须保证 `ExecutionSummary/System` 在 explicit final answer item started 之前完成。
- Provider adapter 需要逐步覆盖各类兼容协议，不能只按 OpenAI 或 Anthropic 单一路径设计。

## Open Questions

- CLI batch 是否允许低延迟 raw output 模式，以及关闭 finalization 时协议如何明确标记非 `FinalAnswer` 输出？
- 后续引入 OpenAI Responses provider mode 时，是否完全信任 Responses phase，还是通过配置白名单控制？
- finalization prompt 是否应由 `PromptBuilder` 提供独立静态段，确保工具禁用后只输出最终总结？

## 与既有 spec 的关系

- `refactor-019-message-timeline-model` 定义 public timeline item 生命周期，本 spec 不替换它。
- `refactor-020-application-timeline-recovery-stream` 定义 public envelope 和恢复流，本 spec 复用它。
- `refactor-021-kernel-stream-phase-ownership` 解决了 phase ownership 的第一阶段问题，本 spec 将其扩展为 provider-neutral model block 与 explicit finalization 设计。
