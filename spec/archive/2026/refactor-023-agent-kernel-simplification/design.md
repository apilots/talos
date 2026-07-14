# Agent Kernel 分层简化设计

Status: archived
Archived: 2026-07-14
Owner: agent
Created: 2026-05-21
Updated: 2026-05-24

## 目标

本次重构只解决一个核心问题：**Talos Agent 执行链路的职责边界混乱，存在多套重复 loop 和 stop decision**。

目标分层固定为：

```text
application-service
        |
        v
orchestration/agent  (AgentHarness)
        |
        v
agent-kernel/general_turn
```

其中：

- `application-service` 是 HTTP / JSON-RPC / Desktop / SDK 等多协议共享实现层。
- `orchestration/agent` 就是 AgentHarness 层，不再额外新增一套平行 Harness 子系统。
- `agent-kernel/general_turn` 是唯一拥有 model/tool loop 和 turn stop decision 的层。

本 spec 的重点不是扩大协议、stream、SDK、active turn 大重构，而是先把当前重复执行路径收掉。

## 当前实现状态

截至 2026-05-22，本 spec 描述的主路径收敛已经落地：

- `conf/talos.yaml` 和 Desktop bundled `talos-app/desktop/resources/conf/talos.yaml` 均只保留当前 kernel 配置；旧 `planner_selection`、`execution_mode`、`execution_children`、`execution_loop_iters`、`max_iters`、`max_task_per_turn`、`max_llm_calls` 已删除。
- `orchestration/agent` 作为 AgentHarness，主路径直接调用 `agent-kernel/general_turn`。
- `KernelRuntime`、`DefaultRunner`、`LlmKernelAgent`、`FlowFactory`、`SingleFlow`、composite kernel agents 已从 Rust 主路径删除。
- 交互式工具审批路径已删除；当前等待态只保留 dynamic tool result / user input。
- `planner_mode` 仅保留 `BuiltIn`；`PlanReAct` 显式模式、policy、planner、配置字段和相关测试已删除。

## 原始代码现状（Historical）

以下内容记录重构前的根因，不再代表当前实现。

重构前代码已经大致有三层目录：

| 目录 | 当前职责 | 主要问题 |
| --- | --- | --- |
| `talos-rs/crates/orchestration/application-service` | `TurnService` 负责 message submit、idempotency、event persistence、active turn recovery，并通过 `Gateway` 调下游。 | 基本符合多协议共享实现层定位，但仍依赖下游 stream 事件来完成 turn 表达。 |
| `talos-rs/crates/orchestration/agent` | `AgentImpl` 实现 `AgentOrchestrator`，负责 active turn queue、unary/stream 执行、timeline adapter、kernel agent 构建。 | 这里实际已经是 AgentHarness，但职责过宽，混入了 kernel agent factory、execution mode、stream projection 等逻辑。 |
| `talos-rs/crates/orchestration/agent-kernel` | 包含 `KernelRuntime`、`DefaultRunner`、`LlmKernelAgent`、`FlowFactory`、`SingleFlow`、`general_turn`、composite kernel agents。 | 重复执行器最多：runner loop、kernel agent run、flow loop、general_turn loop、composite loop 同时存在。 |

重构前默认/配置使用情况：

- `conf/talos.yaml` 使用 `planner_mode: "built_in"`、`execution_mode: "single"`。
- `AgentKernelConfig::default()` 中 `planner_mode` 默认是 `BuiltIn`，`execution_mode` 默认是 `Single`。
- `ExecutionMode::{Sequential, Parallel, Loop}` 暂未看到稳定产品主路径使用，主要是早期参考 ADK 的泛化 runtime 设计。
- 旧 `planner_selection` / ReAct selector 路径已删除，当前不再存在动态选择 planner 的运行时入口。

重构前主要调用链是：

