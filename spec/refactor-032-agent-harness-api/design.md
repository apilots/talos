# Agent Harness API 统一设计

Status: implemented
Owner: agent
Created: 2026-06-30
Updated: 2026-07-01
Related: `refactor-031-api-concept-alignment`, `refactor-028-orchestration-store-service-layout`, `refactor-023-agent-kernel-simplification`, `refactor-024-agent-kernel-final-answer-simplification`, `refactor-033-agent-framework-api-standardization`

## 背景

Talos 已经完成 Service、Gateway、SDK、app-server protocol 和 store boundary 的一轮核心概念对齐。剩余关键问题是 Agent API 抽象：如何用一套稳定 API 表达不同 Agent runtime 的能力，并让 Coordinator 可以管理、选择和调度 Agent。

本设计基于以下分层：

```text
Gateway / SDK / UI
  -> TalosService
      -> TurnExecutor / AgentDispatchRuntime adapter
          -> Coordinator
              -> AgentHarness
                  -> KernelAgent implements AgentRuntime
                  -> CodexAgent implements AgentRuntime
                  -> ClaudeAgent implements AgentRuntime
```

其中：

1. `TalosService` 负责产品会话、timeline、active turn、project/workspace、DB/file 等业务持久化融合。
2. `Coordinator` 负责 Agent 注册、选择、路由、调度和跨 Agent 管理。
3. `AgentHarness` 是 Coordinator 面向 Agent 执行能力的统一 facade，同时包含公共实现，不只是 trait 规范。
4. `AgentRuntime` 不是独立业务层级，而是 `AgentHarness` 内部用于接入不同 Agent backend 的最小执行契约，由 `KernelAgent`、`CodexAgent`、`ClaudeAgent` 等实现。

本 spec 定义 `Coordinator -> AgentHarness` 的对上 API，以及 `AgentHarness` 内部接入 `AgentRuntime` 实现的 SPI 边界、命名和核心数据结构。Gateway / SDK / app-server 的产品 API 首版不重做。更完整的分层 API 标准化、核心结构继续收敛、小框架抽取，以及 backend continuation、`resume_run`、`runtime_ref` 等 run lifecycle 能力，统一放入 `refactor-033-agent-framework-api-standardization` 评审。

## 当前代码基线

当前代码已经有接近目标分层的链路：

```text
TalosService
  -> TurnExecutor
      -> AgentDispatchRuntime
          -> Coordinator
              -> AgentHarness
                  -> talos_agent::AgentHarnessImpl
                      -> Kernel runtime internals
```

关键事实：

1. `TurnService` 已经负责 idempotency、active turn、first event timeout、timeline 持久化和恢复。
2. `AgentDispatchRuntime` 是 Service 到 Coordinator 的现有内部 dispatch 边界，包含 `handle`、`handle_stream`、`cancel_active_turn`、`submit_dynamic_tool_result` 等方法。
3. `Coordinator` 当前通过 `Arc<dyn AgentHarness>` 注册 Agent，并实现 `AgentDispatchRuntime` 与 `SessionRuntime`。
4. `talos_agent::AgentHarnessImpl` 当前是公共 Harness 实现的起点，但仍混合了 Harness 公共职责和 Kernel runtime 职责。
5. `AgentEvent` 已在 `talos_types::talos` 中作为 Agent 执行事件的 canonical 概念暴露，并已用于内部执行/dispatch stream 边界、Service timeline store、Gateway schema 和 TypeScript generated API。

因此首版不是另起一套外部 API，而是把现有 `AgentHarnessImpl` 继续拆成清晰的 `AgentHarnessImpl + KernelAgent implements AgentRuntime`。`AgentRuntime` 是被 Harness 持有和调用的接入契约，不是独立调度层。

## 外部参考与取舍

### Codex SDK

Codex SDK 的核心形态很克制：

```text
Codex
  startThread(options?)
  resumeThread(id, options?)

Thread
  run(input, options?)
  runStreamed(input, options?)
```

Talos 吸收 `run` / `run_streamed` 的命名，因为它直接表达一次执行。Talos 不把 `Thread` 作为公共概念，因为 Talos 已经有产品层 `Session`，一次用户请求背后的 Agent 执行可能绑定、恢复或切换不同底层线程，也可能产生并行子执行。把 `session_id` 固定映射成 backend thread 会误导架构边界。

### Claude Agent SDK / Claude Code

Claude Agent SDK / Claude Code 相关能力通常包括：

1. 非流式或流式 query / message 执行。
2. session resume / continue。
3. interrupt。
4. permission / approval callback。
5. hooks。
6. custom tools。
7. MCP。
8. settings / model / permission mode。
9. long-running task control。

Talos 首版只吸收当前业务需要的最小稳定能力：`run`、`run_streamed`、`submit_input`、`interrupt`。动态输入后的续跑通过带 `resume_context` 的下一次 `run` / `run_streamed` 表达，不单独定义 `resume_run`。这不是否定 `resume_run` / backend continuation 的价值，而是把它从当前实现 spec 中剥离到后续独立设计。Hooks、subagents、handoff、fork、settings manager 等能力不进入首版 Harness API。

### Claude Agent Manager

Claude Agent Manager 这类管理/编排能力与 Talos `Coordinator` 处于同一层级。它不是 `RemoteAgent`，也不是 concrete runtime。未来如果 Talos 需要多 Agent manager 策略，应扩展 `Coordinator` 或新增同层 orchestration manager。

## 核心概念

| Concept | Owner | Meaning |
| --- | --- | --- |
| `Session` | `TalosService` | Talos 产品会话，唯一对外 `session_id` 来源，持久化。 |
| `Turn` | `TalosService` | 用户一次请求的产品语义单元。 |
| `Run` | `AgentHarness` | Agent 对一个 turn 的执行实例；`turn_id` 就是 run identity，不再引入独立 `run_id`。 |
| `Runtime` | `AgentHarness` internal SPI | Harness 接入具体 Agent backend 的最小执行契约。 |
| `Event` | `AgentHarness` / `AgentRuntime` | run 的标准流式输出。 |
| `Item` | `AgentHarness` / `AgentRuntime` | run 产生的结构化结果片段。 |

命名约束：

