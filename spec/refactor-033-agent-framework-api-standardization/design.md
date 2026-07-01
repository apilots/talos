# Agent Framework API 标准化设计草案

Status: draft
Owner: agent
Created: 2026-07-01
Updated: 2026-07-01
Related: `refactor-032-agent-harness-api`, `refactor-031-api-concept-alignment`, `refactor-020-application-timeline-recovery-stream`

## 背景

`refactor-032-agent-harness-api` 已经把当前代码收敛到一条更清晰的基础链路：

```text
TalosService
  -> Coordinator
      -> AgentHarness
          -> AgentRuntime SPI
              -> KernelAgent
```

但是 032 只是首版落地：它完成了部分命名收敛、`AgentEvent` / `RunItem` / `TokenUsage` 等核心对象统一，以及 `AgentHarnessImpl -> KernelAgent implements AgentRuntime` 的基础拆分。下一阶段的核心目标不是单点补 `crash recovery`，而是继续把 Agent 领域做成一套可演进的小框架：

1. 分层职责稳定。
2. 每层 API 命名和数据结构标准化。
3. 同一概念只保留一套核心结构。
4. Harness 提供公共实现，Runtime 只接入具体 backend。
5. Coordinator 可以管理、选择、调度多个 Agent。
6. 未来 CodexAgent / ClaudeAgent / RemoteAgent 可以按同一套 SPI 接入，而不是每个 backend 复制一套 Harness 逻辑。

`crash recovery`、`resume`、`runtime_ref`、long-running task control 是这个框架在 run lifecycle 维度需要补齐的能力之一，不是本 spec 的主线。

## 目标

1. 明确 `Service -> Coordinator -> AgentHarness -> AgentRuntime -> concrete Agent` 的 API 边界。
2. 统一内部 Agent API 使用 `*Params` / `*Result`，对外产品 API 使用 `*Request` / `*Response`。
3. 继续合并和重命名同义核心结构，避免 `Runtime*`、`Harness*`、`Kernel*`、`Codex*`、`Claude*` 并行 DTO。
4. 抽取各层小框架：definition loading、factory、registry、router、capability validation、run lifecycle、event normalization、input bridge、error mapping、runtime SPI。
5. 让 AgentHarness 成为公共实现承载层，不只是 trait；concrete runtime 只实现 backend execution。
6. 为 Codex / Claude / remote runtime 接入保留清晰路径。
7. 把 live recovery、backend continuation recovery、dynamic input resume、interrupt 纳入统一 run lifecycle 设计，但不让 recovery 抢主线。

## 非目标

1. 不在本 spec 中直接实现 CodexAgent / ClaudeAgent / RemoteAgent。
2. 不把 Gateway / SDK 产品 API 全面重做。
3. 不把 timeline store、session store、active turn store 移入 AgentHarness。
4. 不把 DB `Record` 暴露到 Harness / Runtime / Gateway / SDK。
5. 不把 concrete runtime 的 thread/session/conversation id 作为公共 API。
6. 不为了未来能力提前暴露不可达半成品 API。

## 已确认决策

1. Agent 内部边界的 `turn_id` 目标态改为必填。Gateway / SDK 不要求客户端传入，Service 在创建 turn 或从 active turn / pending action 恢复后传入真实 `turn_id`。
2. 内部 `run_id` 和 `turn_id` 必须表示同一个值：`run_id == turn_id`。实现、日志、错误和 continuation 映射里如果需要表达 Agent run identity，只能映射到真实 `turn_id`，不能生成第二套 run id；历史拼写 `trun_id` 只能按同一个 `turn_id` 解读并纠正，不能成为合法字段或 alias。
3. 本轮引入内部 `resume` / `resume_streamed` 标准 API，只承载 dynamic input continuation；不暴露 Gateway / SDK resume API，不实现 `attach`。
4. 本轮不在 Gateway / SDK `StartTurnRequest` 暴露 `agent_id` / `agent_name`。`AgentRouter` 先落地内部 selection params / result、configured default 和 routing policy，外部显式选择留到多 Agent 产品形态明确后再开放。
5. `AgentRunOptions` 本轮只纳入当前真实可达的 `model`。`continuation_policy` 不进入本轮 options；dynamic input continuation 通过 `AgentResumeParams` / `AgentInput` 表达。
6. 落地真实 Agent domain error 类型：`AgentError` / `AgentErrorKind`。Agent / Harness / Runtime / Coordinator 内部使用 Agent error，Service 边界转换回 `TalosError`，Gateway / SDK 不直接感知内部 Agent error 类型。
7. `SubmitDynamicToolResultRequest` 不新增 `turn_id`。外部保持 session-level API，Service 负责从 pending action / active turn 恢复原始 `turn_id` 后传给 AgentHarness。
8. `attach` 本轮不暴露、不实现。

### 本轮 7 个硬性约束