```text
TurnService
  -> Gateway
  -> Coordinator
  -> AgentImpl::run / AgentImpl::run_stream
  -> KernelRuntime::run_turn
  -> DefaultRunner::run_turn
  -> LlmKernelAgent::run
  -> FlowFactory / SingleFlow::run_events
  -> general_turn prepare / step / tool_batch / finalize
```

这条链路的问题是：中间每一层都或多或少参与了执行生命周期。

## 根因

当前混乱不是因为缺一个新抽象，而是因为旧抽象没有收边界：

1. `orchestration/agent::AgentImpl` 同时做 queue、active turn、timeline adapter、kernel agent 构建、unary/stream 分支。
2. `agent-kernel::KernelRuntime` 和 `DefaultRunner` 又包了一层 agent iteration loop。
3. `agent-kernel::Agent` trait 让 `LlmKernelAgent`、`SequentialKernelAgent`、`ParallelKernelAgent`、`LoopKernelAgent` 都可以自己决定执行生命周期。
4. `SingleFlow::run_events` 内部已经复制了一套与 `general_turn::execute_general_turn_loop` 相近的 model/tool loop。
5. 旧 planner selector 与 response processor 逻辑曾散在 flow/processor/kernel state 中，造成 finalization 边界不清。
6. response processor 和 stream adapter 仍参与 final/phase 投影，容易形成上层重复判断。

因此本次重构要做的是：**保留三层目录，但让每层只做一类事；删除或降级无用老代码，不再维护多套 loop。**

## 非目标

第一阶段不做以下事情：

- 不重写 public stream 协议；
- 不重写 SDK schema；
- 不重写完整 active turn snapshot；
- 不新增平行 `AgentHarness` crate；
- 不复制 `pi` 的 API；
- 不把 composite agent lifecycle 继续留在 kernel 主路径。

交互式命令审批暂未形成可用产品能力，本次不做兼容保留。工具是否可执行先由现有配置策略决定；未来如果需要风险命令拦截，另起设计，不进入本次 agent loop 简化。

## 核心决策

### 1. `orchestration/agent` 就是 AgentHarness

不新增第四层。

`orchestration/agent` 的目标职责：

- 实现 `AgentOrchestrator`；
- 接收 coordinator/application-service 下发的 `OrchestratorRequest`；
- 管理 session active turn queue、cancel、resume 调用；
- 把 Talos session/config/input 转成 kernel turn input；
- 把 kernel events/outcome 转成 timeline events；
- 调用 `agent-kernel/general_turn`。

它不再负责：

- 创建 `SequentialKernelAgent` / `ParallelKernelAgent` / `LoopKernelAgent`；
- 选择一套独立 execution mode 来包 kernel；
- 自己判断 final answer；
- 自己实现 model/tool while loop；
- 让 unary 和 stream 各自走不同 kernel 生命周期。

### 2. `agent-kernel/general_turn` 是唯一 loop

Kernel 内部只保留一条主执行路径：

```text
run_general_turn
  -> prepare_general_turn
  -> execute_general_turn_loop
       -> model step
       -> classify output
       -> execute tool batch
       -> stop / continue / waiting
  -> finalize_general_turn_artifacts
```

其他旧路径要删除或降级：

| 当前代码 | 目标处理 |
| --- | --- |
| `KernelRuntime` / `DefaultRunner` | 短期可作为兼容 facade，最终不再拥有 iteration loop。 |
| `agent-kernel::Agent` trait | 不再作为 kernel 主执行抽象；保留前先降级为兼容接口。 |
| `LlmKernelAgent` | 拆掉 executor 责任，能力下沉到 `general_turn` input/adapters。 |
| `FlowFactory` / `SingleFlow` | 删除重复 loop；只保留仍有价值的 request/response processor 能力，或迁入 `general_turn`。 |
| `SequentialKernelAgent` / `ParallelKernelAgent` / `LoopKernelAgent` / `RoutingKernelAgent` | 从 kernel 主路径移除；若有多 agent 编排需求，放到 orchestration 层或作为 tool-like child agent。 |