1. 跨层 API 只使用 `session_id`，不使用 `service_session_id`。
2. `session_id` 永远表示 Talos product session id，不表示 Claude session、Codex thread 或 Kernel runtime id。
3. `Thread` 不作为 AgentHarness 公共概念。
4. concrete runtime 的私有 thread/session/conversation/run 留在 runtime 实现内部，首版不定义跨层 `runtime_ref`。
5. `workspace_id` 是 execution workspace context key，不是 Agent selection key，也不是 filesystem path。

## 目标

1. 建立统一 `AgentHarness` API，让 Coordinator 只依赖 Harness，不直接依赖具体 Agent runtime。
2. 建立统一 `AgentRuntime` SPI，让 Kernel / Codex / Claude 按同一最小执行契约接入 Harness。
3. 用 `run` / `run_streamed` 表达 Agent 执行能力，保持 API 简洁直接。
4. 用 `submit_input`、`interrupt` 表达最小运行时控制面；动态续跑复用 `run` / `run_streamed` 和 `resume_context`。
5. Harness 和 Runtime 共享核心数据结构，禁止每层重新发明一套 `RunParams` / `RunResult` / `InputParams` / `Event`。
6. 保持 Service 持久化边界不变，禁止 DB record / store 对象进入 Harness 或 Runtime 公共 API。
7. 为未来 Codex / Claude runtime 接入保留清晰边界，但首版不暴露半成品能力。

## 非目标

1. 不在本 spec 中实现 CodexAgent 或 ClaudeAgent。
2. 不把 Claude Agent Manager 建模成 runtime。
3. 不引入 `RemoteAgent` 首版概念。
4. 不把 `Thread` / `Conversation` / backend `Session` 暴露为 Harness 公共 API。
5. 不改变 Gateway / SDK / app-server protocol 的现有产品 API。
6. 不把 timeline store、session store、active turn store 移入 AgentHarness。
7. 不设计完整 subagent、handoff、A2A 或 multi-agent collaboration protocol。
8. 不设计完整 Agent framework 后续阶段，包括更完整的小框架抽取、跨 runtime 能力模型、backend runtime continuation、跨进程 run 恢复、`runtime_ref` 或 `resume_run`；这些进入 `refactor-033-agent-framework-api-standardization`。
9. 不移除或弱化现有 Service timeline recovery / live recovery；它们仍属于 Service / Gateway 产品 API。
10. 不在本 spec 中写执行阶段；执行阶段如需要应放入根目录 `IMPLEMENTATION_PLAN.md`。

## 目标架构

```text
TalosService
  -> TurnExecutor / AgentDispatchRuntime adapter
      -> Coordinator
          -> AgentRegistry
          -> AgentRouter
          -> AgentHarness
              owns one AgentRuntime implementation:
                KernelAgent
                CodexAgent
                ClaudeAgent
```

从业务链路看，稳定目标是：

```text
Coordinator -> AgentHarness
```

从 Harness 内部看，不同执行后端通过同一 SPI 接入：

```text
AgentHarness
  -> AgentRuntime contract
      <- KernelAgent implements it
      <- CodexAgent implements it
      <- ClaudeAgent implements it
```

`AgentHarnessImpl` 是公共实现层，负责：

1. Agent definition 校验。
2. capability 校验。
3. run lifecycle 管理。
4. runtime handle / cancellation token 管理。
5. pending input / approval / dynamic tool result bridge。
6. interrupt 协调。
7. event ordering、event enrichment 和 timeline adapter 前置校验。
8. usage / trace metadata 汇总。
9. error mapping。

`AgentRuntime` 是 Harness 内部接入 SPI，负责定义具体 Agent backend 必须提供的最小执行能力：

1. 对接具体 runtime client / process / SDK。
2. 执行 `run` / `run_streamed`。
3. 把 runtime 私有 SDK event 转成标准 `AgentEvent` 后再跨出 runtime 边界。

首版当前实现中，`submit_input`、`interrupt`、dynamic tool result resume 和 active run handle 管理由 `AgentHarness` 负责。只有当后续 runtime 需要 backend continuation token、remote thread control 或跨进程恢复时，才在独立 spec 中扩展 Runtime SPI。

`AgentRuntime` 不负责调度、不负责 Agent 选择、不保存业务持久化状态，也不作为 Coordinator 可直接依赖的层级。

## 职责边界

### TalosService

`TalosService` 负责产品语义和持久化：

1. 创建、读取和更新产品 `Session`。
2. 管理 `Turn` 和持久化 active turn。
3. 写 timeline。
4. 处理 idempotency。
5. 管理 project、workspace document、DB/file 状态。
6. 组装 Gateway / SDK response DTO。
7. 调用 Coordinator 执行 Agent run。

`TalosService` 不直接调用 `AgentRuntime`，不理解 backend thread/session。

### Coordinator

`Coordinator` 负责 Agent 管理和调度：

1. 加载、注册、注销 Agent。
2. 维护 `agent_id` / `agent_name` 到 `AgentHarness` 的索引。
3. 基于请求、用户绑定、session binding、默认 Agent、routing policy 选择 Agent。
4. 调用 `AgentHarness` 执行 run。
5. 把 `AgentEvent` 交给 Service/timeline pipeline。
6. 保留当前 `AgentDispatchRuntime` 和 `SessionRuntime` adapter，作为迁移期 Service 调用入口。

`Coordinator` 不应：

1. 直接调用 `KernelAgent`、`CodexAgent`、`ClaudeAgent`。
2. 解析 Codex / Claude / Kernel 私有 event。
3. 写 DB record。
4. 根据 `workspace_id` 隐式选择 Agent。

### AgentHarness

`AgentHarness` 是 Coordinator 面向 Agent 执行能力的标准 facade，也是公共实现承载层。

它负责：

1. 暴露 `run`、`run_streamed`、`submit_input`、`interrupt`。
2. 暴露 `descriptor`、`capabilities`、`status`。
3. 管理 run 内存态、active runtime handle、backlog、cancellation。
4. 标准化 event / item / usage。
5. 管理 pending input、approval、dynamic tool result。
6. 映射 runtime error。

它不负责：

1. 写 session store / timeline store / active turn store。
2. 生成 Gateway / SDK response DTO。
3. 管理 project DB record。
4. 持久化 workspace document。
5. 暴露 runtime 私有 thread/session/event。

