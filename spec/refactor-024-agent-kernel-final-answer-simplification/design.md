# Agent Kernel FinalAnswer 判定简化设计

Status: draft
Owner: agent
Created: 2026-05-23
Related: `refactor-023-agent-kernel-simplification`

## 背景

`refactor-023` 已经完成主要分层收敛：`application-service -> orchestration/agent -> agent-kernel/general_turn`。本 spec 不再重写分层，只解决 `agent-kernel/general_turn` 内部一个剩余问题：

**普通 agent loop 结束路径被 forced finalization、ExecutionSummary、stream phase 和旧 response processor 残留混在一起，导致 FinalAnswer 判定比实际需要复杂。**

Talos 需要保留自己的产品能力：

- 模型 reasoning / assistant text 增量流式反馈；
- tool args / tool call lifecycle 增量反馈；
- streaming tool execution；
- parallel / sequential tool batch；
- dynamic tool waiting / resume；
- 多端 timeline 投影与恢复。

但这些能力不应该改变核心 loop 判定：

```text
model round done
  -> has tool calls: commentary + execute tools + continue
  -> no tool calls: final answer + end turn
```

## 目标

1. 删除普通 terminal 后的二次模型 finalization。
2. 删除 `kernel_config.finalization_required` 和 `ForcedFinalizationCause::LoopTerminal`。
3. 将 FinalAnswer 判定收敛到模型 round 完成后的结构化 tool call 判断。
4. 保留 Talos 实时增量反馈和 streaming tool execution，不引入 provisional phase。
5. 将 `ExecutionSummary` 明确定义为本轮 kernel 执行 metrics/status，不再表达模型最终总结，也不影响 FinalAnswer 顺序和判定。
6. 清理不再服务主路径的 final answer / tool call response processor 残留。

## 非目标

- 不修改 `refactor-023` 文档。
- 不重写 application-service / AgentHarness 分层。
- 不移除 streaming tool execution。
- 不新增 `provisional phase`。
- 不重写既有 timeline item DTO 语义；允许增量新增 metrics 字段 / live-only metrics event，并同步 schema / SDK。
- 不引入新的 planner 模式。
- 不重新设计 dynamic tool approval。

## 核心决策

### 1. FinalAnswer 判定只在 round 收敛点提交

模型 streaming 过程中可以实时输出 reasoning、assistant text、tool args、tool lifecycle，但 kernel 不在普通无 phase hint 的 assistant text 开始时提交最终 FinalAnswer 判定。

最终判定点是当前 model round done 后的结构化结果。BuiltIn 模式规则：

```text
if finalized_tool_calls.is_empty():
    assistant text is FinalAnswer
else:
    assistant text is Commentary
```

PlanReAct 模式不能直接套用 BuiltIn 终止规则，见“PlanReAct final tag policy”。

### 1.1 No-hint assistant text 投影策略

本 spec 不引入 `provisional phase`，也不新增 phase correction / reclassify 事件。因此普通无 phase hint 的 assistant text 采用唯一策略：

```text
no-hint assistant text:
  - streaming 过程中先在 kernel 内缓冲，不立即发 AssistantTextStarted/Delta/Completed
  - round done 后按最终 disposition 一次性投影
  - no tool calls => FinalAnswer
  - has tool calls => Commentary
```

仍实时输出：

```text
reasoning delta
tool args delta
tool call start
tool queued / executing / progress / result
```

这意味着：没有 explicit phase hint 的普通 assistant 可见文本可能不再逐 token 实时展示；它会在当前 model round done 后一次性进入最终 phase。该取舍用于避免当前“先 Commentary，再补同文本 FinalAnswer”的重复展示。若未来要恢复 no-hint text 的逐 token 实时展示，必须另起设计引入 provisional 或 phase correction，不在本 spec 范围内。

### 2. Provider explicit final 仍可立即生效

如果 provider 明确给出 `AssistantTextPhaseHint::FinalAnswer`：

- 该 block 立即进入 FinalAnswer；
- 同一 round 后续不允许 tool call；
- 同一 round 后续不允许 commentary；
- round done 后直接 `EndTurn`。

这属于 provider 显式协议，不等同于 kernel 自己再次调用模型总结。