### 3. Stop decision 只在 kernel

唯一允许判断 turn 是否结束的位置是 `general_turn`。

规则收敛为：

1. provider 明确 final：结束。
2. 本轮模型没有结构化 tool call：可见 assistant text 作为 final answer，结束。
3. 本轮模型有 tool call：执行 tool batch，把 tool result 追加回上下文，继续下一轮。
4. 工具需要 dynamic input：kernel 返回 waiting outcome，暂停。
5. 达到 model/tool limit：按 kernel 配置返回 limit 或 forced finalization。
6. cancel/error：返回 cancelled/failed。

`application-service`、`orchestration/agent`、provider adapter、timeline adapter、客户端都不能再新增一套“看起来结束了”的判断。

Stop / waiting 必须强类型表达，不能再用 `EndTurn + "Tool confirmation pending."` 表示等待：

```rust
pub enum StopReason {
    EndTurn,
    ModelOutputLimit,
    ToolRoundLimit,
    WaitingDynamicInput,
    Cancelled,
    Failed,
}
```

本次不保留命令审批等待态；`WaitingDynamicInput` 是当前唯一进入 kernel waiting outcome 的用户补充输入场景。

### 4. BuiltIn 是唯一 planner 主路径

当前重构以 BuiltIn LLM agent 主路径为基准。旧 ReAct planner strategy、selector、policy 和相关配置已从实现中删除；`planner_mode` 仅保留单值 `built_in`，作为配置治理边界暂时保留。

当前 planner mode 只能影响：

- prompt sections；
- planner instruction；
- response parsing policy；
- tool exposure；
- optional nudge/replan policy。

它们不能拥有：

- 独立 model loop；
- 独立 tool loop；
- 独立 final answer extraction；
- 独立 stream phase 判断。

`planner_selection=auto` 和 selector 代码已删除，不再存在隐式或显式选中 ReAct planner 的路径。在 `general_turn` 唯一化之后，不应重新引入第二套 planner lifecycle。

### 5. ExecutionMode 不是当前主路径能力

`ExecutionMode::{Sequential, Parallel, Loop}` 属于早期 ADK 风格泛化执行框架。当前产品主路径是 LLM agent 的 single execution。

处理原则：

- `Single` 是唯一主路径。
- `Sequential` / `Parallel` / `Loop` 不再作为 kernel 主路径 execution mode。
- 如果确认没有真实产品调用，删除配置、factory 和 composite kernel agents。
- 如果未来需要多 agent 编排，重新在 orchestration 层设计，不继承 kernel generic lifecycle。

## 目标结构

```text
HTTP / JSON-RPC / Desktop / SDK / CLI / TUI / WebUI
        |
        v
+-----------------------------+
| application-service         |
|-----------------------------|
| TurnService                 |
| EventService                |
| SessionHistoryService       |
| idempotency / recovery      |
| public protocol behavior    |
+-----------------------------+
        |
        v
+-----------------------------+
| orchestration/agent         |
| AgentHarness role           |
|-----------------------------|
| AgentImpl / AgentOrchestrator|
| active turn queue           |
| cancel / resume dispatch    |
| kernel input assembly       |
| timeline adapter            |
| memory flush coordination   |
+-----------------------------+
        |
        v
+-----------------------------+
| agent-kernel/general_turn   |
|-----------------------------|
| prompt preparation          |
| model/tool loop             |
| planner step policy         |
| tool batch execution        |
| finalization / stop reason  |
| kernel events / result      |
+-----------------------------+
        |
        v
LlmProvider / ToolRegistry / MemoryStore / PromptBuilder
```

## 模块接口清单

