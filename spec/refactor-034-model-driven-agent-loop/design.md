# Model-driven Agent Loop 与 Context Compaction 设计草案

Status: draft
Owner: agent
Created: 2026-07-04
Updated: 2026-07-04
Related: `refactor-033-agent-framework-api-standardization`, `refactor-032-agent-harness-api`, `refactor-022-provider-neutral-agent-stream`

## 背景

Talos 当前 Agent Kernel 已经具备模型循环、工具调用、stream event、session history compaction、memory summary、dynamic tool result、timeline recovery 和中断能力。但是主执行路径仍保留 `agent.kernel.max_tool_rounds` 与 `ToolRoundLimit` 语义：当工具轮次达到配置上限时，Kernel 会强制进入 finalization。

固定工具轮次对短任务有保护作用，但对编程、文档生成、PPT 生成、跨文件修改、长时间调研等任务不合适。长任务的核心不是固定轮次问题，而是模型是否仍在有效推进任务，以及系统资源边界是否还能支撑继续执行。`max_tool_rounds` 会把产品行为绑定到实现计数，导致模型仍需要继续调用工具时被迫收尾。

同时，当前 context compaction 是阻塞阶段：自动 session history compaction 发生在 `prepare_general_turn` 中，执行期间不会进入下一次主路径 model request。如果多端没有明确的 compaction timeline 状态，客户端会看到 turn started 后长时间没有 delta，表现像“卡住”。因此本 spec 不只删除固定轮次限制，还要把 context compaction 建模为可恢复、可订阅、可渲染的 run 内部生命周期。

## 改造前实现事实

1. `conf/talos.yaml`、`conf/talos-dev.yaml`、`conf/talos-prod.yaml` 当前存在 `agent.kernel.max_tool_rounds: 8`。
2. `talos-rs/crates/orchestration/agent-kernel/src/general_turn/loop_run.rs` 当前在 `state.round >= max_tool_rounds` 时触发 forced finalization。
3. Kernel 和公共 `StopReason` 当前存在 `ToolRoundLimit`。
4. 自动 session history compaction 在 `prepare_general_turn` 阶段触发，调用 `compact_session_history_if_needed(...).await`，因此会阻塞当前 turn 的下一次主路径 model request。
5. 自动 session history compaction 内部通过 `flush_session_memory_with_report` 执行 memory summary 和 memory persistence decision，二者都是 `llm_provider.chat(...)` 非流式 JSON helper call，失败时已有 retry 和 rejected summary 语义。
6. manual `session.compact` 会先要求 session idle，然后同步等待 `flush_session_memory_with_report` 返回，是阻塞式管理操作。
7. 旧 `CompactionRequestProcessor` 只做本地 transcript summary event，不触发 LLM 调用；它不是本 spec 主要处理的 context-window compaction 路径。
8. session title generation 当前使用 `llm_provider.chat(...)`，属于非流式 helper call。
9. forced finalization / salvage finalization 是主 run 的 finalization model step，不属于 utility model call；在 streamed run 中它可以复用 streaming path 输出 final answer delta。
10. Talos 已有 `timeline.turn.error`、`TimelineActiveTurnStatus::Failed`、SSE timeline subscription 和 `timeline.recover`，但 `TalosTurnErrorEvent` 目前只有 `error: String`，缺少类似 Codex `CodexErrorInfo` 的结构化错误分类。
11. PromptBuilder 已经区分 static prefix 和 dynamic suffix；`TrimToBudget` 只裁剪 dynamic suffix，不裁剪 cacheable static prefix。
12. `SummaryRecord` 当前已有 `covers_until_seq`，但 prompt context 加载 recent messages 时还没有用它过滤已 compact 的 raw history。
13. 当前 compact source 更接近“最近窗口”输入，而不是严格按“上一次 compact 游标之后的未压缩 turn 区间”推进。
14. 当前 active run 内产生的 tool call / tool result 追加在 Kernel `state.messages`，不会经过 PromptBuilder 的 session history compact 路径；这是长任务去掉固定轮次后需要独立治理的 working context 问题。

## 第一性原则

1. Agent run 的完成条件应来自任务语义：模型给出 final response、模型等待外部输入、用户中断、权限拒绝、资源耗尽或不可恢复错误。
2. 工具轮次和模型调用次数是诊断指标，不应成为主路径默认产品语义。
3. 长任务需要稳定的资源治理，而不是隐藏的 turn limit。
4. Context compaction 是 run 内部阻塞维护阶段，不是用户请求、不是子 run、也不是新的 turn。
5. 阻塞维护阶段必须通过 timeline 明确暴露给客户端，否则 Web / TUI / SDK 会把正常维护误判为卡住。
6. Helper model call 必须 bounded：非流式、无工具、可计量、有 timeout、有 input / output cap。
7. 错误分类属于 turn lifecycle / structured error info，不属于 public `StopReason`。
8. `run_id == turn_id` 是 Agent domain 的唯一身份不变量；不能引入第二套 run identity。
9. Session history compact 必须按已完成 turn 的 coverage 区间严格推进，避免重复压缩同一段历史。
10. PromptBuilder 可以为了短期新鲜度回补少量已被 summary 覆盖的 recent tail，但这只属于 prompt view 策略，不改变 compact coverage。

## 目标