### 3. 普通 terminal 不再 forced finalization

删除普通 terminal 触发的：

```text
FinalResponse -> finalization_required -> ForcedFinalizationCause::LoopTerminal -> extra model call
```

普通 no-tool terminal 的 assistant text 就是最终答案。

### 4. Forced finalization 只处理异常旁路

保留以下异常场景：

| Cause | 触发条件 | 行为 |
| --- | --- | --- |
| `ToolRoundBudget` | 达到 `max_tool_rounds` 后仍有工具循环风险 | 再调用一次模型，要求基于已有证据输出最终答案 |
| `RepeatedReadOnlyInvestigation` | 重复等价只读工具探索 | 再调用一次模型，要求停止探索并回答 |
| `EmptyFinalResponseAfterTools` | 已执行工具后，模型返回 no-tool 但可见文本为空 | salvage 一次最终答案 |

不再存在：

```text
ForcedFinalizationCause::LoopTerminal
```

### 4.1 PlanReAct final tag policy

BuiltIn 模式的 stop decision 是：

```text
round done && tool_calls.is_empty()
```

PlanReAct 在 `require_final_answer_tag=true` 时必须额外满足 planner final tag：

```text
round done
  && tool_calls.is_empty()
  && planner_response.has_final_answer_tag
```

如果 PlanReAct 输出没有 tool call，但也没有 `FINAL_ANSWER` / final segment：

```text
- 不得按 BuiltIn 规则直接 EndTurn；
- 不得把纯 thought / no-progress 文本当作 FinalAnswer；
- 交给 PlanReAct policy 处理 no-progress / replan / guard；
- 超出策略预算后才进入异常 forced finalization 或失败路径。
```

实现边界：

- `GeneralTurnStepDisposition` 可以继续表达结构化 tool call 层面的 `FinalResponse` / `ContinueWithTools`；
- PlanReAct 是否接受该 `FinalResponse` 为 turn final，由 `GeneralTurnPolicy.after_step_result` 或 planner-private helper 决定；
- 旧 `FinalAnswerExtractionProcessor` 如果仍有 tag parsing 价值，只能下沉为 PlanReAct planner helper，不能作为 public kernel final 判定 API。

### 5. ExecutionSummary 是执行 metrics，不是模型总结

模型最后一轮无 tool call 的 assistant text 本身就是最终总结。

`ExecutionSummary` 只表达本轮 kernel 执行 metrics/status，例如：

- 本轮执行了多少次 tool call；
- 多少次 tool call 成功 / 失败；
- 是否触发 tool round budget；
- 是否进入 waiting dynamic input；
- 可选的 token usage / duration 等非模型语义指标。

它不是模型最终总结，不是 agent answer，不应该参与 FinalAnswer 判定，也不能成为 FinalAnswer 前置条件。

处理策略：

1. 保留或改名为系统 metrics/status 事件均可，但语义必须是 kernel-generated execution metrics。
2. 不再要求 FinalAnswer 前必须出现 `ExecutionSummary`。
3. 不再把 `ExecutionSummary` 写入模型上下文作为最终总结内容。
4. UI 如果展示它，应展示为系统状态/metrics，不应展示为 assistant answer 的一部分。
5. metrics payload 必须结构化，禁止用自然语言结论表达，例如不要写 `Kernel summary: executed 3 tool call(s) successfully before final response.`。

建议的结构化 payload：

```rust
pub struct TurnExecutionMetrics {
    pub model_rounds: u32,
    pub tool_calls: u32,
    pub tool_successes: u32,
    pub tool_errors: u32,
    pub stop_reason: StopReason,
    pub usage: TalosUsage,
    pub duration_ms: Option<u64>,
}
```

已有 `TalosTurnCompletedEvent.usage` 是完成态 token usage 的权威持久化来源。Execution metrics 不应复制一份自然语言 usage，而应引用或包含同一个强类型 `TalosUsage`。

强类型协议边界：

```rust
pub enum KernelStreamEventData {
    ...
    TurnMetricsUpdate(TurnExecutionMetrics),
}

pub struct TalosTurnCompletedEvent {
    ...
    pub usage: TalosUsage,
    pub metrics: Option<TurnExecutionMetrics>,
}
```

历史兼容边界：

