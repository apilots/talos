# 流式交替执行 Part 1：核心架构与执行路径

## 1. 背景

Talos 当前的问题不是不能流式输出，而是工具执行仍然发生在采样结束之后：

```text
stream_round()
  -> stream text / thinking
  -> buffer tool call

execute_general_turn_tool_batch()
  -> execute all tools
  -> emit tool result
```

这会产生明显空窗。  
Part 1 只解决一个问题：如何让工具在 assistant stream 期间进入调度与执行，同时不破坏当前 `general_turn` 主路径。

---

## 2. 设计结论

采用三层结构：

```text
stream_round()
  -> consume provider events
  -> emit output / thinking / tool-args
  -> finalize tool call
  -> submit call to StreamingToolScheduler
  -> opportunistically drain completed outputs

StreamingToolScheduler
  -> queue / start / cancel / discard tool tasks
  -> expose progress and completed updates

ToolBatchCollector
  -> convert ToolExecutionOutput[]
  -> build GeneralTurnToolBatch-compatible result
```

一句话：

- `stream_round()` 只负责采样与投递
- `StreamingToolScheduler` 负责执行与收口
- `ToolBatchCollector` 负责业务规约

---

## 3. 当前代码边界

### 3.1 `step_execute.rs`

当前 [step_execute.rs](/Users/xujiang/data/ailabs/talos/talos-rs/crates/orchestration/agent-kernel/src/general_turn/step_execute.rs:63) 只负责：

- 消费 provider stream
- emit delta 事件
- 汇总 `tool_calls`
- 返回 `StreamRound`

Part 1 保持这个边界，不让它直接承担 tool payload 构造、pending request 提取、memory 提取等职责。

### 3.2 `tool_batch.rs`

当前 [tool_batch.rs](/Users/xujiang/data/ailabs/talos/talos-rs/crates/orchestration/agent-kernel/src/general_turn/tool_batch.rs:191) 除执行外，还承担：

- `ToolExecutionOutput -> payload`
- `TalosOutputItem::ToolResult`
- `ChatMessage::tool(...)`
- `pending_confirmations`
- `pending_dynamic_requests`
- `retrieved_memories`
- `ToolResultLog`

这些业务规则必须继续复用，不能重写一套新的流式路径版本。

### 3.3 `tool_execution.rs`

当前 [tool_execution.rs](/Users/xujiang/data/ailabs/talos/talos-rs/crates/orchestration/agent-kernel/src/tool_execution.rs:132) 的并发能力是批级的：

- 整批都允许并发才并发
- 否则退回串行

Part 1 明确新增单工具调度层，不把现有 `ParallelToolExecutor` 误当作流式调度器。

---

## 4. 新增模块

新增：

```text
talos-rs/crates/orchestration/agent-kernel/src/tool_streaming.rs
```

包含：

- `StreamingToolScheduler`
- `StreamingToolState`
- `StreamingToolUpdate`
- 调度、取消、discard、收口逻辑

不新增“只是为了搬运参数”的中间 options 结构，继续复用现有 config / execution context。

---

## 5. 核心数据结构

### 5.1 `ToolStreamKey`

```rust
pub struct ToolStreamKey {
    pub round: u32,
    pub index: usize,
}
```

用途：

- 解决 tool args delta 阶段 `call_id` 尚未稳定的问题
- 作为 UI 临时关联 key

### 5.2 `StreamRound`

```rust
pub(crate) struct StreamRound {
    pub(crate) content: String,
    pub(crate) reasoning: String,
    pub(crate) tool_calls: Vec<FunctionCall>,
    pub(crate) tool_outputs: Vec<ToolExecutionOutput>,
    pub(crate) finish_reason: Option<String>,
    pub(crate) saw_output_delta: bool,
    pub(crate) usage: TalosUsage,
}
```

### 5.3 `StreamingToolUpdate`

```rust
pub enum StreamingToolUpdate {
    EventOnly(KernelStreamEvent),
    Completed {
        event: KernelStreamEvent,
        output: ToolExecutionOutput,
        index: usize,
    },
}
```

含义：

- UI 事件与真正进入下一轮上下文的工具输出同时来自 scheduler
- 避免 UI 和上下文走两套独立推断逻辑

---

## 6. 事件模型

保留现有：