1. 将主 Agent loop 标准化为模型驱动循环。
2. 删除主路径 `max_tool_rounds` / `ToolRoundLimit` 产品语义。
3. 明确哪些边界可以停止 run，哪些只能记录诊断或触发维护动作。
4. 将 context compaction 建模为可见 timeline item，支持 SSE、replay、recovery 和多端等待态。
5. 明确 compaction 失败后的 retry、prompt rebuild、deterministic trim、fail-fast 顺序。
6. 明确 compact / memory / title generation 等 helper model call 的统一规则。
7. 明确主 Agent loop 不设置默认 step limit；subagent / fork / background run 是否设置限制由显式 runtime policy 决定。
8. 明确配置目标态，避免为主 Agent loop 新增 `maxTurns`、`agent_step_limit` 等同类硬限制。
9. 保持与 `refactor-033` 的 AgentHarness / AgentRuntime SPI 分层一致。
10. 明确 session compact summary 的 coverage 游标、raw recent 过滤和 fresh tail 回补策略。
11. 明确 active run working context 的预算检查、压缩/裁剪和 fail-fast 规则，避免删除固定轮次后同一 run 内上下文无界增长。

## 非目标

1. 不在本 spec 中重新设计 Service / Gateway / SDK 全部公共 API。
2. 不新增用户可见的 `resume` / `attach` 产品 API。
3. 不改变 `run_id == turn_id` 的身份不变量。
4. 不把 DB 持久化、timeline store、session store 移入 AgentHarness 或 AgentRuntime。
5. 不实现新的 CodexAgent / ClaudeAgent runtime。
6. 不把 context compaction / memory / title generation 设计成第二套 streaming agent loop。
7. 不为旧 `max_tool_rounds` 行为保留兼容分支。
8. 不在本 spec 中完整落地 tracing 设计；tracing 可后续独立演进。
9. 不在第一阶段完整统一所有 helper model call policy；第一阶段只要求 compaction helper 具备 timeout、retry、usage 和生命周期事件。
10. 不在第一阶段完成 subagent topology 的完整设计；本 spec 只要求主 Agent loop 不再继承固定工具轮次限制。

## 收敛后的阶段边界

第一阶段必须形成一个可运行闭环：

1. 删除 `max_tool_rounds` / `ToolRoundLimit` 及其配置、schema、SDK、UI、测试分支。
2. 新增 `ContextCompaction` timeline item / delta wire contract，并让阻塞 compaction 可被 SSE、replay、Web、TUI 看到。
3. 将 session history compact 改为基于 latest summary `covers_until_seq` 的 coverage 游标推进。
4. 将 MemoryStore prompt context 加载改为支持 compact cursor 过滤、Prompt view 最新窗口回补，以及 summary compaction source 连续窗口推进。
5. 在每次主路径 model request 前检查 `prepared.messages + state.messages` 的 working context 预算，超预算时执行 working-context compaction / trim / fail-fast。
6. 将 session compaction usage 和任何 working-context helper usage 聚合到 run-level usage。

以下内容可以后续独立演进，不能成为第一阶段的前置阻塞：

1. 完整 AgentHarness helper model call framework。
2. 完整 structured error mapping framework。
3. subagent topology 和 background runtime policy。
4. 可配置 runaway guard。
5. CodexAgent / ClaudeAgent / RemoteAgent runtime 接入。

## 术语

| 概念 | 定义 |
| --- | --- |
| `Run` | 一次用户请求触发的 Agent 执行。内部 `run_id == turn_id`。 |
| `Main agent loop` | Run 内部的主模型循环：model request -> model output -> tool execution -> tool result -> next model request。 |
| `Context compaction` | Run 内部为了恢复 prompt budget 执行的阻塞维护阶段。 |
| `Compact summary` | 对已完成 session turns 的连续区间摘要，记录 coverage 游标供后续 prompt view 过滤 raw history。 |
| `Raw recent tail` | PromptBuilder 中保留的少量原始最近消息，用于保持短期上下文新鲜度。它可以与最新 summary 有有限重叠。 |
| `Working context` | 当前 active run 内尚未持久化的模型消息、tool calls、tool results、nudges 和 retrieved memories。它存在于 Kernel loop state 中，不属于 completed session history。 |
| `Helper model call` | compact、memory summary、memory extraction、title generation、classifier、metadata generation 等非主路径模型调用。 |
| `Finalization model step` | 主 run 为了生成最终用户可见答案而执行的 finalization / salvage 调用，可在 streamed run 中继续 streaming。 |
| `Subagent` | 由主 Agent 或工具触发的子任务执行单元，可独立运行，但受父级拓扑、等待和资源策略约束。 |
| `Iteration` | 仅可作为内部诊断指标，表示一次 main loop 的 model step 或 tool round；不能作为产品停止条件。 |

## 主 Agent Loop 目标态

目标执行模型：

```text
start run
  -> build prompt/context
  -> if context compaction is required:
       emit ContextCompaction timeline item
       compact uncovered completed turns / rebuild / deterministic trim
       continue only when prompt is within budget
  -> stream main model request
       -> final response: complete run
       -> tool calls: execute tools, append tool results, continue
       -> waiting input: suspend run and wait for dynamic input
       -> output limit: stop with ModelOutputLimit
  -> before each next model request:
       validate prepared prompt + working context budget
       compact / trim working context if needed
       fail before provider request if still over budget
  -> repeat until model or boundary stops the run
```

目标态约束：

1. 主 loop 默认不设置 `maxTurns`。
2. 主 loop 默认不设置 `agent_step_limit`。
3. 主 loop 默认不设置 `max_tool_rounds`。
4. 工具调用次数、模型 step 次数、累计时间、累计 token 只作为 metrics / diagnostics 记录。
5. 模型返回 final response 时，run 以 `StopReason::EndTurn` 完成。
6. 模型请求外部 dynamic input 时，run 以 `StopReason::WaitingDynamicInput` 暂停。
7. 用户中断或系统取消时，由 AgentHarness / Service 映射为 public `StopReason::Cancelled` 或 cancelled error；Kernel 不需要因此新增独立取消状态。
8. 资源、权限、上下文、工具错误等边界命中时，run 按稳定 stop reason 或 structured run error 停止，不能临时扩展 public `StopReason` 承载错误分类。