- `TalosOutputItem::ExecutionSummary { text }` 只作为 legacy read 处理；
- `TalosTimelineContent::ExecutionSummary { summary }` 只作为 legacy read 处理；
- 新写入路径不再写自然语言 summary item；
- SDK / TS schema 必须生成 `TurnExecutionMetrics`；
- history mapper 对旧 summary item 做只读展示，不把它解释为 FinalAnswer 或 metrics 权威来源。

### 6. Usage / metrics 的持久化和多端展示

当前完成态 usage 已经有强类型链路：

```text
GeneralTurnResult.usage
  -> TalosTurnCompletedEvent.usage
  -> TimelineEventEnvelope
  -> session history / replay / SDK schema
```

因此历史加载和已完成 turn 的 token usage 应继续以 `TurnCompleted.usage` 为准。

本 spec 需要补齐的是正在执行态和重载恢复态的 metrics 展示。这里分成两类数据：

```text
running turn:
  - 多端可通过 live-only metrics update 展示当前累计 usage/tool counts
  - 这类增量更新不进入 timeline event 持久化日志
  - active turn snapshot 可保存 last known metrics，便于页面重载后立即显示

completed turn:
  - 以 TurnCompleted.usage 和最终 TurnExecutionMetrics 为准
  - history mapper 从 persisted timeline event 重建，不依赖 ExecutionSummary 文本
  - 最终 metrics 随 TurnCompleted / message record 持久化
```

running active snapshot 可选设计：

```rust
pub struct TimelineActiveTurnSnapshot {
    ...
    pub metrics: Option<TurnExecutionMetrics>,
}
```

持久化原则：

```text
live metrics update:
  - 只发给在线客户端
  - 不写 timeline_events
  - 不参与历史 replay

active turn last known metrics:
  - 可选写入 active_turns.metrics_json
  - 覆盖式更新，不保留每次增量历史
  - 只用于正在执行时页面重载 / active turn 查询

final metrics:
  - 写入 TurnCompleted.usage
  - 如需要 tool/model/duration 统计，写入最终 TurnExecutionMetrics
  - 作为历史和 retry 展示的权威来源
```

如果第一阶段不扩展 active turn 表结构，页面重载正在执行的 turn 时只依赖 replay 到 `last_event_id` 的 timeline 内容；metrics 等下一次 live update 再显示。不要为了 running metrics 把高频 delta 全量写入 timeline event log。

### 7. Streaming tool execution 保留

Talos 与 pi 的差异是产品能力，不是 loop 判定错误。

保留：

```text
ToolArgsDelta
ToolCallStart
ToolQueued
ToolExecuting
ToolProgressDelta
ToolResult
```

约束：

```text
streaming tool execution 可以提前执行工具；
但 turn terminal 只能在 model round done 后根据 finalized tool_calls 判定。
```

## 目标流程

```text
User input
    |
    v
prepare_general_turn
    |
    v
+------------------------------+
| execute_general_turn_loop    |
+------------------------------+
    |
    v
model round stream
    |-- reasoning delta --------------------> stream/timeline
    |-- explicit-hint assistant text delta -> stream/timeline
    |-- no-hint assistant text ------------> kernel buffer
    |-- tool args delta -------------------> stream/timeline
    |-- tool lifecycle/result -------------> stream/timeline
    v
model round done
    |
    +-- explicit final? ----------------------+
    |                                         v
    |                                  FinalAnswer + EndTurn
    |
    +-- no tool calls? -----------------------+
    |                                         v
    |                                  FinalAnswer + EndTurn
    |
    +-- has tool calls -----------------------+
                                              v
                                      execute/collect tool batch
                                              |
                        +---------------------+----------------------+
                        |                                            |
                        v                                            v
                  waiting dynamic                              append tool results
                        |                                            |
                        v                                            v
                 Waiting outcome                              next model round
```

异常旁路：

```text
tool round budget exhausted
  -> forced finalization once

repeated read-only investigation
  -> forced finalization once

empty final response after tools
  -> salvage finalization once
```

## 核心 API 伪代码

### Loop