1. `AgentRunOptions` 本轮只包含 `current_model?`；`continuation_policy` 不进入 options，dynamic input continuation 由 `AgentResumeParams` / `AgentInput` 表达。
2. `AgentError` / `AgentErrorKind` 是 Agent domain 内部错误结构，不导出到 generated TS schema，不作为 HTTP / JSON-RPC response body。
3. `resume_streamed` 只表示同一 `turn_id` 下的 continuation stream，不是第二次 `run_streamed`，不是客户端 attach，也不是 Service timeline recovery。
4. `AgentRunParams` 不再携带完整 `TalosSession`；run 边界使用 `AgentRunContext`、`AgentRunConfig`、`AgentRunSessionState`、`AgentRunOptions` 和 input。
5. 内部 run identity 必须写死为 `run_id == turn_id`。如果历史文档、日志或代码出现 `trun_id` 拼写，它只能映射回同一个 `turn_id` 并被纠正，不能引入第三套 id、兼容 alias 或独立存储字段。
6. Unsupported option / capability fail-fast 统一放到 capability validation framework；`AgentRunContext` / `AgentRunOptions` 成型阶段不顺手实现分散校验。
7. 最终验收必须先通过相关非 E2E case，再完整 E2E 只跑一次；失败后只跑失败 group / case，最后执行 Web / TUI 核心 smoke。具体命令放在根目录 `IMPLEMENTATION_PLAN.md`。

### 7 个硬性约束的设计归属

1. `AgentRunOptions.current_model?` 归属 `AgentRunOptions`，只表达跨 runtime 可标准化的执行选项；continuation 状态归属 run lifecycle。
2. `AgentError` / `AgentErrorKind` 归属 Agent domain error mapping；Service 负责把它映射回产品错误，schema / SDK 不直接暴露。
3. `resume_streamed` 归属 run lifecycle continuation；它不启动新 run，不接管 attach，不替代 Service timeline recovery。
4. `AgentRunContext` 归属 Harness / Runtime prepared execution context；完整产品 session 只能留在 Service / Coordinator adapter 或 session lifecycle API。
5. run identity 归属 Talos turn：内部 `run_id` 必须等于 `turn_id`；历史 `trun_id` 拼写只能按同一个 `turn_id` 解读、映射和迁移，不能成为新字段、alias 或 runtime-private id 的替代名。
6. unsupported option / capability fail-fast 归属 capability validation framework，不分散在参数构造、runtime adapter 或测试分支里。
7. 最终验收归属 PLAN；spec 只定义质量门禁原则，具体命令和顺序由 `IMPLEMENTATION_PLAN.md` 固化。

### Run Identity Invariant

Agent domain 只有一个稳定 run identity：真实 Talos `turn_id`。

1. 内部代码、日志、错误、event、diagnostic 或 continuation 需要使用 `run_id` 时，`run_id` 必须直接等于同一个 `turn_id`。
2. `run_id` 不能作为独立字段持久化，不能由 runtime 自行生成，也不能被 backend thread id / session id 替代。
3. 历史拼写 `trun_id` 不是合法字段、别名或兼容入口；如果在文档、日志或代码中出现，迁移期只能按同一个 `turn_id` 解读、映射并纠正，不能引入第三套 id。
4. Codex / Claude / Kernel 等 backend private thread / session / conversation id 只能留在 runtime-private diagnostic 或 continuation payload 内部，跨 Harness / Coordinator 边界时必须映射回 Talos `turn_id`。

Identity mapping 固化如下：

| 表达 | 合法边界语义 |
| --- | --- |
| `turn_id` | Agent domain 的唯一 run identity。 |
| `run_id` | 只允许作为语义标签出现，值必须等于 `turn_id`，不能独立生成或持久化。 |
| `trun_id` | 历史拼写错误；审查和迁移时必须映射回同一个 `turn_id` 并删除该拼写。 |
| backend thread / session / conversation id | 只能留在 concrete runtime private diagnostic / continuation payload 内，跨层时必须映射回 `turn_id`。 |

### 7 个硬性约束验收矩阵

| 约束 | 目标代码形态 | 必查边界 |
| --- | --- | --- |
| `AgentRunOptions` 只含 `current_model?` | `continuation_policy` 不出现在本轮 Agent domain code；continuation 通过 `AgentResumeParams` / `AgentInput` 表达。 | `AgentRunOptions`、`AgentResumeParams`、`AgentInput`、Runtime SPI。 |
| `AgentError` 不外露 | `AgentError` / `AgentErrorKind` 只存在于 Agent / Coordinator 内部错误路径；Service 映射为 `TalosError`。 | Gateway、app protocol schema、Rust SDK、TS SDK、generated TS。 |
| `resume_streamed` 语义收窄 | `resume_streamed` 只能继续同一 `turn_id` 的 continuation stream，不能创建新 run，不能作为 attach。 | Harness API、Runtime SPI、run lifecycle tests。 |
| `AgentRunContext` 边界迁移 | `AgentRunParams` 不携带完整 `TalosSession`，Runtime 从 prepared context/config/state 消费执行输入。 | `AgentRunParams`、Coordinator adapter、Kernel request adapter。 |
| `run_id == turn_id` | Agent domain 内部没有独立 `run_id` 字段；历史 `trun_id` 必须映射并迁移到同一个 `turn_id`，不能保留 alias。 | events、errors、diagnostics、continuation refs、logs、tests。 |
| capability fail-fast 集中 | unsupported option / input / interrupt / resume 只通过 capability validation framework 判定。 | `AgentCapabilityValidator`、Harness validation helpers、Coordinator boundary。 |
| 最终验证顺序 | 非 E2E case 先全通过，完整 E2E 只跑一次，失败后只跑失败 group / case，再做 Web / TUI smoke。 | `IMPLEMENTATION_PLAN.md` 阶段 9。 |