主 loop 可以保留 runaway guard，但 guard 不能重新变成固定 turn limit。guard 应只识别低价值重复行为，例如连续只读调查无实质推进、同一工具连续失败且模型仍重复尝试、空响应后工具调用反复震荡。guard 的结果应是注入 nudge、触发 finalization 请求、请求用户输入或显式失败，而不是恢复 `ToolRoundLimit` 语义。

## 停止条件与错误生命周期

### Public StopReason

迁移后的 public `StopReason` 只承载 run 生命周期可稳定表达的完成状态：

1. `EndTurn`: 模型给出 final response。
2. `ModelOutputLimit`: provider 返回输出长度限制。
3. `WaitingDynamicInput`: 模型通过工具或运行时机制等待外部输入。
4. `Cancelled`: 用户 interrupt、系统 shutdown、active turn 被取消。
5. `TransferUnresolved`: 保持现有 transfer 语义，不在本 spec 中展开。

`ToolRoundLimit` 不再是主路径 stop reason。迁移时应删除：

1. Kernel `StopReason::ToolRoundLimit`。
2. 公共 `StopReason::ToolRoundLimit`。
3. generated schema / TS SDK 中的 `tool_round_limit`。
4. `max_tool_rounds` 配置校验、默认值、测试 fixture。
5. 依赖 `ToolRoundLimit` 的负向测试。

### Structured Run Error

`UsageLimited`、`ContextLimit`、`CompactFailed` 不应临时塞进 public `StopReason`。它们应作为 turn lifecycle 的结构化错误分类，通过 `timeline.turn.error`、active turn `Failed`、Gateway / SDK error DTO 反馈给客户端。

目标结构：

```text
TalosRunErrorInfo:
  ContextWindowExceeded
  SessionBudgetExceeded
  UsageLimitExceeded
  ContextCompactionFailed
  ProviderUnavailable
  PermissionDenied
  ToolFailure
  InternalError
```

`TalosTurnErrorEvent` 目标形态：

```text
TalosTurnErrorEvent {
  request_id
  turn_id
  error
  error_info?
  failed_at_ms
}
```

本轮可以先增加最小必要错误分类，不需要一次性实现完整 error mapping framework。但删除 `ToolRoundLimit` 后不能把错误语义退化为模糊字符串长期存在。

目标映射：

| 场景 | 对外生命周期 | 结构化错误 |
| --- | --- | --- |
| provider output length | `timeline.turn.completed` | `StopReason::ModelOutputLimit` |
| user interrupt | `timeline.turn.completed` 或 cancelled error | `StopReason::Cancelled` |
| dynamic input required | `timeline.turn.completed` 或 active waiting state | `StopReason::WaitingDynamicInput` |
| budget exhausted | `timeline.turn.error` + active turn failed | `UsageLimitExceeded` 或 `SessionBudgetExceeded` |
| context unrecoverable | `timeline.turn.error` + active turn failed | `ContextWindowExceeded` |
| compaction unrecoverable | `timeline.turn.error` + active turn failed | `ContextCompactionFailed` 或 `ContextWindowExceeded` |
| permission denied | `timeline.turn.error` 或 waiting approval state | `PermissionDenied` |
| repeated tool failure | `timeline.turn.error` 或 finalization | `ToolFailure` |
| repeated low-value loop | nudge / finalization / explicit error | `InternalError` 或 domain-specific guard error |

## Context Compaction 设计

### 语义

Context compaction 是 run 内部阻塞维护阶段：

```text
running -> compacting -> running
running -> compacting -> failed
```

规则：

1. 自动 context compaction 发生在同一个 `turn_id` / `run_id` 内。
2. 自动 context compaction 不创建新 run，不创建新 turn，不作为子 agent。
3. 自动 context compaction 执行期间不能发起下一次主路径 model request。
4. helper summary / persistence decision 使用非流式 model call，不输出 token delta。
5. compaction usage 必须计入 run usage，并可保留 breakdown 供诊断。
6. compaction lifecycle 必须通过 timeline 暴露给客户端，避免多端显示为“卡住”。
7. compaction raw prompt、raw completion、runtime-private payload 不应通过 Gateway / SDK 暴露。

manual `session.compact` 是用户显式管理操作，当前仍可作为阻塞 request / response API；它不改变当前 active run 的 `run_id`，也不应该伪装成主 run 内部自动 compaction。

### Session History Coverage 与 Prompt View

Session history compact 的目标不是物理删除历史，而是让 prompt view 不再无限重复加载已被 summary 覆盖的大段 raw history。完整历史仍保留在 memory / session store 中，用于审计、历史展示、恢复和后续重新生成 summary。

目标状态下，每次 compact summary 都代表一段已完成 turn 的连续 coverage：

```text
CompactSummary {
  summary_id
  session_id
  covers_from_seq
  covers_until_seq
  source_turn_count
  created_at
  content
}
```

当前实现已经有 `SummaryRecord.covers_until_seq`；第一阶段可以先复用该字段作为 compact 游标。如果需要审计和调试更清晰，应补充 `covers_from_seq`，避免只靠上一条 summary 反推起点。

compact source 选择规则：

```text
latest_compacted_seq = latest persisted compact summary covers_until_seq, or 0
latest_completed_seq = latest completed turn seq

compact_range = turns where:
  seq > latest_compacted_seq
  seq <= latest_completed_seq
```

规则：