- `ToolCallStart`
- `ToolResult`

新增：

- `ToolArgsDelta`
- `ToolQueued`
- `ToolExecuting`

```rust
pub enum KernelStreamEventType {
    OutputDelta,
    ThinkingDelta,
    ExecutionSummary,
    ToolCallStart,
    ToolArgsDelta,
    ToolQueued,
    ToolExecuting,
    ToolResult,
    PromptWarning,
    ConfirmationRequest,
}
```

事件原则：

1. `ToolArgsDelta` 用 `stream_key` 关联
2. `ToolCallStart` 表示 finalized call 已经形成
3. `ToolQueued` 表示已交给调度器，但未必立刻执行
4. `ToolExecuting` 表示任务真实开始
5. `ToolResult` 只表示终态

---

## 7. 调度策略

### 7.1 原则

不采用“tool call 一闭合就全部 `spawn`”。

调度规则：

1. 只读且支持并发的工具可并发执行
2. mutating tool 必须独占执行
3. 需要 approval / dynamic response 的工具不进入普通执行池
4. 若独占工具执行中，后续工具进入 `queued`

### 7.2 能力来源

首版继续复用当前 registry 能力：

- `tool_supports_parallel(tool_name)`
- `tool_is_mutating(tool_name)`

不在本次引入新的 capability schema。

---

## 8. `stream_round()` 新职责

`stream_round()` 的职责改为：

1. 消费 provider stream
2. emit `OutputDelta` / `ThinkingDelta` / `ToolArgsDelta`
3. 在 `ToolCallEnd` 时 finalize call 并投递给 scheduler
4. 在流中 opportunistic drain 已完成结果
5. 在流末统一 `finish()` 剩余结果

不负责：

- 直接执行单工具
- 构造 tool result payload
- 构造 `ChatMessage::tool(...)`
- 识别 `pending_confirmations`
- 识别 `pending_dynamic_requests`

---

## 9. 主循环对接

`prepare.rs` 继续保留为唯一 `general_turn` 主循环入口。

但 `execute_general_turn_step()` 改为返回已收敛的工具结果：

```rust
pub struct GeneralTurnStepResult {
    pub text: String,
    pub reasoning: Vec<String>,
    pub tool_calls: Vec<FunctionCall>,
    pub tool_outputs: Vec<ToolExecutionOutput>,
    pub output_items: Vec<TalosOutputItem>,
    pub pending_confirmations: Vec<PendingToolConfirmation>,
    pub pending_dynamic_requests: Vec<PendingDynamicToolRequest>,
    pub retrieved_memories: Vec<RetrievedMemoryItem>,
    pub tool_logs: Vec<ToolResultLog>,
    pub tool_error_count: usize,
    pub usage: TalosUsage,
    pub saw_output_delta: bool,
    pub disposition: GeneralTurnStepDisposition,
}
```

主循环改为：

- `apply_general_turn_step(&mut state, &step)`
- 若有 pending confirmation / dynamic request，终止当前 turn
- 否则推进到下一轮

不再在 `prepare.rs` 里额外调用“独立的流后工具批执行”。

---

## 10. ToolBatchCollector

建议从 `tool_batch.rs` 抽出纯收敛层：

```rust
pub fn collect_general_turn_tool_batch(
    outputs: Vec<(usize, ToolExecutionOutput)>,
) -> TalosResult<GeneralTurnToolBatch>
```

要求：

- UI 事件顺序按真实完成顺序
- `ChatMessage::tool(...)` 顺序按原始 tool call index
- payload / log / memory / pending 行为与当前一致

---

## 11. 错误与终态

任何进入 scheduler 的 tool call，必须进入稳定终态之一：

- `completed`
- `error`
- `cancelled`
- `discarded`

错误来源包括：

- 工具业务错误
- timeout
- task panic
- user interrupt
- provider retry / stale execution discard

不允许出现“UI 看到了 tool call，但上下文里没有对应 tool result”的状态。

---

## 12. 本部分完成标准

Part 1 完成后，必须满足：

1. 工具可在 assistant stream 期间启动执行
2. `stream_round()` 不承担业务收敛职责
3. 主循环仍保持 `general_turn` 单一路径
4. 结果顺序与上下文顺序都可预测
5. 任一已接收 tool call 都有稳定终态