| 层 | 对上接口 | 对下接口 | 不允许做的事 |
| --- | --- | --- | --- |
| application-service | `TurnService::submit_message`、`submit_dynamic_tool_result`、event/history APIs | `Gateway::handle_request_stream` | 不运行 model/tool loop；不判断 final answer；不直接调用 provider/tool。 |
| coordinator/gateway | `Gateway` / `RequestHandler` | `AgentOrchestrator` | 不实现 agent execution；只做 session resolve、route、转发。 |
| orchestration/agent | `AgentOrchestrator::run/run_stream` | `agent-kernel::run_general_turn` 或兼容 facade | 不构建 composite kernel agent；不实现 model/tool loop；不做 stop decision。 |
| agent-kernel/general_turn | kernel turn API | `LlmProvider`、`ToolRegistry`、`MemoryStore`、`PromptBuilder` | 不引用 public timeline envelope；不持久化 application event；不管理 HTTP/SDK 语义。 |

## 目标执行流

```text
submit message
  |
  v
application-service / TurnService
  - validate input
  - idempotency
  - active turn conflict check
  - call Gateway stream
  - persist timeline events
  |
  v
coordinator
  - resolve session
  - route agent
  - call AgentOrchestrator
  |
  v
orchestration/agent as AgentHarness
  - reserve/queue active turn
  - build kernel input
  - attach stream event sink
  - call agent-kernel/general_turn
  - map kernel events to timeline
  |
  v
agent-kernel/general_turn
  - prepare prompt/context
  - run only model/tool loop
  - produce kernel result and stop reason
```

## Kernel 核心 API 方向

不要求第一步就一次性重命名所有现有类型，但目标 API 应接近：

```rust
pub async fn run_general_turn(
    input: GeneralTurnInput,
    adapters: GeneralTurnAdapters,
) -> TalosResult<GeneralTurnOutcome>;

pub struct GeneralTurnInput {
    pub session_id: String,
    pub message_id: String,
    pub invocation_id: String,
    pub agent_id: String,
    pub user_input: String,
    pub selected_model: ModelInfo,
    pub planner_mode: PlannerMode,
    pub kernel_config: AgentKernelConfig,
    pub resume: Option<GeneralTurnResume>,
}

pub enum GeneralTurnResume {
    DynamicToolResult(DynamicToolResultResume),
}

pub struct DynamicToolResultResume {
    pub call_id: String,
    pub tool_name: String,
    pub output: String,
    pub success: bool,
    pub original_call: Option<ResumeToolCall>,
    pub replay_dynamic_tool_results: HashMap<String, DynamicToolResult>,
}

pub struct ResumeToolCall {
    pub item_id: Option<String>,
    pub call_id: String,
    pub tool_name: String,
    pub arguments: TalosJsonValue,
}

pub struct GeneralTurnAdapters {
    pub llm_provider: Arc<dyn LlmProvider>,
    pub tool_registry: Arc<dyn ToolExecutorPort>,
    pub memory_store: Arc<MemoryStore>,
    pub stream_tx: Option<mpsc::Sender<KernelStreamEvent>>,
}

pub enum GeneralTurnOutcome {
    Completed {
        output_text: String,
        output_items: Vec<TalosOutputItem>,
        stop_reason: StopReason,
        usage: TalosUsage,
    },
    Waiting {
        output_items: Vec<TalosOutputItem>,
        pending_action: PendingUserAction,
        stop_reason: StopReason,
        usage: TalosUsage,
    },
    Failed {
        error: KernelError,
        stop_reason: StopReason,
    },
    Cancelled {
        stop_reason: StopReason,
    },
}
```

`RunRequest`、`KernelOutput`、`RunResponse` 可以短期作为兼容类型存在，但不能继续推动一套 `Runner -> Agent::run -> Flow` 生命周期。

Resume/replay 是删除 `SingleFlow` 前的硬前置条件。当前实现已由 `general_turn` 接管 dynamic tool result replay，并由 AgentHarness 从 transcript 恢复原始 dynamic request arguments：