```rust
async fn execute_general_turn_loop(...) -> TalosResult<ExecutedTurnArtifacts> {
    let mut state = GeneralTurnLoopState::new(&prepared);

    loop {
        let step = execute_general_turn_step(...).await?;

        match policy.after_step_result(&mut step)? {
            GeneralTurnPolicyDecision::Continue => {}
            GeneralTurnPolicyDecision::AcceptFinal => {
                step.disposition = GeneralTurnStepDisposition::FinalResponse;
            }
            GeneralTurnPolicyDecision::ContinueWithNudge { message } => {
                apply_general_turn_step(&mut state, &step);
                state.messages.push(message);
                continue;
            }
            GeneralTurnPolicyDecision::ForceFinalization { cause } => {
                let forced = run_forced_finalization_step(..., cause).await?;
                apply_general_turn_step(&mut state, &forced);
                return finalize(state, forced.text, stop_reason_for(cause));
            }
            GeneralTurnPolicyDecision::Stop { output_text, stop_reason } => {
                apply_general_turn_step(&mut state, &step);
                return finalize(state, output_text, stop_reason);
            }
            GeneralTurnPolicyDecision::Fail { error } => {
                return Err(error);
            }
        }

        if let Some(cause) = abnormal_forced_finalization_cause(&mut guard, &state, &step) {
            let forced = run_forced_finalization_step(..., cause).await?;
            apply_general_turn_step(&mut state, &forced);
            return finalize(state, forced.text, stop_reason_for(cause));
        }

        apply_general_turn_step(&mut state, &step);

        if step.disposition == GeneralTurnStepDisposition::FinalResponse {
            state.messages.push(ChatMessage::assistant(step.text.clone()));
            let stop_reason = match step.model_finish_reason {
                ModelFinishReason::Length => StopReason::ModelOutputLimit,
                _ => StopReason::EndTurn,
            };
            return finalize(state, step.text, stop_reason);
        }

        let batch = collect_or_execute_tool_batch(&step).await?;

        if batch.has_waiting_action() {
            apply_general_turn_tool_batch(&mut state, &batch);
            return finalize(state, "", StopReason::WaitingDynamicInput);
        }

        apply_general_turn_tool_batch(&mut state, &batch);

        if state.round >= max_tool_rounds {
            let forced = run_forced_finalization_step(..., ToolRoundBudget).await?;
            apply_general_turn_step(&mut state, &forced);
            return finalize(state, forced.text, StopReason::ToolRoundLimit);
        }
    }
}
```

Policy decision API：

```rust
pub enum GeneralTurnPolicyDecision {
    /// BuiltIn default: do not override the structural step disposition.
    Continue,
    /// Explicitly accept a no-tool step as final.
    AcceptFinal,
    /// Reject the apparent final response, add a policy nudge, and continue model loop.
    ContinueWithNudge { message: ChatMessage },
    /// Force a final answer through the abnormal finalization path.
    ForceFinalization { cause: ForcedFinalizationCause },
    /// Stop without another model call.
    Stop { output_text: String, stop_reason: StopReason },
    /// Fail the turn.
    Fail { error: TalosError },
}
```

BuiltIn policy returns `Continue` and accepts structural `FinalResponse` in the loop. PlanReAct policy can return `ContinueWithNudge` when `require_final_answer_tag=true` and the no-tool response lacks a final tag; this prevents the apparent `FinalResponse` from falling through to normal finalization.

### Step 判定

```rust
fn classify_round(
    round: StreamRound,
    planner_policy: &dyn GeneralTurnPolicy,
) -> GeneralTurnStepDisposition {
    if round.tool_calls.is_empty() {
        GeneralTurnStepDisposition::FinalResponse
    } else {
        GeneralTurnStepDisposition::ContinueWithTools
    }
}
```

`GeneralTurnStepDisposition::FinalResponse` 只表示结构化 tool call 为空。BuiltIn policy 会接受它作为 turn final；PlanReAct policy 可以基于 final tag / no-progress / replan 状态拒绝该 final，并返回 policy stop、forced finalization 或继续策略动作。

### Stream phase

```rust
enum AssistantTextPhaseDecision {
    ExplicitFinal,
    RoundTerminalFinal,
    RoundCommentary,
}

fn decide_assistant_text_phase(round: &StreamRound) -> KernelAssistantTextPhase {
    if round.provider_explicit_final {
        return KernelAssistantTextPhase::FinalAnswer;
    }
    if round.tool_calls.is_empty() {
        KernelAssistantTextPhase::FinalAnswer
    } else {
        KernelAssistantTextPhase::Commentary
    }
}
```