### AgentRuntime

`AgentRuntime` 是 `AgentHarness` 内部用于接入具体 Agent backend 的统一 SPI。它不是独立产品层，也不是编排层。

| Runtime | Meaning |
| --- | --- |
| `KernelAgent` | Talos 当前内置 Agent runtime，首个落地实现。 |
| `CodexAgent` | 未来对接 Codex SDK / Codex runtime。 |
| `ClaudeAgent` | 未来对接 Claude Agent SDK / Claude Code runtime。 |

`KernelAgent`、`CodexAgent`、`ClaudeAgent` 可以有内部 thread、session、conversation、client、process、tool runtime、MCP connection、sandbox 等私有概念，但这些概念首版只能停留在 runtime 实现内部，不能进入 AgentHarness / Coordinator / Service / Gateway / SDK 的公共对象。

## 共享数据结构原则

`AgentHarness` 和 `AgentRuntime` 有不同 API 边界，但 `AgentRuntime` 只是 Harness 内部 SPI，因此它们必须共享核心数据结构：

```text
AgentRunParams
AgentRunResult
AgentInputParams
AgentInputResult
AgentInterruptParams
AgentInterruptResult
AgentEvent
RunItem
TokenUsage
```

禁止新增以下重复 DTO 家族：

```text
RuntimeRunParams
RuntimeRunResult
RuntimeInputParams
RuntimeInputResult
RuntimeInterruptParams
RuntimeInterruptResult
HarnessRunParams
HarnessRunResult
KernelRunParams
KernelRunResult
CodexRunParams
CodexRunResult
ClaudeRunParams
ClaudeRunResult
```

允许 runtime 内部存在 SDK 私有对象，例如 Codex thread item、Claude message、Kernel context event，但这些对象必须在 runtime crate 内部转换成共享执行结构后再跨层。

命名约束：

1. Service / Gateway / SDK / app-server protocol 等对外公共类型使用 `*Request` / `*Response`。
2. Coordinator / AgentHarness / AgentRuntime SPI 内部执行 API 使用 `*Params` / `*Result`。
3. 同一语义不能同时定义 `AgentRunRequest` 和 `AgentRunParams`；内部 Agent API 统一使用 `AgentRunParams`。
4. 同一语义不能靠 alias 长期保留双名；目标态应重命名已有对象并删除旧公共名。
5. `AgentDispatchParams` / `AgentDispatchResult` 属于 Service -> Coordinator adapter 边界；新的 AgentHarness / AgentRuntime API 同样不沿用 `Request` / `Response`。

## 分层 API 列表

### Service API

Service 面向产品能力，不暴露 Agent runtime 私有细节。现有产品 API 形态应保持：

```rust
TalosService
  start_turn(request: StartTurnRequest) -> StartTurnResponse
  interrupt_turn(request: InterruptTurnRequest) -> InterruptTurnResponse
  submit_dynamic_tool_result(request: SubmitDynamicToolResultRequest) -> SubmitDynamicToolResultResponse
  get_active_turn(request: SessionActiveTurnRequest) -> TimelineActiveTurnSnapshot
  cancel_active_turn(session_id) -> ()
  subscribe_timeline(request: SubscribeTimelineRequest) -> SubscribeTimelineResponse
  timeline_subscription_source(request: SubscribeTimelineRequest) -> TimelineSubscriptionSource
  recover_timeline(request: RecoverTimelineRequest) -> TimelineRecoveryResponse
```

约束：

1. Service API 使用 `session_id`、`turn_id`。
2. Service API 不暴露独立 `run_id`；`turn_id` 就是 Agent run identity。
3. Service API 不暴露 `runtime_ref`。
4. 流式输出仍通过 timeline subscription / recovery 表达，不新增 `TurnStream` 产品 API。
5. dynamic tool result 仍使用现有产品语义，不新增 `submit_turn_input`。
6. `get_active_turn`、`recover_timeline`、`timeline_subscription_source` 是 live recovery / crash 后 UI 恢复的产品 API，归 Service / Gateway 管理，不下沉到 Harness / Runtime。
7. Service 通过 timeline store 和 active turn store 恢复客户端渲染状态；这不等同于 runtime backend continuation。

### Gateway / App Protocol Recovery API

现有 live recovery / timeline recovery 对外暴露在 Gateway / app-server protocol，不在 AgentHarness。

HTTP API：

```text
GET /v1/sessions/:session_id/active-turn
GET /v1/sessions/:session_id/timeline/recovery
GET /v1/sessions/:session_id/timeline/events
```

App protocol API：

```text
session.active_turn
timeline.recover
timeline.* subscription notifications
```

约束：

1. 这些 API 恢复的是产品 timeline、active turn snapshot 和 live event stream。
2. `TimelineEventEnvelope` 提供 `event_id`、`event_seq`、`created_at_ms` 等恢复游标元数据。
3. `TurnMetrics` 是 live-only event，不推进持久化 cursor。
4. cursor 过期应显式返回资源过期错误，不能 silent fallback 到不完整历史。
5. 如果服务进程崩溃导致 Runtime 内存执行句柄丢失，Service 可以恢复 UI 状态和已持久化事件，但不能在本 spec 中承诺原 backend task 原地续跑。

### Coordinator API

Coordinator 面向 Agent 管理和编排：

```rust
Coordinator
  register_agent(agent_id: &str, agent: Arc<dyn AgentHarness>) -> ()
  registry() -> AgentRegistry

AgentRegistry
  register(agent_id: &str, harness: Arc<dyn AgentHarness>) -> ()
  get(agent_id: &str) -> AgentRegistration
  list() -> Vec<AgentRegistration>

AgentDispatchRuntime adapter
  handle(params: AgentDispatchParams) -> AgentDispatchResult
  handle_stream(params: AgentDispatchParams) -> AgentEventStream
  cancel_active_turn(params: CancelActiveTurnParams) -> ()
  submit_dynamic_tool_result(session, call_id, tool_name, output, success) -> ()

SessionRuntime adapter
  resolve_target_agent(context: &TalosContext) -> String
  compact_session(session: TalosSession) -> TokenUsage
  clear_session(session: TalosSession) -> ()
  close_session(session: TalosSession) -> ()
  prepare_delete_session(session: TalosSession) -> ()
```