```text
resume request
  |
  v
GeneralTurnInput.resume
  |
  +-- DynamicToolResult
        -> append supplied dynamic result
        -> continue model loop
```

`GeneralTurnResume::DynamicToolResult` 已落地后，`SingleFlow` replay 分支已随旧 flow 删除。

## GeneralTurnPolicy 边界

`SingleFlow` 中的 request/response processor、callback、旧 planner parser、replay injection 有价值，但不能继续拥有 loop。迁移后统一收敛为 policy/adapters：

```rust
pub trait GeneralTurnPolicy: Send + Sync {
    async fn before_step(
        &self,
        ctx: &mut GeneralTurnPolicyContext,
        request: &mut GeneralTurnStepRequest,
    ) -> TalosResult<PolicyDecision>;

    async fn after_step(
        &self,
        ctx: &mut GeneralTurnPolicyContext,
        response: &mut GeneralTurnStepResult,
    ) -> TalosResult<PolicyDecision>;

    async fn before_tool_batch(
        &self,
        ctx: &mut GeneralTurnPolicyContext,
        batch: &mut GeneralTurnToolBatchRequest,
    ) -> TalosResult<PolicyDecision>;

    async fn after_tool_batch(
        &self,
        ctx: &mut GeneralTurnPolicyContext,
        batch: &mut GeneralTurnToolBatch,
    ) -> TalosResult<PolicyDecision>;
}

pub enum PolicyDecision {
    Continue,
    StopBeforeModel,
    StopAfterModel,
}
```

Policy 允许做：

- 修改 prompt / step request；
- 注入 planner instruction；
- 解析 provider-neutral response；
- 设置 state delta；
- 执行 resume replay 注入；
- 触发 callback stop。

Policy 禁止做：

- 自己启动 model loop；
- 自己执行 tool loop；
- 自己判断 final answer phase；
- 自己创建 public timeline envelope；
- 在 kernel 外层追加下一次 model call。

## Loop 伪代码

```rust
pub async fn execute_general_turn_loop(
    prepared: PreparedGeneralTurn,
    adapters: GeneralTurnAdapters,
) -> TalosResult<ExecutedTurnArtifacts> {
    let mut state = GeneralTurnLoopState::new(&prepared);

    loop {
        if state.tool_rounds_exceeded() {
            return finalize_limit(state).await;
        }

        if let Some(replay) = state.take_resume_replay() {
            let batch = execute_resume_replay(&prepared, replay, &adapters).await?;
            state.apply_tool_batch(batch);
            continue;
        }

        let step = execute_general_turn_step(&prepared, &state, &adapters).await?;

        match step.disposition {
            GeneralTurnStepDisposition::FinalResponse => {
                return finalize_final_response(state, step).await;
            }

            GeneralTurnStepDisposition::ToolCalls => {
                let batch = execute_general_turn_tool_batch(&prepared, &step, &adapters).await?;

                if batch.has_pending_user_action() {
                    return finalize_waiting(state, batch).await;
                }

                state.apply_tool_batch(batch);
                continue;
            }
        }
    }
}
```

关键要求：

- BuiltIn 是唯一实现和验证的 planner 主路径。
- 旧 ReAct planner strategy 已删除，不作为兼容目标。
- resume/replay 必须在 loop 内部执行，不能依赖 `SingleFlow` 外层分支。
- forced finalization 在 loop 内部执行。
- dynamic input 在 loop 内部转成 waiting outcome。
- 上层只能消费 outcome，不能自行继续模型调用。

## Waiting Outcome 映射

`GeneralTurnOutcome::Waiting` 是 kernel 到上层的唯一等待表达。

| Kernel outcome | StopReason | AgentHarness timeline mapping | application-service active turn |
| --- | --- | --- | --- |
| `Waiting { pending_action: DynamicToolResult }` | `WaitingDynamicInput` | emit pending dynamic tool item；complete turn as waiting, not final answer | `WaitingUserAction::DynamicToolResult` |
| `Completed` | `EndTurn` / limit source | emit final answer / completed items | `Completed` |
| `Failed` | `Failed` | fail open items and turn | `Failed` |
| `Cancelled` | `Cancelled` | cancel open items and turn | `Completed` or `Cancelling -> Completed` per existing store semantics |