## 分层模型

目标分层：

```text
Gateway / SDK / UI
  -> TalosService
      -> Coordinator
          -> AgentRegistry
          -> AgentRouter
          -> AgentHarness
              -> AgentRuntime SPI
                  -> KernelAgent
                  -> CodexAgent
                  -> ClaudeAgent
                  -> RemoteAgent
```

### TalosService

Service 负责产品语义和持久化融合：

1. `Session`、`Turn`、active turn。
2. timeline store 和 live recovery。
3. idempotency。
4. project / workspace / directory resolver。
5. DB / file store mapping。
6. Gateway / SDK DTO 组装。
7. 调用 Coordinator 执行或控制 Agent run。

Service 不负责：

1. concrete runtime 选择之外的 backend 实现细节。
2. Codex / Claude / Kernel 私有 event 解析。
3. runtime private continuation payload 解码。

### Coordinator

Coordinator 负责 Agent 管理和编排：

1. 加载和注册 Agent。
2. 维护 `AgentRegistry`。
3. 通过 `AgentRouter` 做 agent selection。
4. 执行 `AgentDispatchParams -> AgentRunParams` 的边界转换。
5. 调用 `AgentHarness` 的 run / control API。
6. 未来承载多 Agent 调度、并行协作、handoff 策略。

Coordinator 不负责：

1. 写 DB record。
2. 直接依赖 `KernelAgent` / `CodexAgent` / `ClaudeAgent`。
3. 解释 runtime private thread/session/event。

### AgentHarness

AgentHarness 是 Coordinator 面向 Agent 能力的标准 facade，也是公共实现层。

它负责：

1. run lifecycle。
2. active runtime handle 和 cancellation。
3. pending input / dynamic tool result / approval bridge。
4. capability validation。
5. event normalization / enrichment / ordering。
6. usage / run diagnostics 汇总。
7. runtime error mapping。
8. tool registry、skill roots、plugin roots、MCP/tool policy 等公共基础设施。

它不负责：

1. Service 持久化。
2. Gateway / SDK DTO。
3. runtime private SDK object。
4. concrete backend 的核心推理执行逻辑。

### AgentRuntime SPI

AgentRuntime 是 Harness 内部接入 concrete backend 的最小 SPI。

它负责：

1. 对接 backend client / process / SDK。
2. 执行 `run` / `run_streamed`。
3. 把 backend private event 转成标准 `AgentEvent`。
4. 声明 runtime capabilities。

它不负责：

1. Coordinator 调度。
2. Service active turn store。
3. Gateway / SDK public DTO。
4. 每个 backend 复制 Harness 的 lifecycle / event / input / error 逻辑。

## 分层 API 标准化

### 命名规则

1. Service / Gateway / SDK 对外产品 API 使用 `*Request` / `*Response`。
2. App protocol JSON-RPC params 继续遵循协议层规则：request payload 用 `*Params`，response 用 `*Response`。
3. Coordinator / AgentHarness / AgentRuntime 内部执行 API 使用 `*Params` / `*Result`。
4. Store / repository 使用 `*Record` / `*Store`。
5. 同一语义不能同时存在 `AgentRunRequest` 和 `AgentRunParams`。
6. 不通过 type alias 长期保留双名；目标态必须重命名已有对象并删除旧公共名。

### 分层 API 列表

Service 产品 API 继续保持产品语义：

```text
start_turn(StartTurnRequest) -> StartTurnResponse
interrupt_turn(InterruptTurnRequest) -> InterruptTurnResponse
submit_dynamic_tool_result(SubmitDynamicToolResultRequest) -> SubmitDynamicToolResultResponse
get_active_turn(SessionActiveTurnRequest) -> TimelineActiveTurnSnapshot
recover_timeline(TimelineRecoveryRequest) -> TimelineRecoveryResponse
subscribe_timeline(SubscribeTimelineRequest) -> SubscribeTimelineResponse
timeline_subscription_source(SubscribeTimelineRequest) -> TimelineSubscriptionSource
```

Coordinator API：

```text
register_agent(agent_id, AgentHarness)
resolve_agent(AgentSelectionParams) -> AgentRegistration
handle(AgentDispatchParams) -> AgentDispatchResult
handle_stream(AgentDispatchParams) -> AgentEventStream
submit_input(AgentDispatchInputParams) -> ()
take_generated_session_title(session_id, turn_id) -> Option<String>
cancel_active_turn(CancelActiveTurnParams) -> ()
```

AgentHarness API：