1. compact 只处理已完成 turns，不处理当前 active run 内尚未持久化的 working context。
2. compact range 必须按 turn 边界选择，不能截断 tool call / tool result 配对。
3. 如果 `compact_range` 为空，不能生成新的 summary。
4. 如果 `compact_range` 很小且 prompt 仍在预算内，可以跳过 compact。
5. 新 summary 的 `covers_until_seq` 必须等于本次 compact range 的最后一个 completed turn seq。
6. 新 summary 的 `covers_from_seq` 如果存在，应等于 `latest_compacted_seq + 1`。
7. compact summary 之间不应重复覆盖同一段历史；prompt view 的 fresh tail 回补允许有限重叠，但 compact source 不允许重复压缩。

PromptBuilder 的 session history view 应分成两个区域：

```text
Compact Summary Region:
  latest K compact summaries

Raw Recent Tail:
  prefer turns where seq > latest_compacted_seq
  if preferred raw tail count < configured recent_messages_limit:
    backfill latest completed turns at or before latest_compacted_seq
    until the configured recent window is filled
```

这意味着 prompt 可以出现少量 intentional overlap：

```text
summary covers 1..150
raw tail backfills 141..150
```

这种重叠是为了保持最近上下文新鲜度，不代表 compact coverage 重复。Prompt diagnostics 应能标记：

1. latest compacted seq。
2. raw tail preferred count。
3. raw tail backfilled count。
4. summary records included count。
5. compact summary coverage range。

推荐 prompt 形态：

```text
Static Prefix
+ Compact Summary Region
+ Raw Recent Tail
+ Current User Input
+ Current Active Run Context
```

`Static Prefix` 不参与 compact，不参与 dynamic trim。`Current User Input` 不参与 compact，不参与 dynamic trim。`Current Active Run Context` 属于 Kernel working context，不能混入 session history compact；去掉固定 tool round limit 后，需要单独用 working-context compaction / trim 策略治理。

compact 后仍超预算时，才进入 deterministic trim。trim 仍只裁 dynamic suffix，不裁 static prefix 和 current user input。对于 `Compact Summary Region` 和 `Raw Recent Tail`，trim 优先裁更旧、更低价值、更可恢复的 dynamic section；如果 static prefix + current user input 已经超预算，应 fail fast，而不是继续压缩历史。

Session compaction source 不能复用 Prompt view 的“最新窗口”语义。生成下一条 compact summary 时，MemoryStore 必须使用 `OldestUncovered` 窗口：从 `latest_compacted_seq + 1` 开始选择最早的未覆盖 completed turns，并用已有 `source_recent_messages_limit` / `source_recent_tool_results_limit` / `source_recent_tool_exchanges_limit` 作为窗口上限。如果 compaction input budget 需要裁剪 history，应保留最早未覆盖 history，裁剪窗口末尾 history，保证 `SummaryRecord.covers_until_seq` 只推进连续 coverage。

### MemoryStore Prompt Context API

compact cursor 和 fresh tail 选择必须由 MemoryStore / session prompt context 层处理，不能把 seq 过滤逻辑散落到 PromptBuilder。PromptBuilder 只消费结构化 prompt context，并负责组装 section。

目标输入：

```text
SessionPromptContextConfig {
  compact_cursor: Option<CompactPromptCursor>,
  recent_window: SessionPromptRecentWindow,
  recent_messages_limit: usize,
  recent_tool_results_limit: usize,
  recent_tool_exchanges_limit: usize,
  ...
}

CompactPromptCursor {
  covers_until_seq
}

SessionPromptRecentWindow {
  Latest,
  OldestUncovered,
}
```

目标输出应保留足够 diagnostics：

```text
SessionPromptContext {
  summaries
  recent_messages
  recent_tool_results
  recent_tool_exchanges
  diagnostics {
    latest_compacted_seq
    raw_tail_preferred_count
    raw_tail_backfilled_count
    summary_records_included_count
  }
}
```

过滤规则：

1. `compact_cursor` 存在时，raw recent messages / tool results / tool exchanges 默认只取 `turn.seq > covers_until_seq`。
2. `SessionPromptRecentWindow::Latest` 用于正常 Prompt view；如果过滤后的 raw recent message 数量少于 `recent_messages_limit`，可以从 `turn.seq <= covers_until_seq` 的 completed turns 向前回补最近少量 messages。
3. `SessionPromptRecentWindow::OldestUncovered` 用于 summary compaction source；它只返回 cursor 后最早的未覆盖窗口，不做 covered tail 回补。
4. 回补必须保持 turn 边界和 tool call / tool result 配对，不允许只回补半个工具交换。
5. hot cache 只有在能证明 message 所属 `turn.seq` 时才能参与 cursor 过滤；否则 cursor 存在时应回退到 turn store 读取，避免把已 compact 的 raw history 重新注入 prompt。
6. `covers_until_seq` 只用于 prompt view 过滤，不删除底层 session artifacts。

### Active Run Working Context

session history compact 只处理已完成 turns；长任务真正去掉固定轮次后，还必须治理当前 active run 内的 working context。当前 Kernel 每轮请求都会把 `prepared.messages + state.messages` 发给 provider，因此 `state.messages` 里的 tool call / tool result 会随长任务持续增长。

目标规则：