waiting turn 不产生 final answer。application-service 和 timeline adapter 不允许再靠文本或普通 `EndTurn` 推断 pending user action。

## `orchestration/agent` 收敛方式

当前 `AgentImpl` 不需要被推倒，但要拆清职责。

保留：

- `AgentOrchestrator` trait；
- `AgentImpl::run` / `run_stream` 对外入口；
- per-session active turn queue；
- cancel / drain / resume dispatch；
- timeline builder/adapter；
- memory flush coordination。

删除或迁移：

- `KernelExecutionMode::{Single, Sequential, Parallel, Loop}` 主路径；
- `build_kernel_agent_for_request` 中创建 composite kernel agents 的逻辑；
- `ExecutionMode::{Sequential, Parallel, Loop}` 对 kernel 主路径的影响；
- unary/stream 两套不同 kernel 生命周期；
- Agent 层对 stop reason / final answer 的推断；
- Agent 层对 provider phase 的兜底修正。

目标调用应变为：

```text
AgentImpl::run_stream
  -> reserve active turn
  -> build GeneralTurnInput
  -> build GeneralTurnAdapters
  -> spawn general_turn
  -> map KernelStreamEvent / GeneralTurnOutcome to TalosTimelineEvent
```

unary `run` 可以复用同一条内部执行路径，只是不订阅 timeline stream 或只汇总 outcome。

## `agent-kernel` 清理方式

### 第一类：已删除旧 facade / wrapper

这些类型不再作为当前 Rust 主路径存在：

- `KernelRuntime`
- `KernelBuilder`
- `DefaultRunner`
- `RunRequest`
- `RunResponse`
- `KernelOutput`

目标是它们直接调用 `general_turn`，不再通过 `Arc<dyn Agent>` 和 `Agent::run` 间接执行。

### 第二类：迁移后删除

这些类型不应继续在主路径存在：

- `LlmKernelAgent` 作为 executor；
- `FlowFactory` 作为 executor factory；
- `SingleFlow::run_events` 中的重复 model/tool loop；
- `AutoFlow` 中的额外 flow 包装；
- `SequentialKernelAgent`；
- `ParallelKernelAgent`；
- `LoopKernelAgent`；
- `RoutingKernelAgent`。

如果某些测试仍依赖这些类型，应迁移到：

- `general_turn` 单元测试；
- `orchestration/agent` harness 测试；
- 后续 child-agent-as-tool 编排测试。

### 第三类：保留并迁入 `general_turn`

这些能力本身有价值，但不能作为第二套 loop：

- request processor 中的 prompt/planner effect；
- response processor 中的 provider-neutral parsing；
- 旧 planner final tag parsing 已删除，不作为主路径能力保留；
- resume replay injection；
- compaction / memory summary；
- tool callback hooks；
- prompt warning；
- finalization guard；
- tool loop guard。

迁入原则：它们只能影响 `general_turn` 的输入、step policy 或事件，不拥有 turn lifecycle。

## Composite / Child Agent

本次不继续支持 kernel 层 generic composite lifecycle。

目标原则：

- 多 agent 编排属于 orchestration 层；
- child agent 可作为 tool-like callable；
- child agent 内部仍走 `orchestration/agent -> agent-kernel/general_turn`；
- kernel 不维护 sequential/parallel/loop/routing agent 生命周期。

```text
parent general_turn
  |
  +-- tool call: child_agent
        |
        v
     orchestration/agent
        |
        v
     child general_turn
```

## 分阶段计划

### Phase 1: 现状盘点与行为锁定

目标：先确认重复实现点和必须保留的行为。