```text
descriptor() -> AgentDescriptor
run(AgentRunParams) -> AgentRunResult
run_streamed(AgentRunParams) -> AgentEventStream
# Continue an existing Talos agent run after validated continuation input is available.
resume(AgentResumeParams) -> AgentRunResult
# Continue an existing Talos agent run and stream AgentEvent values; this is not attach or Service timeline recovery.
resume_streamed(AgentResumeParams) -> AgentEventStream
submit_input(AgentInputParams) -> AgentInputResult
interrupt(AgentInterruptParams) -> AgentInterruptResult
```

AgentRuntime SPI：

```text
kind() -> AgentKind
capabilities() -> AgentCapabilities
run(AgentRunParams) -> AgentRunResult
run_streamed(AgentRunParams) -> AgentRuntimeStream
# Continue an existing backend execution after Harness validates continuation input.
resume(AgentResumeParams) -> AgentRunResult
# Continue an existing backend execution and return a standardized AgentRuntimeStream; this is not attach.
resume_streamed(AgentResumeParams) -> AgentRuntimeStream
```

未来可选 SPI：

```text
# Attach to an already-running backend task and stream standardized Agent events without starting a new run.
attach(AgentAttachParams) -> AgentRuntimeStream
```

约束：

1. `resume` 是 run lifecycle 的标准 API；本迭代可以只完成 dynamic input resume 的语义对齐，不要求完整 backend continuation。
2. `attach` 是后续可选能力，只在 long-running backend task 可被重新接回时落地。
3. 即使新增 resume / attach，也必须复用共享结构，不能定义 `RuntimeResumeResult`。
4. Service 的 live recovery API 不下沉到 Harness；Harness 的 backend continuation 能力不替代 Service timeline recovery。
5. `resume_streamed` 是同一 `turn_id` 下的 continuation stream，不是第二次 `run_streamed`，也不是客户端 attach。

## 核心结构统一

### 已落地统一对象

032 已统一：

1. `AgentEvent`：Agent 执行事件 canonical object。
2. `TimelineEventEnvelope`：Service / Gateway recoverable timeline envelope。
3. `RunItem`：一次 Agent run / turn 的结构化 transcript item。
4. `TokenUsage`：跨 run、session、history、metrics 复用的 token 计量对象。
5. `AgentRunParams` / `AgentRunResult`：Harness / Runtime 共享执行结构。
6. `AgentInputParams` / `AgentInputResult`：Harness input bridge，携带同一个 `turn_id` run identity。
7. `AgentInterruptParams` / `AgentInterruptResult`：Harness interrupt bridge。
8. `AgentDefinition` / `AgentDescriptor` / `AgentKind` / `AgentCapabilities` / `AgentStatus`：Agent definition 和 registry domain。

### 继续收敛对象

#### AgentRunParams

当前形态：

```text
AgentRunParams
  context
  config
  session_state
  resume_context?
  options?
  input
```

目标评审项：

1. `AgentRunContext.turn_id` 目标态改为必填。
2. `AgentRunContext.turn_id` 是 Agent 内部 run identity 的唯一字段；文档或实现中需要表达 `run_id` 时，必须映射为同一个 `turn_id`，满足 `run_id == turn_id`。
3. 如果历史文档、日志或代码出现 `trun_id` 拼写，迁移期只能按同一个 `turn_id` 解读并纠正；`trun_id` 不是合法字段或别名。
4. `AgentRunParams` 不再携带完整 `TalosSession`；Coordinator adapter 从产品 session 准备 `AgentRunContext`、`AgentRunConfig` 和 `AgentRunSessionState`。
5. `AgentRunParams::new` 只能接收已经准备好的 Agent domain parts，不能继续作为 `TalosSession` 拆分 adapter；session 拆分逻辑只能存在于 Coordinator adapter、测试 fixture 或示例 helper。
6. `resume_context` 只承载 dynamic input continuation；普通 `run` / `run_streamed` 携带它必须 fail-fast 并要求调用 `resume` / `resume_streamed`。

#### AgentRunContext

`AgentRunContext` 解决的问题是把 Agent 执行所需上下文从完整产品会话对象中收敛出来，避免 Harness / Runtime 长期依赖完整 `TalosSession`。它不是 `TalosContext` 的改名，也不是 DB record 或 Gateway DTO。

本轮最小形态：

```text
AgentRunContext
  session_id
  turn_id
  message_id
  owner_user_id
  user_id?
  workspace_id?
  project_root?
  environment?
```

后续完整目标：

```text
AgentRunContext
  session_id
  turn_id
  message_id
  user_id?
  project_id?
  workspace_id?
  resolved_workspace_dir?
  working_directory?
  additional_directories?
  memory_scope?
  tool_policy?
  skill_context?
  session_snapshot?
```

约束：

1. 它是 Harness / Runtime 的 prepared domain context，不是 DB record。
2. Service / Coordinator 负责解析 workspace、project、权限和 session binding。
3. Runtime 只能消费 resolved context，不能自行查询产品 DB。
4. 不能和 `TalosContext` 字段相同却另起不同名字；抽取时必须说明替代关系。
5. `AgentRunParams` 不能携带完整 `TalosSession`；仍需要完整 session 的 API 只能属于 session lifecycle、Service / Coordinator adapter，或明确标记的内部转换点。
6. `AgentRunContext.turn_id` 同时是 run identity；任何 `run_id` 概念必须直接等于该 `turn_id`，不能作为独立字段持久化或跨层传递。