1. Kernel 在每次主路径 model request 前都必须构建候选 request messages，并按当前 model context window / effective prompt budget 做 preflight check。
2. 如果候选 messages 在预算内，直接进入 provider request。
3. 如果候选 messages 超预算，触发 `ContextCompactionTrigger::WorkingContextExceeded`，但不写 `SummaryRecord`，也不推进 session history coverage。
4. working-context compaction 只压缩当前 run 内较早的 tool exchange / assistant working messages，不能压缩 static prefix、current user input、pending dynamic input、未闭合 tool call、最近必要 tool result。
5. 如果 helper summary 成功，Kernel 用一个 bounded `WorkingContextSummary` system message 替换被压缩的早期 working context，并保留最近 N 个完整 tool exchanges。
6. 如果 helper summary 失败，进入 deterministic trim：优先裁剪低价值旧 tool output、重复 retrieved memory、过长 read-only result、已被 summary 覆盖的旧工作片段。
7. 如果 compact / trim 后仍超预算，必须在 provider request 前 fail fast，并发送 `timeline.turn.error(ContextWindowExceeded)`。

推荐保留顺序：

```text
must keep:
  static prefix
  compact summary region
  raw recent tail
  current user input
  pending dynamic tool context
  unresolved tool call/result linkage
  latest N complete working tool exchanges

compact or trim first:
  older working tool exchanges
  repeated read-only outputs
  duplicate retrieved memories
  large low-value tool outputs
```

working-context compaction 与 session history compaction 使用同一个 `ContextCompaction` timeline item 类型，但 metrics 不同：

```text
session history compaction:
  covers_from_seq / covers_until_seq are present

working-context compaction:
  covers_from_seq / covers_until_seq are absent
  working_context_input_messages
  working_context_compacted_messages
  working_context_preserved_recent_exchanges
```

第一阶段可以先实现 deterministic working-context trim 和 fail-fast，再补 helper summary；但不能在删除 `max_tool_rounds` 后完全没有 preflight budget check。

### Timeline Item

不新增 `RunCompactionStarted` / `RunCompactionCompleted` 顶层事件。目标是复用现有 timeline item lifecycle，让 Web / TUI / SDK 通过统一事件模型渲染：

```text
TalosTimelineItemKind::ContextCompaction
```

目标内容结构：

```text
TalosTimelineContent::ContextCompaction {
  trigger: ContextCompactionTrigger,
  stage: ContextCompactionStage,
  outcome: Option<ContextCompactionOutcome>,
  metrics: Option<ContextCompactionMetrics>,
  message: Option<String>,
}
```

目标枚举：

```text
ContextCompactionTrigger:
  ThresholdExceeded
  HardThresholdExceeded
  BudgetExceeded
  HistorySectionsDropped
  WorkingContextExceeded

ContextCompactionStage:
  Preparing
  Summarizing
  RebuildingPrompt
  Trimming
  Completed
  Failed

ContextCompactionOutcome:
  Applied
  RejectedMissingSummary
  TrimmedFallbackApplied
  FailedContextStillTooLarge
  FailedProviderError
  FailedInternalError
```

目标 metrics：

```text
ContextCompactionMetrics {
  covers_from_seq?
  covers_until_seq?
  pre_compaction_total_tokens
  post_compaction_total_tokens
  compaction_input_tokens
  compaction_output_tokens
  trimmed_context_unit_count
  raw_tail_preferred_count?
  raw_tail_backfilled_count?
  working_context_input_messages?
  working_context_compacted_messages?
  working_context_preserved_recent_exchanges?
}
```

### Timeline Delta Contract

`ContextCompaction` 必须有明确的 wire delta，不应把 stage / metrics / outcome 塞进普通 text delta。

目标新增：

```text
TalosTimelineDelta::ContextCompaction {
  stage: Option<ContextCompactionStage>,
  outcome: Option<ContextCompactionOutcome>,
  metrics: Option<ContextCompactionMetrics>,
  message: Option<String>,
}
```

规则：

1. `timeline.item.started` 创建 `TalosTimelineContent::ContextCompaction` 初始内容。
2. `timeline.item.delta` 使用 `TalosTimelineDelta::ContextCompaction` 更新同一个 item 的 stage / metrics / message。
3. `timeline.item.completed` 必须携带最终 stage、outcome 和 metrics。
4. Web / TUI / SDK 不需要解析 raw helper prompt 或 raw helper completion，只消费 stage、outcome、metrics、message。
5. Rust serde、schema、generated TS、Web、TUI 的 enum tag 和字段命名必须一致。

### 事件序列

自动 compaction 目标事件序列：

```text
timeline.item.started   ContextCompaction(stage=Preparing)
timeline.item.delta     ContextCompaction(stage=Summarizing)
timeline.item.delta     ContextCompaction(stage=RebuildingPrompt)
timeline.item.delta     ContextCompaction(stage=Trimming)
timeline.item.completed status=Completed / Failed
timeline.turn.error     只有最终 run 失败时发送
```

这些事件必须进入 timeline store，因此天然支持：

1. SSE live update。
2. `timeline.recover`。
3. `subscribe_timeline` cursor replay。
4. Web / TUI 重连后恢复 running compaction item。

第一阶段可以不扩 `TimelineActiveTurnSnapshot`。如果 UI 需要无需 replay 就能快速展示阻塞阶段，后续可新增：

```text
TimelineActiveOperation::ContextCompaction {
  item_id
  stage
}
```

### 失败与降级链路

compaction 失败后不能直接打主模型请求，让 provider 报 context error。目标顺序如下：

```text
build prompt
  -> if compaction trigger hit:
       emit ContextCompaction started
       retry bounded compaction helper on recoverable errors
       rebuild prompt
       if within budget:
          emit ContextCompaction completed(Applied)
          continue main model loop
       deterministic trim
       rebuild prompt
       if within budget:
          emit ContextCompaction completed(TrimmedFallbackApplied)
          continue main model loop
       emit ContextCompaction failed(FailedContextStillTooLarge)
       emit timeline.turn.error(ContextWindowExceeded)
       fail turn before provider request
```

可恢复 retry 只适用于：

1. JSON parse / schema validation failure。
2. provider transient error。
3. network / stream transport transient error。