本 spec 不要求引入 `provisional phase`。无 phase hint 的 ordinary assistant text 必须缓冲到 round done 后再按最终 phase 一次性投影；不能继续“先按 Commentary 实时发，再由 finalize 补同文本 FinalAnswer”。

## 模块接口清单

| 模块 | 当前问题 | 目标改动 |
| --- | --- | --- |
| `general_turn/loop_run.rs` | `FinalResponse` 后仍可通过 `finalization_required` 触发 `LoopTerminal` extra model call。 | 删除普通 terminal forced finalization；`FinalResponse` 直接 `EndTurn` / `ModelOutputLimit`。 |
| `general_turn/step_execute.rs` | WorkingLoop 无 phase hint 时默认 Commentary，direct answer 容易先 commentary 后补 final。 | no-hint assistant text 缓冲到 round done 后一次性投影；explicit final hint 保持立即生效。 |
| `general_turn/types.rs` | `ForcedFinalizationCause::LoopTerminal` 表达普通完成路径。 | 删除 `LoopTerminal`；只保留异常 forced finalization cause。 |
| `general_turn/guard.rs` | forced finalization prompt 同时服务普通 terminal 和异常场景。 | 删除 `LoopTerminal` 分支；prompt 只服务异常收敛。 |
| `general_turn/finalize.rs` | `ExecutionSummary` 被当作 final answer 前置流事件，语义容易被误解为模型总结。 | 将 `ExecutionSummary` 明确收敛为 kernel execution metrics/status；final artifacts 的 assistant answer 只来自最终 no-tool round 文本。 |
| `common/types/src/talos.rs` | `TalosTurnCompletedEvent.usage` 已有完成态 usage，但没有最终 tool/model/duration metrics。 | 保留 `TurnCompleted.usage` 为完成态权威 usage；新增 `TurnExecutionMetrics` 和 `TalosTurnCompletedEvent.metrics`。 |
| `gateway/app-protocol/src/requests.rs` | active turn snapshot 只暴露 status / pending action / last event。 | 可选增加 last known metrics；这是覆盖式 active snapshot，不是 timeline event log。 |
| `session-store/active_turn.rs` | active turn 只保存 status/pending/last_event。 | 可选增加 `metrics_json` 保存 running turn last known metrics；覆盖式更新，不保留增量历史。 |
| `infra/config/src/domains/kernel.rs` | `finalization_required` 允许普通 terminal 二次调模型。 | 删除 `AgentKernelConfig.finalization_required` 及默认值。 |
| `processor/response*.rs` | 旧 `FinalAnswerExtractionProcessor` / `ResponseDisposition` 与 `GeneralTurnStepDisposition` 重复。 | 若已不在主路径使用，删除；若 PlanReAct 仍需要 final tag parsing，收敛为 planner-private helper，并接入 `GeneralTurnPolicy`。 |
| `planner.rs` / PlanReAct policy | `require_final_answer_tag=true` 时，no-tool response 不能天然视为 final。 | BuiltIn 接受 `tool_calls.is_empty()`；PlanReAct 必须同时满足 final tag，否则走 no-progress/replan/guard。 |
| `orchestration/agent/src/agent.rs` | timeline adapter 要求 final answer 前必须看到 execution summary。 | 取消该要求；FinalAnswer 由 kernel event/outcome 判定，系统状态通知不参与 final ordering。 |

## 文件级迁移计划

### Phase 1: 删除普通 terminal finalization

目标：不改变 streaming tool execution，只移除普通完成路径的额外模型调用。

文件：

- `talos-rs/crates/orchestration/agent-kernel/src/general_turn/loop_run.rs`
- `talos-rs/crates/orchestration/agent-kernel/src/general_turn/types.rs`
- `talos-rs/crates/orchestration/agent-kernel/src/general_turn/guard.rs`
- `talos-rs/crates/infra/config/src/domains/kernel.rs`
- `talos-rs/conf/talos.yaml`
- `talos-app/desktop/resources/conf/talos.yaml`