#### AgentRunConfig

`AgentRunConfig` 是 Agent run 的 prepared runtime configuration，不是完整产品配置对象。

当前形态：

```text
AgentRunConfig
  agent
  llm
```

约束：

1. 它只携带 Runtime 当前执行需要的配置域。
2. 不加入 Gateway / SDK DTO、DB Record、session title、active turn 状态或 timeline 状态。
3. concrete runtime 如需私有配置，应通过 `AgentDefinition.runtime_config` 或自身 SPI 内部配置解析，不扩展成 `RuntimeRunParams`。

#### AgentRunSessionState

`AgentRunSessionState` 是 Harness 输出行为需要的轻量产品 session snapshot，目前用于首轮 title generation 判断。

当前形态：

```text
AgentRunSessionState
  title
  message_count
```

约束：

1. 它不是 DB record，也不是完整 `TalosSession`。
2. 新增字段前必须证明 Harness 输出行为真实需要该字段。
3. 不能承载 Service persistence、timeline recovery 或 access-control 逻辑。

#### AgentRunOptions

本轮最小目标：

```text
AgentRunOptions
  current_model?
```

后续候选：

```text
AgentRunOptions
  current_model?
  approval_policy?
  sandbox_policy?
  max_turns?
  max_budget?
  output_schema?
  stream_detail?
  reasoning_effort?
  web_search?
  continuation_policy?
```

约束：

1. 只放跨 runtime 可以标准化表达的选项。
2. `AgentRunConfig.llm` 是执行配置 source of truth；`AgentRunOptions.current_model` 只是从配置派生的 selected model snapshot，用于跨 runtime 检查、诊断和未来 capability 评估，不能覆盖 runtime config。
3. runtime 私有配置继续放在 `AgentDefinition.runtime_config`。
4. 不支持的 option 必须通过 capability validation 显式失败。
5. 本轮不纳入 `approval_policy`、`sandbox_policy`、`output_schema`、`web_search`，避免暴露不可达半成品 API。
6. 本轮不纳入 `continuation_policy`；dynamic input continuation 由 `AgentResumeParams` / `AgentInput` 表达，避免 options 承载 lifecycle 状态。

#### AgentInput

Service -> Coordinator 输入桥接参数：

```text
AgentDispatchInputParams
  session
  turn_id
  input
```

Coordinator -> Harness 输入桥接参数：

```text
AgentInputParams
  session
  turn_id
  input
```

当前只包含：

```text
DynamicToolResult
```

候选扩展：

```text
AgentInput
  DynamicToolResult
  ApprovalDecision
  CustomToolResult
  UserMessage
  ContinuationSignal
```

约束：

1. 扩展同一个 tagged union。
2. 不新增 `submit_approval`、`submit_custom_tool_result`、`submit_resume_signal` 等 parallel API。
3. 每种 input 必须引用明确 pending request id 或 run identity。
4. `turn_id` 等于 Agent run identity，内部 `run_id == turn_id`；如果 Service 已从 active turn / pending action 恢复出原始 turn，必须继续传到 Coordinator / Harness。
5. 外部 `SubmitDynamicToolResultRequest` 不新增 `turn_id`，Service 负责把 session-level 产品 API 转换成内部 `AgentDispatchInputParams { turn_id, ... }`，Coordinator 再转换为 `AgentInputParams`。

#### AgentCapabilities

当前形态：

```text
streaming
interrupt
dynamic_input
```

候选扩展：

```text
approval_flow
custom_tools
mcp
sandbox
structured_output
usage
continuation_recovery
attach_live_task
resume_after_process_restart
```

约束：

1. capability 是 Coordinator / Harness validation 输入，不只是 UI 展示。
2. 不支持能力必须 fail fast。
3. continuation capability 必须区分 attach、resume、replay、cancel remote task。
4. 本轮不新增 standalone resume / continuation 字段；当前 dynamic tool result continuation 和 `resume_context` 先复用 `dynamic_input` 门禁。
5. 后续如果落地 backend continuation recovery，再新增真实可验证的 continuation capability，不一次性暴露候选全集。

#### AgentStatus

当前形态：

```text
Enabled
Disabled
```

候选扩展：

```text
AgentStatus
  lifecycle
  health
  active_runs
  recoverable_runs
  last_error?
  updated_at
```

约束：

1. `active_runs` 是 Harness / Runtime 内存或 backend 视图。
2. Service active turn 仍由 Service store 管理。
3. 不把产品 session 状态和 runtime health 混为一个对象。

#### AgentError / AgentErrorKind

本轮落地真实 Agent domain error 类型。Agent / Harness / Runtime / Coordinator 内部使用 `AgentError` / `AgentErrorKind`；Service 边界再映射回 `TalosError` 和产品 API error。

边界约束：