deterministic trim 规则：

1. 必须保留 system / developer / current user input。
2. 必须保留 pending dynamic tool context。
3. 必须保留当前 run 所需 tool call / tool result linkage。
4. 优先保留最近 N 轮、最近关键 tool result、当前任务相关摘要。
5. 优先丢弃旧 history、低价值 tool output、过长历史片段、可恢复历史详情。
6. 必须记录 trimmed unit count、unit kind 和 fallback outcome。

trim 后仍超预算时，必须 fail fast：

```text
timeline.item.completed ContextCompaction(status=Failed)
timeline.turn.error error_info=ContextWindowExceeded
active_turn.status=Failed
```

此时禁止再请求主模型。这样错误可控、可恢复、可被多端明确展示。

### 客户端渲染

多端统一渲染规则：

1. `ContextCompaction` running: 显示 `Compacting context...` 等待态。
2. `Summarizing`: 显示 `Summarizing previous context...`。
3. `RebuildingPrompt`: 显示 `Rebuilding context...`。
4. `Trimming`: 显示 `Trimming older context...`。
5. `Completed + Applied`: 可折叠展示，也可短暂展示后隐藏。
6. `Completed + TrimmedFallbackApplied`: 显示轻量 warning，提示旧上下文被裁剪。
7. `Failed` 且后续 `timeline.turn.error`: 展示最终失败原因。

## Helper Model Call 规则

非主路径 helper model call 包括：

1. session / history compaction summary。
2. memory persistence decision。
3. workspace memory summary。
4. memory extraction。
5. session title generation。
6. classifier / routing helper。
7. metadata generation。
8. 未来的小型 side query。

目标约束：

1. 默认非流式调用。
2. 默认不允许工具调用。
3. 必须有明确 timeout。
4. 必须有 input size cap 和 output size cap。
5. 必须计入 usage。
6. 只发送 coarse lifecycle event：started / completed / failed。
7. 不能复用 main loop 的 open-ended run 语义。
8. 失败不能静默吞掉；必须在正确层级转为 fallback、warning、turn error 或管理 API error。

注意：forced finalization / salvage finalization 不属于 helper model call。它们是主 run 的 finalization model step，在 streamed run 中可以继续输出 streaming final answer delta。

本 spec 第一阶段不要求一次性重构所有 helper model call。主线是删除主 Agent loop 的固定轮次停止语义、让阻塞 context compaction 对客户端可见、补齐 session coverage 和 working-context 预算治理。

## Subagent 设计

Subagent 不继承主 Agent loop 的 open-ended 默认语义。是否设置 step / runtime limit 必须由 subagent kind、runtime policy 或具体 batch/background 场景显式决定，而不是复用 `agent.kernel.max_tool_rounds`。

主 Agent loop 默认不设置固定 step limit；subagent、fork agent、background worker 可以设置显式限制。例如 Claude Code 的 fork subagent 有自己的 `maxTurns` 默认值，但主 query loop 不靠全局默认 max turns 终止。

建议控制项：

```text
max_agents
max_depth
max_concurrency
wait_timeout_ms
max_wait_timeout_ms
max_runtime_seconds?
```

语义约束：

1. `max_agents` 限制一次 run 内可创建的子 agent 总数。
2. `max_depth` 限制 agent nesting 深度。
3. `max_concurrency` 限制并发执行数量。
4. `wait_timeout_ms` 是父 agent 等待子任务结果的时间，不是子 agent 生命周期。
5. `max_wait_timeout_ms` 是父级等待上限，避免模型无限延长等待。
6. `max_runtime_seconds` 只用于 batch / background job 等非交互子任务，不作为主 agent 默认限制。
7. 子 agent usage、events、failure 必须能归集到父 run。

## 配置目标态

目标态删除：

```yaml
agent:
  kernel:
    max_tool_rounds: 8
```

不要为主 Agent loop 新增：

```yaml
agent:
  kernel:
    max_turns: 100
    agent_step_limit: 100
```

本阶段优先复用已有资源治理配置：

```yaml
agent:
  kernel:
    tool_parallelism: 1
    tool_result_max_bytes: 65536
    compaction:
      trigger_ratio_percent: 85
      hard_ratio_percent: 95
      recent_summaries_keep: 10
      compacted_recent_messages_keep: 10
      source_recent_messages_limit: 48
      source_recent_tool_results_limit: 16
      source_recent_tool_exchanges_limit: 12
      tool_output_keep_last: 2
      tool_exchange_keep_last: 2
```

配置原则：

1. 配置表达资源治理和安全边界，不表达主 loop 固定轮次。
2. 默认配置必须体现真实产品语义，测试不能通过修改默认语义来绕开设计。
3. 删除配置字段时同步更新 schema、fixture、dev yaml、prod yaml、README 和测试。
4. 不保留无效 alias 或兼容 fallback。
5. runaway guard、helper timeout / retry、working-context 专用 policy 可以后续独立配置化；本阶段不新增这些 YAML 概念。
6. `compacted_recent_messages_keep` 表达 compact 后 Prompt view 保留的最新 raw tail 数量；`source_recent_messages_limit` 表达 summary compaction source 的连续未覆盖窗口上限。不要新增第二套 compact 配置来重复表达同一语义。
7. 示例中的 `working_context` 只表达同一 active run 内的上下文治理，不等价于主 loop step limit。

## 资源边界 Owner

删除固定工具轮次后，资源边界必须显式归属，不能隐式依赖 provider context error。