改动：

- 删除 `AgentKernelConfig.finalization_required`。
- 删除 `ForcedFinalizationCause::LoopTerminal`。
- 删除 `FinalResponse` 分支中的 `run_forced_finalization_step(... LoopTerminal ...)`。
- `FinalResponse` 直接返回 `EndTurn` 或 `ModelOutputLimit`。
- 同步删除默认配置、示例配置、测试配置中的 `finalization_required`。
- 因 `AgentKernelConfig` 使用 `deny_unknown_fields`，本项目未上线阶段接受破坏性配置清理；所有仓库内配置必须同改。
- 如果发现用户本地配置仍带旧键，启动应明确报 unknown field，提示删除该配置；不保留 deprecated ignored field。

验证：

- direct answer 只调用一次模型。
- no tool calls 直接 `EndTurn`。
- tool 后最终 no tool calls 直接 `EndTurn`。
- `ToolRoundBudget` / `RepeatedReadOnlyInvestigation` / `EmptyFinalResponseAfterTools` 仍可 forced finalization。
- 仓库内配置文件不再包含 `finalization_required`。
- `config_behavior` 测试删除该字段断言。

### Phase 2A: 删除 ExecutionSummary 前置语义和自然语言 summary

目标：ExecutionSummary 不再是 FinalAnswer 的前置语义；新写入路径不再产生自然语言 summary。

文件：

- `talos-rs/crates/orchestration/agent-kernel/src/general_turn/finalize.rs`
- `talos-rs/crates/orchestration/agent-kernel/src/types.rs`
- `talos-rs/crates/orchestration/agent/src/agent.rs`
- `talos-rs/crates/common/types/src/talos.rs`
- `talos-rs/crates/orchestration/application-service/src/history.rs`

改动：

- 删除 timeline adapter 中 “final answer requires execution summary first” 的强约束。
- `finalize_general_turn_artifacts` 不再把 `TalosOutputItem::ExecutionSummary` 当作 assistant answer 或最终总结。
- 新写入路径不再写 `TalosOutputItem::ExecutionSummary { text }`。
- 新写入路径不再写 `TalosTimelineContent::ExecutionSummary { summary }`。
- 旧历史中的 ExecutionSummary 只做 legacy read / system display。
- `TalosTurnCompletedEvent.usage` 继续作为完成态 token usage 权威持久化字段。
- history/replay 从 `TurnCompleted.usage` 和最终 metrics 重建，不读取自然语言 summary。

验证：

- FinalAnswer 可以在没有 ExecutionSummary 的情况下正常投影。
- 历史回放不依赖 ExecutionSummary 判断 final answer。
- UI/TUI/WebUI 不把 ExecutionSummary 当 answer 内容。
- ExecutionSummary/metrics 内容与模型最终总结无关，只反映 kernel 执行指标。
- 已完成 turn 的 token usage 仍能从 `TurnCompleted.usage` 加载。

### Phase 2B: 新增最终结构化 execution metrics

目标：完成态 metrics 强类型持久化，不使用自然语言 summary。

文件：

- `talos-rs/crates/common/types/src/talos.rs`
- `talos-rs/crates/orchestration/agent-kernel/src/general_turn/types.rs`
- `talos-rs/crates/orchestration/agent-kernel/src/general_turn/finalize.rs`
- `talos-rs/crates/orchestration/agent/src/agent.rs`
- `talos-rs/crates/gateway/app-protocol/src/schema.rs`
- TS SDK/schema 生成相关文件

改动：

- 新增 `TurnExecutionMetrics` 强类型。
- `TalosTurnCompletedEvent` 增加 `metrics: Option<TurnExecutionMetrics>`。
- `ExecutedTurnArtifacts` 或等价结果中携带 final metrics。
- final metrics 与 `TurnCompleted.usage` 一起作为完成态权威来源。
- 更新 schema / TS SDK。

验证：

- completed turn replay 可以读取 `TurnCompleted.metrics`。
- token usage 仍来自 `TurnCompleted.usage`。
- metrics 不出现自然语言 summary 字段。

### Phase 2C: running metrics live update / active snapshot

目标：在线执行时多端可显示累计 metrics；页面重载可选显示 last known metrics。