1. `AgentError` 是 Agent domain 内部错误，不是 Gateway / SDK / app protocol wire DTO。
2. `AgentError` 不导出到 generated TS schema，不作为 HTTP / JSON-RPC response body。
3. Service 负责把 `AgentError` 映射到 `TalosError` 或产品层错误码；Gateway / SDK 只看到稳定产品错误语义。
4. runtime private error 只能放在 internal diagnostic detail，不能作为公共错误类型或公共 code。

```text
AgentError
  kind: AgentErrorKind
  message
  retryable
  details?
  source_message?
```

基础分类：

```text
AgentNotFound
AgentDisabled
AgentUnavailable
RuntimeUnavailable
CapabilityUnsupported
InvalidExecutionContext
RunNotFound
RunConflict
InputRejected
ApprovalExpired
Interrupted
RuntimeError
Timeout
ContinuationUnsupported
ContinuationNotFound
ContinuationExpired
ContinuationInvalid
ContinuationConflict
BackendTaskCompleted
BackendTaskLost
```

约束：

1. runtime private error 只能作为 diagnostic detail。
2. Service / Gateway 最终错误响应仍按产品 API 映射。
3. 不为每个 runtime 定义一套错误 enum。
4. Gateway / SDK 不直接感知 Agent 内部错误类型。

## 小框架抽取清单

### AgentDefinition Framework

当前已有基础：

```text
AgentDefinitionConfig
  -> AgentDefinitionLoader
      -> AgentDefinition
          -> AgentHarnessFactory
              -> AgentHarness
                  -> AgentRuntime
          -> AgentRegistry
              -> Coordinator
```

后续要求：

1. `AgentDefinitionLoader` 只负责配置到 domain definition。
2. `AgentHarnessFactory` 属于 Harness 层，负责公共基础设施组装。
3. `AgentRegistry` 属于 Coordinator 层，负责 routing / scheduling 查询。
4. `runtime_config` 是强类型 tagged union。
5. 新增 backend 只新增 runtime config variant 和 AgentRuntime implementation。

### AgentRouter Framework

候选 API：

```text
resolve_agent(AgentSelectionParams) -> AgentSelectionResult
```

选择优先级：

```text
internal explicit agent_id
  > internal explicit agent_name
  > user/session binding
  > coordinator routing policy
  > configured default agent
```

约束：

1. `workspace_id` 不直接选择 Agent。
2. workspace 到 Agent 的绑定只能作为显式 routing policy 或 user/session binding。
3. Router 不解析 runtime private config。
4. 本轮不把 `agent_id` / `agent_name` 暴露到 Gateway / SDK `StartTurnRequest`；外部显式选择留到多 Agent 产品形态明确后再开放。

### Capability Validation Framework

候选职责：

1. 校验 run option 是否被 selected Agent 支持。
2. 校验 input / interrupt / resume 是否被 runtime 支持。
3. 校验 unsupported capability 时 fail fast。
4. 给 Service / UI 提供可解释错误。

落地顺序：

1. Stage 3 只负责 `AgentRunOptions` / `AgentRunContext` 结构成型，不做完整 unsupported option fail-fast。
2. Stage 4 统一落地 capability validation，作为 unsupported option、input、interrupt、resume 的唯一 fail-fast 入口。
3. Stage 4 不新增 standalone resume capability；当前 `resume_context` / dynamic continuation 使用 `dynamic_input` 门禁。
4. validation error 必须进入 `AgentErrorKind::CapabilityUnsupported` 或更具体的 Agent error kind，再由 Service 映射到产品错误。

### Run Lifecycle Framework

统一管理：

1. `run`。
2. `run_streamed`。
3. `interrupt`。
4. `submit_input`。
5. dynamic input resume。
6. active runtime handle。
7. queued run / backlog。
8. backend continuation recovery。
9. attach live backend task。

其中：

1. Service timeline / live recovery 已存在，仍归 Service / Gateway。
2. backend continuation recovery 是 Harness / Runtime 的未来能力。
3. 两者通过 `AgentEvent` 和 timeline pipeline 衔接，不能混成一个 API。
4. lifecycle 内部所有 run identity 都使用真实 `turn_id`；如果代码中存在 `run_id` 概念，它必须等于同一个 `turn_id`。

### Event Normalization Framework

统一链路：

```text
runtime private event
  -> AgentEvent
      -> Harness validation / enrichment
          -> Coordinator
              -> Service timeline pipeline
                  -> TimelineEventEnvelope
```

约束：

1. concrete runtime 不能把 raw SDK event 泄漏给 Coordinator。
2. `AgentEvent` 是真实公共类型，不用 alias。
3. `TimelineEventEnvelope` 只负责恢复游标和 envelope metadata。
4. metrics live-only、terminal event、pending dynamic action 语义必须保持。

### Input Bridge Framework

统一入口：

```text
submit_input(AgentInputParams) -> AgentInputResult
```

约束：

1. dynamic tool result、approval、custom tool result、continuation signal 都进入 `AgentInput` tagged union。
2. 不为每种 input 新增一组 Harness API。
3. Service 仍负责持久化 pending action 校验。

### Error Mapping Framework

统一路径：

```text
runtime private error
  -> AgentError
      -> Coordinator / Service error
          -> Gateway / SDK response
```