约束：

1. 首版 Coordinator 的公开 trait 只新增注册能力；执行入口继续复用现有 `AgentDispatchRuntime` / `SessionRuntime` adapter，避免同时重做 Service / Gateway / SDK 产品 API。
2. `AgentDispatchParams` 是 Service -> Coordinator 的 routing envelope，Coordinator 内部转换为共享 `AgentRunParams`。
3. `handle` 对应非流式 `AgentHarness.run`，`handle_stream` 对应 `AgentHarness.run_streamed`。
4. `submit_dynamic_tool_result` 桥接到 `AgentHarness.submit_input`。
5. dynamic tool result 后的续跑由 Service / Coordinator 用同一 `turn_id` / `message_id` 和 `resume_context` 调用 `handle_stream` / `run_streamed`，不新增 `resume_run`。
6. `cancel_active_turn` 桥接到 `AgentHarness.interrupt`。
7. `unregister_agent`、分页 `list_agents`、`get_agent`、`resolve_agent` 等管理 API 是未来 Coordinator 管理面，不是首版落地要求。

### AgentHarness API

```rust
pub trait AgentHarness: Send + Sync {
    fn id(&self) -> &str;

    fn descriptor(&self) -> AgentDescriptor;

    async fn run(&self, params: AgentRunParams) -> TalosResult<AgentRunResult>;

    async fn run_streamed(
        &self,
        params: AgentRunParams,
    ) -> TalosResult<mpsc::Receiver<AgentEvent>>;

    async fn interrupt(
        &self,
        params: AgentInterruptParams,
    ) -> TalosResult<AgentInterruptResult>;

    async fn submit_input(
        &self,
        params: AgentInputParams,
    ) -> TalosResult<AgentInputResult>;

    async fn compact_session(&self, session: TalosSession) -> TalosResult<TokenUsage>;
    async fn clear_session(&self, session: TalosSession) -> TalosResult<()>;
    async fn close_session(&self, session: TalosSession) -> TalosResult<()>;
    async fn drain_active_turns(&self, timeout: Duration) -> TalosResult<()>;
}
```

设计说明：

1. `run` / `run_streamed` 对齐 Codex 的简洁表达，但使用 Talos 的 `Run` 概念。
2. `submit_input` 是统一输入回填 API。
3. `interrupt` 是一等控制 API，不建模成普通 input。
4. `capabilities` 和 `status` 首版通过 `AgentDescriptor` 暴露，不新增同义独立 getter。
5. dynamic input continuation 不是独立 Harness 方法；Service 校验持久化 pending action 后，通过带 `resume_context` 的下一次 `run` / `run_streamed` 继续同一个 `turn_id`。
6. 当前实现使用 `async_trait` 保持 `Arc<dyn AgentHarness>` object-safe registry；如果后续改用 RPITIT，必须同时调整 registry 形态。
7. `compact_session`、`clear_session`、`close_session`、`drain_active_turns` 是当前 `SessionRuntime` 过渡能力，仍在 Harness trait 上服务现有 Service 链路；它们不是 Runtime SPI，也不是 Gateway / SDK 新产品 API。
8. 首版实现中，Service / Coordinator adapter 可以继续保留 `cancel_active_turn`、`submit_dynamic_tool_result` 等产品语义方法名，但进入 Harness facade 前必须转换成 `AgentInterruptParams` 或 `AgentInputParams`，避免 Harness API 暴露散列参数。
9. `AgentInput` 首版只需要包含 dynamic tool result；approval、用户补充输入和未来 runtime-specific input 应扩展同一个 tagged enum，不新增 parallel `submit_*` 方法族。

### AgentRuntime SPI

`AgentRuntime` 是 Harness 内部接入 `KernelAgent` / `CodexAgent` / `ClaudeAgent` 的 SPI，使用与 Harness 相同的核心 params / result / event 类型。首版 Runtime SPI 只承载执行入口；`submit_input` / `interrupt` 仍由 Harness 管理 active run、pending input 和取消桥接后再触发具体 backend 执行。

```rust
pub(crate) trait AgentRuntime: Clone + Send + Sync + 'static {
    fn kind(&self) -> AgentKind;

    fn capabilities(&self) -> AgentCapabilities;

    fn run(
        &self,
        params: AgentRunParams,
    ) -> impl std::future::Future<Output = TalosResult<AgentRunResult>> + Send;

    fn run_streamed(
        &self,
        params: AgentRunParams,
    ) -> impl std::future::Future<Output = TalosResult<AgentRuntimeStream>> + Send;
}
```

设计说明：

1. `AgentRuntime` 是 Harness 内部 SPI，不是 Coordinator 可直接调度的独立业务层级。
2. `KernelAgent`、`CodexAgent`、`ClaudeAgent` 实现 `AgentRuntime`。
3. Runtime 私有 SDK event 不跨出 `AgentRuntime` trait。
4. Runtime 输出标准 `AgentEvent`，Harness 负责 active handle、取消桥接、队列、校验、补齐 metadata、排序和错误映射。
5. `AgentRuntime` 的目标 SPI 使用共享 `AgentRunParams` / `AgentRunResult`；Codex SDK request、Claude SDK message 等只能作为 runtime-private builder input，不能成为 Harness / Runtime 的长期公共契约。
6. 当前实现中 `AgentRuntime` 是 crate-private SPI；它不是 public crate API，不应该被 Coordinator 或 Service 直接依赖。
7. `AgentRuntimeStream` 可以携带 Harness 内部需要的 abort handle 和 completion join handle，但这些仍是 Harness implementation detail，不进入 Gateway / SDK / app-protocol。

### KernelAgent API

`KernelAgent` 是首个 concrete runtime。当前实现中它是 `talos_agent` crate 内部结构体，实现 crate-private `AgentRuntime` SPI，不作为 public trait 暴露：

```rust
pub(crate) struct KernelAgent { ... }

impl AgentRuntime for KernelAgent { ... }
```

约束：

1. Kernel 执行入口必须与 `AgentRuntime` 对齐：`run` / `run_streamed`。
2. `compact_session` 当前仍通过 `AgentHarness` / `SessionRuntime` 过渡边界服务 Service，不提升到 Runtime SPI。
3. 只有当 Codex / Claude / Kernel 都能稳定表达同一能力时，才提升为 Harness capability。
4. Kernel 内部可以使用自己的 context / tool / memory / runtime 对象，但不外泄。