1. KernelAgent 负责 model request 前的 context window / effective prompt budget preflight。
2. KernelAgent 负责 tool result size cap、working-context compaction / trim、context fail-fast。
3. AgentHarness 负责 cancellation handle、runtime active handle、run-level usage 聚合和 runtime error normalization。
4. Service 负责 active turn 状态、timeline store、`timeline.turn.error`、Gateway / SDK DTO 映射。
5. Provider / tool timeout 可以作为边界，但不应是唯一边界；Kernel 必须在明显超 context 时先 fail fast。

第一阶段不新增默认 `max_runtime_seconds`、`max_model_calls` 或 `max_tool_calls`。如果未来引入 usage/time budget，必须作为显式资源治理配置，并映射为 structured run error，而不是恢复固定轮次 stop reason。

## 分层职责

### Service

Service 继续负责产品状态和恢复：

1. 创建 session / turn。
2. 维护 active turn。
3. timeline store 和 live recovery。
4. crash recovery 后恢复客户端可见事件。
5. 将 AgentEvent 映射为 Gateway / SDK 可见 DTO。
6. 维护 `TimelineActiveTurnStatus::Failed`、`timeline.turn.error` 和 replay / recovery 语义。

Service 不负责主 agent loop 的模型循环策略，不执行 compaction prompt rebuild / trim 决策。

### Coordinator

Coordinator 继续负责 Agent 选择和调度：

1. 选择 AgentHarness。
2. 组装 `AgentRunParams`。
3. 调用 `run` / `run_streamed`。
4. 未来管理多 Agent 协作和 subagent topology。

Coordinator 不负责执行 concrete backend loop，也不处理 compaction 算法。

### AgentHarness

AgentHarness 承载公共 run lifecycle：

1. active runtime handle。
2. cancellation。
3. continuation / dynamic input bridge。
4. capability validation。
5. event normalization。
6. usage aggregation。
7. helper model call policy。
8. runtime error mapping。

AgentHarness 不持久化 DB record，不暴露 Gateway DTO。对于 context compaction，它应保证 Runtime 产生的 lifecycle event 能通过统一 `AgentEvent` 进入 Service timeline。

### AgentRuntime SPI

AgentRuntime 是 concrete backend 接入 AgentHarness 的最小 SPI：

```text
run(AgentRunParams) -> AgentRunResult
run_streamed(AgentRunParams) -> AgentEventStream
resume(AgentResumeParams) -> AgentRunResult
resume_streamed(AgentResumeParams) -> AgentEventStream
capabilities() -> AgentCapabilities
```

Runtime 可以内部使用 KernelAgent、CodexAgent、ClaudeAgent 或 RemoteAgent，但跨 Harness 边界只使用共享 `AgentRunParams`、`AgentRunResult`、`AgentEvent`、`TokenUsage`、`RunItem` 等标准对象。

`interrupt(AgentInterruptParams) -> AgentInterruptResult` 属于 AgentHarness / Coordinator 控制 API。当前 concrete Runtime SPI 可以通过 Harness-owned abort handle 实现中断；是否把 interrupt 下沉为 Runtime SPI 方法是后续扩展点，不是本 spec 的必要前置条件。

### KernelAgent

KernelAgent 负责 Talos 内置 Kernel 的具体执行：

1. 判断 context compaction trigger。
2. 执行 compaction helper call。
3. 执行 prompt rebuild。
4. 执行 deterministic trim fallback。
5. 在进入主模型请求前确认 prompt budget。
6. 发出 Kernel 内部 compaction lifecycle event。

KernelAgent 不直接依赖 Gateway / SDK DTO，不直接写 DB timeline store。

## Event 和 Usage

主路径 event：

1. 可以流式输出 assistant text delta。
2. 可以流式输出 reasoning / thinking delta。
3. 可以流式输出 tool call lifecycle。
4. 可以输出 run metrics 和 final completed event。

context compaction event：

1. 使用 timeline item lifecycle 表达 waiting / completed / failed。
2. 不输出 token delta。
3. 不输出 raw prompt / raw completion。
4. usage 汇总进 run usage，同时可保留 compaction usage breakdown 供诊断。
5. failed item 不一定意味着 run failed；只有 prompt rebuild + deterministic trim 后仍不可继续时，才发送 `timeline.turn.error`。

usage 规则：

1. main model usage、tool-mediated model usage、compaction usage、memory usage 都必须进入 run-level accounting。
2. usage breakdown 可以增加内部字段，但对外 DTO 不应暴露 runtime-private provider shape。
3. usage 超限时统一映射为 structured run error；本 spec 不新增 public `UsageLimited` stop reason。

聚合落点：

1. Kernel preparation 阶段产生的 session history compaction usage 必须进入 `PreparedGeneralTurn` 或等价 prepared artifact。
2. Kernel loop 内产生的 working-context compaction usage 必须进入 `GeneralTurnLoopState`。
3. `ExecutedTurnArtifacts.usage` 必须包含 prepared compaction usage、working-context compaction usage、main model usage 和 tool-mediated model usage。
4. `TurnExecutionMetrics.usage` 与最终 `GeneralTurnResult.usage` 必须使用同一份聚合结果，避免 diagnostics 和 API 看到不同 usage。
5. AgentHarness 只做跨 Runtime 的 normalization / aggregation，不重新解释 provider-private usage breakdown。

## 迁移要求