标准对象：

```text
AgentError
  kind: AgentErrorKind
  message
  retryable
  details?
  source_message?
```

基础错误分类：

```text
AgentErrorKind
  AgentNotFound
  AgentDisabled
  AgentUnavailable
  RuntimeUnavailable
  CapabilityUnsupported
  InvalidExecutionContext
  RunNotFound
  RunConflict
  InputRejected
  ApprovalExpired
  Interrupted
  Timeout
  RuntimeError
  ContinuationUnsupported
  ContinuationNotFound
  ContinuationExpired
  ContinuationInvalid
  ContinuationConflict
  BackendTaskCompleted
  BackendTaskLost
```

映射示例：

```text
Codex thread not found       -> ContinuationNotFound
Claude session expired       -> ContinuationExpired
Kernel active run missing    -> RunNotFound
Runtime does not support MCP -> CapabilityUnsupported
Run already active           -> RunConflict
User cancelled               -> Interrupted
Model/provider timeout       -> Timeout
Unknown backend error        -> RuntimeError
```

过渡期 `TalosError` 映射：

```text
TalosError::AgentNotFound        -> AgentNotFound
TalosError::ResourceNotFound     -> RunNotFound / ContinuationNotFound，根据调用点语义区分
TalosError::Conflict             -> RunConflict / ContinuationConflict，根据调用点语义区分
TalosError::ValidationError      -> InvalidExecutionContext / InputRejected，根据参数域区分
TalosError::ToolError            -> InputRejected / RuntimeError，根据 pending input 校验阶段区分
TalosError::Timeout              -> Timeout
TalosError::UnsupportedModel     -> CapabilityUnsupported / RuntimeUnavailable，根据 runtime kind 区分
TalosError::InternalError        -> RuntimeError，除非上层能证明是框架 invariant 破坏
```

约束：

1. 不用 fallback 掩盖 runtime 失败。
2. capability unsupported、run conflict、run not found、continuation expired 必须区分。
3. runtime private error 只能放在 diagnostic detail，不能成为公共错误类型。
4. Gateway / SDK 只看到稳定错误语义，不看到 Codex / Claude / Kernel 私有对象。
5. `retryable` 由 Harness / Coordinator 判断，不由 Gateway 猜。
6. Service 可以再把 `AgentError` 映射为产品 API error，但不能丢掉关键语义。
7. Error Mapping 不包含 tracing/observability 设计；tracing 后续独立设计，只可复用 `turn_id`、`session_id`、`agent_id`、`AgentErrorKind` 等诊断字段。

## Recovery 是 run lifecycle 的子能力

### 已有能力

Service / Gateway 已有：

```text
get_active_turn
recover_timeline
subscribe_timeline
timeline SSE / app protocol subscription
```

这些能力恢复客户端渲染状态，不等于恢复 backend task。

### 本轮已落地边界

本轮只把 dynamic input continuation 显式纳入 Agent run lifecycle：

1. `AgentResumeParams` 是内部 Agent domain params，携带原始 `AgentRunParams`，其 `turn_id` 必须继续等于同一个 Talos turn。
2. `AgentHarness::resume` / `AgentHarness::resume_streamed` 是 Coordinator 调用的标准 facade；它们不作为 Gateway / SDK API 暴露。
3. `AgentRuntime::resume` / `AgentRuntime::resume_streamed` 是 Harness 内部 SPI；concrete runtime 可复用已有 backend continuation 实现，但不能把 backend thread / session id 传到上层。
4. Coordinator 只有在 `resume_context.is_some()` 时选择 resume API；普通用户请求仍走 `run` / `run_streamed`。
5. 普通 `run` / `run_streamed` 携带 `resume_context` 时必须 fail-fast，调用方必须改走 `resume` / `resume_streamed`。
6. `AgentHarness` 默认 `resume` / `resume_streamed` 不能退化调用普通 run；未实现 resume 的 harness 应显式返回 unsupported。
7. Harness lifecycle 必须保留 start / resume mode；排队后不能把 resume 退化成第二次 `run_streamed`。
8. Capability fail-fast 由 `AgentCapabilityValidator` 统一处理：resume 必须有 continuation context，并复用当前 `dynamic_input` capability；streaming resume 还必须有 `streaming` capability。
9. Service 的 `get_active_turn`、`recover_timeline`、`subscribe_timeline` 仍是产品 timeline recovery API，不下沉到 Harness。
10. 本轮不新增 `AgentAttachParams`、`attach`、`AgentContinuationRef` 或 `runtime_ref` 字段。

### 待补能力

backend continuation 候选 API：

```text
resume(AgentResumeParams) -> AgentRunResult
resume_streamed(AgentResumeParams) -> AgentRuntimeStream
attach(AgentAttachParams) -> AgentRuntimeStream
```

候选结构：

```text
AgentContinuationRef
  agent_id
  runtime_kind
  session_id
  turn_id
  message_id?
  backend_state
  created_at_ms
  expires_at_ms?
  version
```

约束：