## 核心数据结构

### AgentDefinition

```text
AgentDefinition
  id
  name
  kind: kernel | codex | claude
  enabled
  description?
  default_workspace_id?
  model?
  runtime_config
```

约束：

1. `id` 是稳定 Agent identity。
2. `name` 是人类可读名称，不替代 id。
3. `kind` 决定使用哪个 `AgentRuntime`。
4. `default_workspace_id` 是默认 execution workspace context key，不是 default agent。
5. `runtime_config` 是 concrete runtime 配置，必须强类型化。

### Agent Instantiation Framework

`AgentDefinition` 是从配置到运行实例的框架入口，不只是展示 DTO。目标调用链：

```text
AgentDefinition
  -> AgentDefinitionLoader
      -> AgentHarnessFactory
          -> AgentHarness
              owns common infrastructure
              owns one AgentRuntime SPI implementation
                  KernelAgent | CodexAgent | ClaudeAgent
          -> AgentRegistry
              -> Coordinator routing / scheduling
```

职责归属：

1. `AgentDefinitionLoader` 负责从配置加载多 Agent 定义，并做基础 schema 校验。
2. `AgentHarnessFactory` 属于 Harness 层，负责把 `AgentDefinition` 构建成可运行的 `AgentHarness`。
3. `AgentRegistry` 属于 Coordinator 层，保存 `AgentDescriptor` 和 `AgentHarness`，供路由、选择、调度和未来多 Agent 协作使用。
4. `runtime_config` 是 `AgentDefinition` 的强类型 tagged union，由 Harness factory 根据 `kind` 传给对应 `AgentRuntime` 实现。
5. Tool registry、skill roots、plugin roots、MCP/tool policy、approval policy、event normalization、capability validation、run lifecycle、pending input、interrupt bridge 等公共基础设施归 `AgentHarness`。
6. `KernelAgent` / `CodexAgent` / `ClaudeAgent` 只实现核心执行逻辑和 runtime-private SDK/client/process 接入，通过 `AgentRuntime` SPI 复用 Harness 提供的公共基础设施。
7. 新 Agent backend 的接入路径应是新增 `AgentRuntime` 实现和对应 `runtime_config` variant，不应复制 Harness 已有的 registry、tool、plugin、event、lifecycle 逻辑。

首版落地约束：

1. 配置层使用 `AgentDefinitionConfig` 表达文件 schema，Harness 层使用 `AgentDefinition` 表达已解析的 domain definition；两者不是重复 DTO，而是配置输入和运行域对象的边界。
2. `AgentRuntimeConfig` 首版只包含 `kernel` variant；`codex` / `claude` 可以作为 `AgentKind` 的保留值出现，但没有对应 runtime config variant 时必须失败，不能默认落到 Kernel。
3. `kind` 与 `runtime_config.type` 必须一致，由 `AgentDefinitionLoader` 在进入 Harness factory 前校验。
4. 显式 `agent.agents` 为空时，loader 合成一个 default Kernel definition，以保持当前单 Agent 产品路径；显式多 Agent 配置存在时，Base runtime 只注册 enabled definitions。
5. Coordinator 默认 router binding 指向首个 enabled Agent definition；未来用户绑定、session binding 和多 Agent 协作策略仍归 Coordinator / Router，不进入 Harness 或 Runtime。

### AgentDescriptor

```text
AgentDescriptor
  agent_id
  name
  kind
  description?
  capabilities
  status
```

### AgentRunParams

```text
AgentRunParams
  session
  turn_id?
  resume_context?
  input
```

字段语义：

| Field | Meaning |
| --- | --- |
| `session` | Service / Coordinator 已准备好的 `TalosSession` domain object，不是 DB record。 |
| `turn_id` | 用户一次请求的 turn id；当前代码中是 `Option<String>`，因为部分历史调用仍可从 session context 获取。目标语义是执行路径必须有 turn identity。 |
| `resume_context` | Service / Coordinator 校验后的动态输入续跑上下文，不包含 backend private thread/session id。 |
| `input` | 本次 run 的输入。 |

### PreparedAgentContext

当前实现中 `PreparedAgentContext` 尚未作为独立公共类型落地，已准备的上下文主要承载在 `TalosSession` / `TalosContext` / `AgentDispatchResumeContext` 中。本节定义的是目标语义边界。后续如果抽取结构体，必须从这些已有对象迁移，而不是并行复制一套同义 DTO。

```text
PreparedAgentContext
  session
  project_id?
  workspace_id?
  resolved_workspace_dir?
  working_directory?
  additional_directories?
  environment?
  user_id?
  memory_scope?
  tool_policy?
  skill_context?
```

约束：

1. `PreparedAgentContext` 由 Service / Coordinator 侧 resolver 准备。
2. Runtime 不自行读取产品 DB，也不自行解析 workspace document。
3. 首版可以携带完整 `TalosSession` 以保持现有 Kernel 行为，但这属于 prepared domain context，不是 DB record。
4. `resolved_workspace_dir` 是经过配置和权限校验后的路径。
5. `workspace_id` 仍只是 execution workspace context key。

### Run Identity Mapping

首版不引入独立 `run_id`：

```text
session_id: Talos product session identity
turn_id: user request identity and Agent run identity
message_id: assistant message identity used by timeline/history
request_id: client request / idempotency identity
resume_context: Service / Coordinator supplied continuation context for dynamic input resume
```

约束：

1. `turn_id` 等于 Agent run identity，`AgentRunParams`、`AgentRunResult`、`AgentInputParams` 都使用 `turn_id` 定位 run。
2. `message_id` 由 Service/timeline 路径管理，用于历史消息和 timeline envelope，不替代 run identity。
3. `request_id` 用于幂等、事件 payload 和诊断，不作为 Agent run identity。
4. `resume_context` 只表达 Talos 已校验的 continuation context，不暴露 backend private thread/session/conversation id。
5. `interrupt_turn` 等 Service adapter 通过 `session_id + active_turn.turn_id` 映射到 Harness 内存 active run；映射丢失必须显式返回 `RunNotFound` 或 `RunConflict`，不能 fallback 新建 run。
6. `submit_dynamic_tool_result` 不要求 Harness 内存 active run 存在。`WaitingDynamicInput` 会结束当前 runtime task，Service 校验持久化 pending action 后把动态结果写入 Agent transcript，并用同一个 `turn_id` / `message_id` 和 `resume_context` 启动下一次 `run_streamed`。