成功标准：

- 列出 `AgentImpl`、`KernelRuntime`、`DefaultRunner`、`LlmKernelAgent`、`SingleFlow`、`general_turn`、composite agents 中的 loop/stop 判断点；
- 增加或调整测试覆盖现有核心行为；
- 明确哪些 public 类型短期保留为兼容 facade。
- 确认 `ExecutionMode::{Sequential, Parallel, Loop}` 是否有真实产品调用；没有则标记为待删除。
- 删除 `planner_selection=auto`、selector 和旧 ReAct planner 路径。
- 定义并测试 `GeneralTurnResume` / `GeneralTurnOutcome::Waiting` 的最小路径。
- 移除交互式命令审批路径；不以 command approval 阻塞主 loop。

测试：

- direct text answer；
- provider explicit final；
- tool call -> tool result -> final answer；
- tool round limit；
- forced finalization；
- pending dynamic input；
- dynamic result resume replay；
- BuiltIn 主路径通过 kernel loop；
- 旧 ReAct planner 路径不可再被配置或动态选择；
- stream 和 unary stop reason 一致。

### Phase 2: 拉直 BuiltIn 主路径

目标：先让 BuiltIn LLM agent 直接走 `orchestration/agent -> general_turn`。

成功标准：

- BuiltIn 不再经过 `Runner -> Agent::run -> FlowFactory/SingleFlow` 生命周期；
- response processor、request processor 不再驱动外层 loop；
- `GeneralTurnPolicy` 接管 processor/callback/replay 能力；
- `GeneralTurnResume` 接管 dynamic replay；
- `execute_general_turn_loop` 负责 continue/stop/waiting/finalization；
- `cargo test -p talos-agent-kernel` 通过。

### Phase 3: 处理旧 planner selector 路径

目标：删除 auto selector 和旧 ReAct planner 路径，避免任何 planner 分支继续要求保留第二套 loop。

成功标准：

- `planner_selection=auto` 和 selector 代码已删除；
- 旧 ReAct planner strategy、policy、parser 和配置入口已删除；
- `require_final_answer_tag`、replan、no-progress 判断不再作为主路径兼容目标；
- 相关旧 planner 测试已明确删除或改写为 BuiltIn 主路径测试。

### Phase 4: 把 `orchestration/agent` 收敛为 AgentHarness

目标：`AgentImpl` 只做 harness 职责。

成功标准：

- `AgentImpl` 不再构建 composite kernel agents；
- `AgentImpl` 不再选择 `KernelExecutionMode` 主路径；
- unary/stream 复用同一 kernel execution path；
- timeline adapter 只映射 kernel event/outcome；
- `cargo test -p talos-agent` 通过。

### Phase 5: 删除无用老代码

目标：清掉不再位于主路径的旧执行器。

成功标准：

- 删除或降级 `SequentialKernelAgent`、`ParallelKernelAgent`、`LoopKernelAgent`、`RoutingKernelAgent`；
- 删除或降级 `LlmKernelAgent` executor 角色；
- 删除 `DefaultRunner` 中的 agent iteration loop，或让它只作为兼容 facade；
- 删除不再使用的 execution mode 配置，或迁移为 orchestration 层后续设计；
- 删除 ADK 风格泛化 runtime 残留。

### Phase 6: 外围表达跟随收敛

目标：在 loop 唯一化后，再处理 stream/resume/schema 的表达差异。

成功标准：

- application-service 不推断 final phase；
- active turn waiting 由 kernel outcome 驱动；
- SDK/schema 只表达稳定后的 outcome；
- 必要时再补充 provider-neutral stream 子设计。

## 验证计划