1. 删除 `max_tool_rounds` 配置字段、校验、默认值和 fixture。
2. 删除 Kernel / public `StopReason::ToolRoundLimit`。
3. 删除 generated schema / TS SDK / Web / TUI 中的 `tool_round_limit` 分支。
4. 删除依赖 tool round limit 的负向测试。
5. 删除当前 `ToolRoundBudget` forced finalization path。已有 repeated read-only / empty response guard 可以保留，但必须映射为 `EndTurn` finalization 或明确 structured run error，不能再映射为 `ToolRoundLimit`。
6. 新增 `ContextCompaction` timeline item kind / content / delta 支持。
7. Kernel 自动 compaction 前后发 started / delta / completed / failed lifecycle。
8. Runtime 将 Kernel compaction lifecycle 映射为标准 `AgentEvent`。
9. Web / TUI 对 `ContextCompaction` item 显示等待态。
10. compaction 失败后执行 deterministic trim；trim 后仍超预算时 fail turn before provider request。
11. 将 compact source 从无游标最近窗口调整为基于 latest summary `covers_until_seq` 的未压缩 completed turn 区间。
12. 如果当前 `SummaryRecord` 只有 `covers_until_seq`，评估补充 `covers_from_seq`；如果第一阶段不补字段，必须在代码注释中说明起点由上一条 summary 推导。
13. MemoryStore / prompt context 加载 recent messages 时支持 latest compact cursor 过滤，默认只返回 `seq > covers_until_seq` 的 raw history。
14. PromptBuilder raw recent tail 支持按 `recent_messages_limit` 回补：过滤后不足时，可从已覆盖区间向前补最近少量 completed turns。
15. Prompt diagnostics 记录 latest compacted seq、raw tail preferred count、raw tail backfilled count 和 included summary count。
16. 主 loop 每次 provider request 前增加 working context budget preflight，超预算时执行 working-context compaction / trim / fail-fast。
17. 新增 `TalosTimelineDelta::ContextCompaction` 或等价强类型 delta，并同步 schema / generated TS / Web / TUI。
18. compaction helper usage 聚合进 run-level usage，保证 metrics、result 和对外 DTO 一致。
19. 保留并强化已有 compact / prompt trim / context hard limit 测试。
20. 为长任务连续工具调用添加集成测试，验证不会在固定工具轮次后停止。
21. 为 compaction blocking phase 添加测试，验证 compaction 完成前不会发起下一次主路径 model request。
22. 为 context compaction timeline 添加 SSE replay / recovery 测试。
23. 为 summary coverage 添加测试，验证连续两次 compact 不重复压缩同一段 completed turns。
24. 为 PromptBuilder fresh tail 添加测试，验证 summary 覆盖后 raw recent 默认过滤已覆盖历史，但不足配置窗口时会有限回补。
25. 为 working-context preflight 添加测试，验证同一 run 内 tool results 增长超预算时不会继续请求 provider。
26. 更新 schema 生成产物和协议文档。

## 验证策略

非 E2E 验证：

1. `max_tool_rounds` 不再出现在配置、schema、测试 fixture 或 runtime control path。
2. `ToolRoundLimit` 不再出现在 Rust enum、TS type、schema 或 UI branch。
3. 主 loop 在模型持续发出有效 tool calls 时不会因为固定轮次停止。
4. repeated low-value guard 只在明确重复场景触发。
5. context compaction 是 blocking phase，且完成前不会发起下一次主路径 model request。
6. context compaction running / completed / failed 都能进入 timeline。
7. compaction helper model call 不输出 streaming token delta。
8. compaction usage 被计入 run usage。
9. compaction 失败后会执行 deterministic trim。
10. trim 后仍超预算时不会调用 provider，而是发送 `timeline.turn.error`。
11. SSE replay / `timeline.recover` 能恢复 running compaction item。
12. subagent 等待超时不杀死子 agent，除非命中显式 runtime policy。
13. `round`、`model_rounds`、`iteration` 等诊断指标可以继续存在，但不能参与主 loop 默认停止条件。
14. 第一次 compact 生成 summary 后，下一次 prompt context 不再加载已覆盖的大段 raw history。
15. 连续两次 compact 的 source range 严格递增，不重复覆盖上一段 completed turns。
16. raw recent 过滤后不足 `recent_messages_limit` 时，会有限回补最近 completed turns，且 diagnostics 能区分 preferred 和 backfilled。
17. `Static Prefix` 和 `Current User Input` 不参与 compact，也不会被 dynamic trim 删除。
18. hot cache 在 compact cursor 存在时不会绕过 `covers_until_seq` 过滤。
19. `ContextCompaction` timeline delta 能更新 stage、outcome、metrics，SSE replay 后状态一致。
20. compaction helper usage 会出现在 final run usage 和 turn metrics 中。
21. active run working context 超预算时会触发 preflight compaction / trim；trim 后仍超预算时不会请求 provider。
22. working-context compaction 不写 `SummaryRecord`，也不推进 session history `covers_until_seq`。

E2E 验证：

1. 完整 E2E 只跑一次。
2. 失败后只重跑失败 group / case。
3. E2E 通过后启动 Web / TUI 做核心 smoke。
4. 验证长任务工具调用、streaming timeline、interrupt、context compaction waiting UI、dynamic input、history recovery 不回归。

## 风险和后续

1. 删除 `ToolRoundLimit` 是 breaking change，必须同步 schema、SDK、UI 和 tests。
2. 没有固定轮次后，provider/tool 成本控制必须依赖 budget、timeout、context、permission 和 structured error，不能缺位。
3. runaway guard 的 cause 命名必须准确，否则会把旧 `max_tool_rounds` 语义换壳保留。
4. context compaction 已有实现偏 Kernel 内部，第一阶段应保持 compaction 算法在 Kernel / MemoryStore 边界内，不要上移到 Service。
5. `TalosRunErrorInfo` 需要和现有 `TalosError` / `AgentErrorKind` 做最小映射，避免新旧错误体系并存。
6. subagent topology 仍需独立 spec 或后续阶段细化，避免一次改造过大。
7. tracing 与 error mapping 可以独立演进；本 spec 不把 tracing 作为阻塞项。