### AgentRunResult

```text
AgentRunResult
  output_text
  output_items: Vec<RunItem>
  reasoning
  stop_reason
  usage: TokenUsage
  task_count
  session_id
  generated_session_title?
```

约束：

1. 当前实现的 `AgentRunResult` 不携带 `agent_id` 和 `turn_id` 字段；agent identity 由 Coordinator registry / selected harness 保证，turn identity 由 `AgentRunContext.turn_id` 和 Service timeline / active-turn 状态保证。
2. 如果未来需要在 result 中回显 `agent_id` / `turn_id`，必须先确认对 Gateway / SDK / timeline envelope 的影响，不能和 `TimelineEventEnvelope` 的恢复元数据重复造对象。
3. `output_items` 使用 `RunItem`，`usage` 使用 `TokenUsage`，不新增 `AgentItem`、`RunUsage` 或 `AgentUsage`。

### Workspace / Project / Directory

Workspace context 选择顺序：

```text
request.workspace_id
  > session context workspace_id
  > selected_agent.default_workspace_id
  > global default workspace key
```

Directory 解析顺序：

```text
explicit working_directory
  > resolved workspace directory
  > project default directory
```

约束：

1. `workspace_id` 是 Agent execution workspace context key，不是 user workspace path。
2. `default_workspace_id` 是 Agent 配置上的默认 workspace key，只在请求和 session 没有提供 workspace context 时生效。
3. `default_workspace_id` 会通过 workspace resolver 映射到默认 workspace dir。
4. `workspace_id` 不决定使用哪个 Agent。
5. 如果未来需要 workspace 到 Agent 的绑定，应表达为 Coordinator routing policy 或 user/session binding。
6. Runtime 只能消费 `PreparedAgentContext.resolved_workspace_dir`，不能自行猜测路径。

### AgentRunOptions

当前 032 实现尚未抽出独立 `AgentRunOptions` 公共结构，model / workspace / resume / tool policy 等仍主要通过 `TalosSession`、配置对象和 `AgentDispatchResumeContext` 传递。以下字段是后续抽取 `AgentRunOptions` 时的候选范围，不代表当前已落地 API：

```text
AgentRunOptions
  model?
  approval_policy?
  sandbox_policy?
  max_turns?
  max_budget?
  output_schema?
  stream_detail?
  reasoning_effort?
  web_search?
```

约束：

1. 字段必须表达跨 runtime 可标准化的能力。
2. runtime 不支持的字段必须通过 capabilities 显式失败。
3. 不允许静默忽略关键 option。
4. runtime 私有配置应放在 `runtime_config`，不进入通用 `AgentRunOptions`。

### AgentInputParams

```text
AgentInputParams
  session
  turn_id?
  input
```

```text
AgentInput
  DynamicToolResult
```

约束：

1. `input` 必须是 tagged union。
2. `turn_id` 等于 Agent run identity；Service 从 active turn / pending action 恢复出原始 turn 后必须传到 Coordinator / Harness。
3. 首版当前实现只包含 `DynamicToolResult { call_id, tool_name, output, success }`。
4. approval、用户补充输入、custom tool result 和未来 runtime-specific input 应扩展同一个 tagged enum，不新增 parallel `submit_*` 方法族。
5. `submit_input` 必须返回结构化 result，便于 UI / SDK 表达 queued / accepted / rejected。

### AgentEvent

`AgentEvent` 是 Runtime 对 Harness、Harness 对 Coordinator、Service timeline store 输出的唯一标准 Agent 执行事件模型。目标态不再保留旧 timeline event 公共概念名；代码、JsonSchema 和 TypeScript generated API 应统一到真实 `AgentEvent`：

```text
AgentEvent
  TurnStarted
  ItemStarted
  ItemDelta
  ItemCompleted
  AssistantTextCommitted
  TurnMetrics
  TurnCompleted
  TurnError
```

公共字段：

```text
event_seq?
event_id?
created_at_ms?
event_type
turn_id
message_id
session_id
sequence
payload
```

约束：

1. 每个事件必须包含 `session_id`、`turn_id`、`message_id` 和递增 `sequence`。
2. `event_id`、`event_seq`、`created_at_ms` 由 Service 持久化或 live-only envelope 阶段补齐。
3. 首版保持现有 `TurnStarted` first event、terminal event、pending dynamic action、metrics live-only 语义。
4. 时间戳沿用现有 Unix milliseconds 字段，避免同时出现 `created_at` 与 `created_at_ms`。
5. 事件 union 必须显式 tag。
6. Rust serde rename 与 TypeScript rename 必须一致。
7. 公共事件不允许携带 raw runtime event。
8. 不允许长期同时存在 `AgentEvent` 和旧 timeline event 两个公共类型名。
9. `AgentEvent` 必须是真实类型，不能用 type alias 伪装迁移完成。
10. `TimelineEventEnvelope.event` 必须使用真实 `AgentEvent`，否则 Gateway / SDK 仍会暴露旧公共概念名。
11. 从旧 timeline event 公共名到 `AgentEvent` 是 schema / TS public type breaking change；即使 wire payload 基本不变，也必须同步 SDK、Web、TUI、tests 和文档。

### Run Item / Token Usage Naming

```text
RunItem
  Message
  Reasoning
  CommandExecution
  FileChange
  ToolCall
  WebSearch
  TodoList
  Error
```

设计说明：

1. `RunItem` 是目标态命名，用于表达一次 Agent run / turn 的结构化 transcript item。
2. `TokenUsage` 是目标态命名，用于表达 token 计量，允许被 run result、session aggregate、history message、compaction progress 和 metrics 复用。
3. 已审查现有 usage owner：它不是 run-only 语义，因此不能机械改成 `RunUsage`。
4. 禁止新增字段相同的 `RunUsage` / `AgentUsage` parallel DTO；如果未来需要 run-only usage，应先证明它和 `TokenUsage` 语义不同。
5. run result item、event item、history item 可以是不同对象，但不能字段相同、语义相同、名字不同。
6. `RunItem` 属于 `AgentRunResult`，不是 Agent 实体属性。
7. Timeline 是否一一映射 `RunItem` 由 Service 决定。