文件：

- `talos-rs/crates/orchestration/agent-kernel/src/types.rs`
- `talos-rs/crates/orchestration/agent/src/agent.rs`
- `talos-rs/crates/gateway/app-protocol/src/requests.rs`
- `talos-rs/crates/orchestration/session-store/src/active_turn.rs`

改动：

- 新增 live-only `KernelStreamEventData::TurnMetricsUpdate(TurnExecutionMetrics)`。
- 该 update 只发 live stream，不写 `timeline_events`。
- 可选增加 `active_turns.metrics_json`，覆盖式保存 last known metrics。
- 可选在 `TimelineActiveTurnSnapshot` 增加 `metrics: Option<TurnExecutionMetrics>`。

验证：

- 在线客户端可收到 running metrics update。
- timeline replay 不包含高频 metrics update。
- 若实现 active snapshot metrics，reload 正在执行 turn 可立即读取 last known metrics。
- 若未实现 active snapshot metrics，reload 后等待下一次 live metrics update，不视为 Phase 2B 阻塞。

### Phase 3: 收敛 stream text phase 判定

目标：普通 assistant text 的最终 phase 与 round disposition 一致。

文件：

- `talos-rs/crates/orchestration/agent-kernel/src/general_turn/step_execute.rs`
- `talos-rs/crates/orchestration/agent-kernel/src/general_turn/tests/stream_tests.rs`
- `talos-rs/crates/orchestration/agent/src/agent/tests.rs`

改动：

- explicit final hint 继续立即输出 FinalAnswer。
- 无 explicit hint 的 ordinary text 在 streaming 过程中缓冲。
- round done 后按 `tool_calls.is_empty()` 一次性发 AssistantTextStarted/Delta/Completed。
- 不新增 public provisional phase。
- 禁止继续把 no-hint text 先作为 Commentary 发出再补 FinalAnswer。

验证：

- explicit final 后再出现 tool call 报协议错误。
- round done 且 no tool calls 输出 FinalAnswer。
- round done 且 has tool calls 输出 Commentary。
- direct answer 不出现同文本 Commentary + FinalAnswer 重复展示。
- tool args / tool lifecycle 仍实时输出。

### Phase 3B: PlanReAct final acceptance policy

目标：PlanReAct 不被 BuiltIn 的 no-tool stop rule 误判。

文件：

- `talos-rs/crates/orchestration/agent-kernel/src/planner.rs`
- `talos-rs/crates/orchestration/agent-kernel/src/general_turn/types.rs`
- `talos-rs/crates/orchestration/agent-kernel/src/general_turn/loop_run.rs`
- `talos-rs/crates/orchestration/agent-kernel/src/processor/response_processors.rs`

改动：

- PlanReAct final tag parsing 下沉为 planner-private helper。
- `GeneralTurnPolicy.after_step_result` 对 PlanReAct `FinalResponse` 做 final acceptance。
- `require_final_answer_tag=true` 时，必须 `tool_calls.is_empty() && has_final_answer_tag` 才 EndTurn。
- no-tool 但无 final tag 的 PlanReAct 输出进入 no-progress / replan / guard，不直接 EndTurn。

验证：

- BuiltIn no-tool answer 仍直接 EndTurn。
- PlanReAct 有 final tag 且 no tool calls 才 EndTurn。
- PlanReAct 无 final tag 且 no tool calls 不被当作 FinalAnswer。

### Phase 4: 清理旧 response processor 残留

目标：kernel 内只保留一套 FinalAnswer / tool call 判定。

文件：

- `talos-rs/crates/orchestration/agent-kernel/src/processor/response_processors.rs`
- `talos-rs/crates/orchestration/agent-kernel/src/processor/response.rs`
- `talos-rs/crates/orchestration/agent-kernel/src/processor/response_kinds.rs`
- `talos-rs/crates/orchestration/agent-kernel/src/lib.rs`

改动：

- 删除或下沉 `FinalAnswerExtractionProcessor`。
- 删除或下沉 `ToolCallExtractionProcessor`。
- 删除或下沉 `ResponseDisposition`。
- `lib.rs` 不再 public export 旧 final 判定类型。

验证：