1. continuation ref 是 internal persisted envelope，不是 Gateway / SDK public DTO。
2. raw backend token 只能在对应 runtime 内解码。
3. 不允许裸 `runtime_ref: String` 在多层 API 之间流动。
4. 如果不能恢复，必须显式返回 unsupported / expired / invalid / lost。
5. 本轮 `resume` / `resume_streamed` 只承载 dynamic input continuation，不实现完整 backend continuation recovery。
6. `attach` 是未来可选 SPI，本轮不暴露、不实现。
7. `resume` / `resume_streamed` 必须继续使用原始 `turn_id`；任何 `run_id`、backend thread id、backend session id 都只能映射到同一个 Talos `turn_id` 或保留在 runtime-private diagnostic 中。

## 禁止重复 DTO

```text
RuntimeRunParams
RuntimeRunResult
RuntimeResumeParams
RuntimeResumeResult
HarnessRunParams
HarnessRunResult
HarnessResumeParams
HarnessResumeResult
KernelRunParams
KernelRunResult
KernelResumeParams
KernelResumeResult
CodexRunParams
CodexRunResult
ClaudeRunParams
ClaudeRunResult
AgentUsage
RunUsage
AgentItem
```

允许 concrete runtime 内部存在 SDK 私有对象，例如 Codex thread item、Claude message、Kernel invocation context，但这些对象必须在 runtime 内部转换成共享 Agent domain 对象后再跨层。

## 强制注释要求

所有分层 API、核心结构和小框架入口必须带英文 doc comment 或等价设计注释，用于说明业务边界和未来演进约束。

要求：

1. Public trait、public struct、public enum、public type alias、public function 必须说明 owner、职责和禁止承载的跨层职责。
2. Crate-private SPI 如果承担框架边界职责，也必须有注释，例如 `AgentRuntime`、runtime stream、runtime abort handle。
3. 核心数据结构必须说明它是否属于 Service / Coordinator / Harness / Runtime / Store / Gateway 边界。
4. 可扩展 enum 必须说明新增 variant 的条件，避免为了单个 backend 暴露 runtime-private 概念。
5. Factory、Loader、Registry、Router、Capability Validator、Lifecycle、Event Normalizer、Input Bridge、Error Mapper 这些小框架入口必须说明谁调用、谁实现、不能依赖哪些上层对象。
6. 注释必须使用英文；团队讨论和 spec 解释可以使用中文。
7. 新增 API 如果没有注释，不应通过 review。

## 落地阶段建议

1. 审查当前 032 改动，列出仍是过渡态的 API 和结构。
2. 固化命名规范，先禁止新增重复 DTO。
3. 收敛 `AgentRunContext.turn_id`、`AgentDispatchInput` / `AgentInput`、`AgentCapabilities`。
4. 最小引入 `AgentRunContext` 和 `AgentRunOptions`，再用 `AgentRunConfig` / `AgentRunSessionState` 完整替换 run 边界的 `TalosSession`。
5. 抽取 capability validation 和 error mapping 小框架。
6. 抽取 run lifecycle 小框架，把 interrupt / input / backlog / dynamic resume 放到同一模型。
7. 在 run lifecycle 下评审 backend continuation recovery，而不是单独做 recovery API。
8. 接入 Codex / Claude spike 时，只实现 `AgentRuntime`，复用 Harness 公共框架。
9. 最后补充 Service / Gateway / SDK 文档和 schema 影响。

## 审查清单

1. 每层 API 是否符合 `Request/Response`、`Params/Result`、`Record/Store` 命名规则？
2. 同一概念是否只有一套核心结构？
3. 新增 runtime 是否只实现 SPI，没有复制 Harness lifecycle？
4. Service store record 是否没有泄漏到 Harness / Runtime / SDK？
5. `AgentEvent` 是否仍是唯一执行事件模型？
6. `RunItem` / `TokenUsage` 是否没有被 `AgentItem` / `RunUsage` 重新分裂？
7. `workspace_id` 是否没有被误用为 Agent selection key？
8. recovery 是否被放在 run lifecycle 下，而不是覆盖整体设计主线？
9. unsupported capability 是否显式失败？
10. 是否存在为未来 Codex / Claude 留下的不可达半成品 API？
11. `AgentRunOptions` 是否仍只包含本轮真实可达的 `current_model?`，没有混入 `continuation_policy`？
12. `AgentError` / `AgentErrorKind` 是否只停留在 Agent domain 内部，没有出现在 Gateway / SDK / generated TS？
13. `resume_streamed` 是否只继续同一 `turn_id`，没有被当作第二次 `run_streamed`、attach 或 Service timeline recovery？
14. `AgentRunParams` 是否已经移除完整 `TalosSession`，剩余 `TalosSession` 是否只在 Service / Coordinator adapter、session lifecycle 或明确内部转换点？
15. Agent domain 中是否只有一个 run identity：`run_id == turn_id`；历史 `trun_id` 是否只作为误写被纠正，没有形成字段、alias 或兼容入口？
16. unsupported option / input / interrupt / resume 是否只通过 capability validation framework fail-fast？
17. 最终验收是否按 PLAN 顺序执行：非 E2E 全通过、完整 E2E 一次、失败 group / case 单跑、Web / TUI smoke？