### AgentCapabilities

```text
AgentCapabilities
  streaming
  interrupt
  dynamic_input
```

当前实现中的 `AgentCapabilities` 首版只包含上述三个字段。以下能力是后续可以扩展的候选项，但不在 032 首版中作为已落地字段记录：

```text
  approval_flow
  custom_tools
  mcp
  file_changes
  command_execution
  sandbox
  structured_output
  usage
```

约束：

1. capabilities 是 Coordinator 和 Harness 做 request validation 的依据。
2. capabilities 不只是 UI 展示字段。
3. 不支持的能力必须 fail fast。
4. `compaction` 首版不作为通用 Harness capability；继续留在现有 session lifecycle adapter 或 Kernel 专属能力。

### AgentStatus

```text
AgentStatus
  Enabled
  Disabled
```

约束：

1. 当前实现中的 `AgentStatus` 只表达 Agent definition 是否启用。
2. `AgentStatus` 不是产品 session 状态，也不是 Service active turn 状态。
3. 如果未来需要 health、lifecycle、active_runs、last_error，应扩展为独立管理面设计，不能把 Service active turn store 语义塞进 Agent status。

## Agent Selection

Agent 选择建议顺序：

```text
explicit agent_id
  > explicit agent_name
  > user/session binding
  > coordinator routing policy
  > configured default agent
```

约束：

1. `workspace_id` 只影响 selected Agent 的执行上下文。
2. `workspace_id` 不决定使用哪个 Agent。
3. 当前代码的 channel/user routing 可以作为 `coordinator routing policy` 的首版实现。

## Active Run Ownership

active run 有两层状态，但权威不同：

1. `TalosService` 是持久化 active turn 权威，负责对外并发冲突、恢复和 timeline 一致性。
2. `AgentHarness` 是 runtime 内存执行权威，负责 active runtime handle、cancel token、pending input、backlog。
3. `Coordinator` 负责把请求路由到正确 Harness，不写 active turn store。
4. `AgentRuntime` 的具体实现只管理自己的执行对象，不感知 Talos active turn store。

如果 Service 已判定 session 存在 active turn，Harness 不应通过 fallback 新建 run 掩盖冲突。

状态所有权：

| State | Owner | Notes |
| --- | --- | --- |
| persistent active turn | `TalosService` | 存储在 active turn store，对外并发语义以它为准。 |
| run identity | `TalosService` / `AgentHarness` | `turn_id` 就是 run identity。 |
| active runtime handle | `AgentHarness` | Harness 负责保存、查找和释放。 |
| cancellation token / abort handle | `AgentHarness` | Runtime 可以接收 token/handle，但不拥有 Talos run 状态。 |
| pending input / dynamic tool result bridge | `AgentHarness` | Harness 接收结构化 input；Service 校验持久化 pending action，并通过 transcript / `resume_context` 触发后续执行。 |
| session backlog / queue | `AgentHarness` | Harness 负责排队和恢复执行。 |
| backend private session/thread/client/process | `AgentRuntime` implementation | 只在具体 runtime 内解释，首版不跨 Harness / Coordinator 边界透传。 |
| DB record / store state | `TalosService` / store crate | 禁止进入 Harness / Runtime API。 |

首版 `AgentRuntime` SPI 只包含 `run` / `run_streamed` 执行入口。`submit_input`、`interrupt` 保持为 Harness lifecycle API；动态输入续跑通过 `resume_context` 进入下一次 `run` / `run_streamed`。未来只有当 Codex / Claude 等 backend 需要专属 continuation token 或 remote thread control 时，才把对应 backend operation 增补到 Runtime SPI。

## Event 与 Timeline 映射

目标链路：

```text
runtime private SDK event
  -> AgentEvent
      -> AgentHarness validation/enrichment
          -> Coordinator
              -> TalosService timeline store
                  -> TimelineEventEnvelope
```

首版迁移约束：

1. `AgentEvent` 是 Agent 执行事件的 canonical domain object。
2. 旧 timeline event 公共命名是历史命名，目标态应删除；不应作为 spec 目标对象。
3. `TimelineEventEnvelope` 是 Service/Gateway 对外 recoverable timeline envelope，负责携带 `event_id`、`event_seq`、`created_at_ms` 等恢复游标元数据。
4. 必须保持 `TurnStarted` first event、terminal event、pending dynamic action、metrics live-only 等现有语义。
5. Runtime 不允许直接构造 Gateway / SDK DTO。
6. 每个 concrete runtime 都必须输出标准 `AgentEvent`，不能把 raw Codex / Claude / Kernel event 泄漏给 Coordinator。

## Session Lifecycle

当前 `SessionRuntime` 承载以下能力：

```text
compact_session
clear_session
close_session
prepare_delete_session
cancel_active_turn
submit_dynamic_tool_result
```

首版约束：

1. 这些能力继续通过 `SessionRuntime` / `AgentDispatchRuntime` adapter 服务现有 Service。
2. `cancel_active_turn` 和 `submit_dynamic_tool_result` 可以桥接到 `AgentHarness.interrupt` / `AgentHarness.submit_input`。
3. `cancel_active_turn` 在 Service -> Coordinator adapter 边界使用 `CancelActiveTurnParams { session, turn_id }`，其中 `turn_id` 来自持久化 active turn；进入 Harness 前转换为 `AgentInterruptParams`。
4. `compact_session`、`clear_session`、`close_session`、`prepare_delete_session` 暂不提升为通用 Harness API。
5. 如果未来 Codex / Claude 也需要这些能力，应先抽象为 `AgentSessionLifecycle`，再提升到 Harness。
6. Harness 不因此接管 DB 持久化。
7. 如果 Service 已有 active turn 语义，但 Harness 内存 run 不存在，必须返回明确 `RunNotFound` 或 `RunConflict`；不能 silent no-op，也不能 fallback 新建 run。
8. Harness 内存 active run 存在但 `turn_id` 不匹配时，必须拒绝中断，不能仅凭 `session_id` abort。