| 阶段 | 验证命令 |
| --- | --- |
| Phase 1-2 | `cargo test -p talos-agent-kernel` |
| Phase 3 | `cargo test -p talos-agent-kernel`，覆盖 BuiltIn planner 配置与旧 selector 删除后的主路径 |
| Phase 4 | `cargo test -p talos-agent` |
| Phase 5 | `cargo test -p talos-agent-kernel -p talos-agent` |
| Phase 6 | `cargo test -p talos-application-service`、相关 schema/SDK 测试 |
| 服务端回归 | `talos-e2e/run_e2e.sh` |
| WebUI/Desktop 受影响时 | 前端 typecheck/test；Desktop 最终 packaged DMG smoke |

## 完成定义

- `application-service` 是多协议共享实现层，不运行 agent loop。
- `orchestration/agent` 明确作为 AgentHarness，不再额外新增平行 Harness。
- `AgentImpl` 不再构建或选择 composite kernel execution mode。
- `agent-kernel/general_turn` 是唯一 model/tool loop。
- stop/final answer/waiting/limit 只由 `general_turn` 产出。
- BuiltIn 主路径直接走 `general_turn`。
- 旧 ReAct planner selector 和显式策略入口已删除，不能保留独立 loop。
- `GeneralTurnOutcome::Waiting` 强类型表达 `WaitingDynamicInput`。
- `GeneralTurnResume` 支持 dynamic result replay；删除 `SingleFlow` 前恢复路径不能断。
- processor/callback 只通过 `GeneralTurnPolicy` 影响 step，不拥有 loop。
- 交互式命令审批路径已移除，不作为兼容目标。
- kernel 主路径不再依赖 `SequentialKernelAgent`、`ParallelKernelAgent`、`LoopKernelAgent`、`RoutingKernelAgent`。
- `KernelRuntime` / `DefaultRunner` 已从当前 Rust 主路径删除。
- 删除无用老代码和不再使用的 tests/config 分支。

## 风险

1. **旧测试依赖 composite kernel agents**

   需要先确认是否只是单元测试遗留。如果没有产品路径依赖，应迁移测试后删除。

2. **旧 planner selector 不能回归为第二套 loop**

   `planner_selection=auto` 和旧 ReAct planner 已删除；后续风险是重新引入 planner 分支时绕开 `general_turn` 唯一 loop。

3. **stream timeline adapter 目前在 `AgentImpl` 内较重**

   第一阶段不重写协议，但要避免它继续承担 stop decision。

4. **resume/replay 若回归会直接断 dynamic user action**

   该路径已接入 `general_turn`，后续改动必须保持 dynamic result resume 集成测试。

5. **Desktop / packaged resource 可能重新带入旧配置**

   `conf` 与 Desktop bundled config 必须同步保持无旧字段；`AgentKernelConfig` 使用 `deny_unknown_fields`，旧字段会导致 packaged app-server 启动失败。

## 决策摘要

- 不新增独立 `AgentHarness` 层；`orchestration/agent` 就是 AgentHarness。
- 下游固定为 `orchestration/agent -> agent-kernel/general_turn`。
- `agent-kernel/general_turn` 是唯一 loop。
- 当前主路径已收敛为 `AgentHarness -> general_turn`。
- composite kernel agents 已从 kernel 主路径移除。
- BuiltIn 是当前唯一 planner 主路径。
- 旧 ReAct planner strategy、selector 和配置入口已删除。
- `ExecutionMode::{Sequential, Parallel, Loop}` 属于 ADK 风格历史泛化设计，已从 kernel 配置和主路径删除。
- waiting/resume 是硬边界：`GeneralTurnOutcome::WaitingDynamicInput` 和 `GeneralTurnResume::DynamicToolResult` 已落地。
- processor/callback/replay 统一收敛到 `GeneralTurnPolicy`，只影响 step，不拥有 loop。
- 交互式命令审批路径暂时移除，不考虑兼容。
- 本次优先清职责和删重复实现，不先扩大成 stream/schema/active turn 大重构。
- 重要：执行过程中，无用代码一定要及时清理，这部分改造能删除大量代码，而不是一直保留，最后污染了项目