- `general_turn` 是唯一 final/tool loop 判定路径。
- PlanReAct tag parsing 只能作为 planner helper + policy final acceptance，不拥有独立 loop。

## 行为测试清单

必须覆盖：

1. direct no-tool answer：
   - provider 调用次数为 1；
   - stop reason 为 `EndTurn`；
   - output text 等于模型返回文本；
   - 不触发 forced finalization。
   - stream 中不出现同文本 Commentary + FinalAnswer 重复。

2. tool loop：
   - 第一轮有 tool call；
   - tool batch 完成后进入第二轮；
   - 第二轮 no tool call；
   - 第二轮文本作为 FinalAnswer；
   - 不触发普通 terminal finalization。

3. multi tool batch：
   - 一轮多个 tool call；
   - parallel safe 可以并行；
   - mutating / serial 工具按策略串行；
   - batch 未完成前不进入下一 model round。

4. streaming lifecycle：
   - reasoning delta 可实时输出；
   - explicit-hint assistant delta 可实时输出；
   - no-hint assistant text 在 round done 后一次性投影；
   - tool args / queued / executing / result 可实时输出；
   - streaming tool execution 不提前决定 turn terminal。

5. waiting dynamic：
   - dynamic pending 返回 `WaitingDynamicInput`；
   - 不产出 FinalAnswer；
   - resume 后 replay dynamic result，再进入下一 model round。

6. abnormal forced finalization：
   - max tool rounds exhausted；
   - repeated read-only investigation；
   - empty final response after tools。

7. PlanReAct final acceptance：
   - `require_final_answer_tag=true`；
   - no tool calls + final tag => EndTurn；
   - no tool calls + no final tag => 不 EndTurn，进入 policy 处理。

必须删除或改写：

- `finalization_required should default to true`；
- `LoopTerminalFinalizationStreamingProvider` 相关测试；
- FinalAnswer 前必须 ExecutionSummary 的测试；
- direct answer 先 Commentary 再通过 LoopTerminal finalization 补 FinalAnswer 的测试。
- no-hint direct answer 先 Commentary 再补同文本 FinalAnswer 的测试。

## 风险与处理

| 风险 | 影响 | 处理 |
| --- | --- | --- |
| 调整 `ExecutionSummary` 影响 UI 展示 | 多端系统状态展示需要适配 | 保留为 metrics/status 系统通知，不参与 FinalAnswer 语义 |
| running turn metrics 不写 timeline event log | 页面重载后无法通过 replay 恢复当前累计 usage/tool counts | active turn 保存覆盖式 last known metrics；若暂不实现，则 reload 后等下一次 live update |
| 普通 no-hint assistant text 缓冲影响实时文本展示 | UI 要等 round done 才显示该文本 | 保留 reasoning/tool lifecycle 实时；explicit final 仍实时；未来如需 no-hint text 实时展示另起 provisional/correction 设计 |
| 旧历史 session 中有 ExecutionSummary item | 回放兼容需要处理 | history mapper 对旧 item 做只读展示，不作为新写入主路径 |
| PlanReAct 旧 final tag parsing 依赖 processor | 编译或行为回归 | 将 parsing helper 内聚到 planner，并通过 `GeneralTurnPolicy` 做 final acceptance |
| 删除 `finalization_required` 后旧配置加载失败 | 本地用户配置仍带旧键时启动失败 | 项目未上线阶段接受破坏性清理；仓库配置全量删除；错误提示指向移除旧键 |

## 完成定义

- `finalization_required` 完全从 config 和代码中删除。
- `ForcedFinalizationCause::LoopTerminal` 完全删除。
- 普通 no-tool terminal 不再额外调用模型。
- BuiltIn FinalAnswer 判定由 `round done && tool_calls.is_empty()` 或 provider explicit final 决定。
- PlanReAct `require_final_answer_tag=true` 时，FinalAnswer 必须同时满足 final tag。
- no-hint assistant text 不再出现同文本 Commentary + FinalAnswer 重复。
- Streaming tool execution 保留并通过测试。
- ExecutionSummary legacy read only；新写入路径使用结构化 metrics，不再作为 FinalAnswer 前置语义或模型总结。
- 旧 response processor 不再作为 public kernel final 判定 API。
- 相关 Rust tests / cargo check 通过。