## Rust 实现约束

RPITIT trait 不是 object-safe。当前 `Coordinator` 使用 `HashMap<String, Arc<dyn AgentHarness>>`，后续迁移必须明确 registry 形态：

1. 当前实现保留 object-safe `AgentHarness` registry，避免 Coordinator 直接绑定 concrete implementation。
2. 如果后续 `AgentHarness` trait 改用 RPITIT，则 Coordinator registry 不能直接存 `Arc<dyn AgentHarness>`，需要明确 concrete registry 或 boxed wrapper。
3. 当前首个落地点是让 `AgentHarnessImpl` 内部持有 `KernelAgent`，并通过 `AgentRuntime` SPI 调用它。
4. 后续新增 Codex / Claude runtime 时，Harness 内部可以再引入 enum dispatch 或 object-safe internal wrapper；该 wrapper 仍然只是 Harness 内部实现细节，不能成为 Coordinator 可直接调度的层级。
5. 不允许同时长期保留旧 `Agent` trait 和新 `AgentHarness` trait 两套等价入口。
6. Kernel 专属 DTO 不能从 Harness crate 公共 API 泄漏；例如 `AgentKernelParams` 若只是 Kernel implementation detail，应收回到 runtime 私有模块或重命名为明确的内部 builder input。

## 错误语义

当前实现仍沿用 `TalosError` 作为跨 crate 错误类型，并在 Service / Coordinator / Harness 的正确层级映射错误。后续如果 Agent domain 需要更稳定的独立错误分类，可以抽取统一 `AgentErrorKind`：

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
  RuntimeError
  Timeout
```

约束：

1. capability 不支持必须显式失败。
2. runtime 私有错误必须在 Harness / Coordinator 边界映射为稳定 Talos 错误语义；如果后续引入 `AgentErrorKind`，再统一映射到该 enum。
3. 原始错误只作为 diagnostic detail。
4. 中断 accepted 不等于 run completed。
5. run 状态冲突必须返回明确错误，不能 fallback 新建 run。

## 与 Service / Store 的边界

```text
KernelAgent / CodexAgent / ClaudeAgent
  -> AgentRuntime SPI
      -> AgentEvent stream
          -> AgentHarness validation/enrichment
              -> Coordinator
                  -> TalosService timeline/store persistence and DTO assembly
```

不允许：

1. `AgentHarness` 返回 `SessionRecord`、`TimelineRecord`、`ActiveTurnRecord`。
2. `AgentRuntime` 直接写 Talos session DB。
3. Gateway / SDK 暴露 runtime private continuation reference。
4. Gateway / SDK 暴露 Kernel / Codex / Claude 私有对象。
5. 为了测试把 store mock 注入 `AgentRuntime`。
6. 每个 runtime 自己实现一套 timeline builder。

允许：

1. `AgentRuntime` 的具体实现可以使用自己的 runtime file/cache/session/thread。
2. `AgentHarness` 保存短生命周期内存态。
3. `TalosService` 持久化 run 相关业务状态，但不解释 runtime private continuation state。

## 首版落地范围建议

首版只做边界收敛：

1. 拆分现有 `talos_agent::AgentHarnessImpl`：公共逻辑保留在 `AgentHarnessImpl`，Kernel 专属执行进入 `KernelAgent`。
2. 定义 `AgentRuntime` SPI 并让 `KernelAgent` 实现它。
3. 定义或整理 `AgentDefinition`、`AgentDescriptor`、`AgentKind`、`AgentCapabilities`、`AgentStatus`。
4. 定义共享 `AgentRunParams`、`AgentRunResult`。
5. 定义共享 `AgentInputParams`、`AgentInputResult`、`AgentInterruptParams`、`AgentInterruptResult`。
6. 定义共享 `AgentEvent`、`RunItem`、`TokenUsage`；`RunItem` 是 run / turn transcript item，`TokenUsage` 是跨 run、session、history、metrics 复用的 token 计量对象，不能新增字段相同的 parallel DTO。`AgentErrorKind` 只作为后续可抽取的错误分类候选，不是 032 当前实现要求。
7. Coordinator registry 改为依赖 Harness 层，而不是直接依赖 concrete Kernel implementation。
8. 保留 `AgentDispatchRuntime` / `SessionRuntime` 作为 Service adapter，避免同时改 Gateway / SDK / app-server protocol。
9. CodexAgent / ClaudeAgent 只作为设计目标和 `AgentKind` reserved extension，除非实现完整 runtime，否则不暴露半成品 API。
10. 不把所有 Agent 类型无差别放入最核心 crate；先放在 orchestration/agent 边界，确认跨模块复用后再提升。

## 暂缓问题

以下问题不进入首版实现：

1. 更完整的分层 API 标准化和小框架抽取，见 `refactor-033-agent-framework-api-standardization`。
2. backend continuation crash recovery 持久化，见 `refactor-033-agent-framework-api-standardization` 的 run lifecycle 子能力。
3. 跨进程 run 恢复，见 `refactor-033-agent-framework-api-standardization` 的 run lifecycle 子能力。
4. subagent / handoff / A2A protocol。
5. CodexAgent / ClaudeAgent 完整 runtime 接入。
6. Agent manager 同层扩展。
7. `compact_session` 是否提升为通用 Harness capability。

注意：这里的暂缓项不包括现有 Service timeline / live recovery。`get_active_turn`、`recover_timeline`、timeline subscription 和客户端增量渲染恢复仍属于 032 当前实现需要保持的产品能力。

## 审查清单

实现前必须确认：

1. Coordinator 不直接依赖 concrete runtime。
2. Harness 不写业务 DB。
3. Runtime 私有 thread/session/event 不进入 Gateway / SDK / schema。
4. 跨层 API 只使用 `session_id`，不使用 `service_session_id`。
5. `workspace_id` 没有被用作 Agent selection key。
6. Harness 和 Runtime 共享核心数据结构，没有重复 DTO 家族。
7. 不支持的 capability 显式失败。
8. event stream 是强类型 tagged union。
9. 新增 trait 有 doc comment，异步接口有 `Send` future contract。
10. 没有为了未来 Codex / Claude 留下不可达半成品 API。
11. 现有 timeline first event、terminal event、pending dynamic action、metrics 语义没有被破坏。
