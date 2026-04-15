# Talos 多 Agent / A2A 方案设计

## 1. 背景

当前 Talos 已具备以下能力基础：

- 统一的 `general_turn` 主执行路径
- `PlannerKind::BuiltIn | PlanReAct`
- `ToolRegistry` / `ToolExecutorPort` / `ToolDefinitionProvider`
- `transfer_to` 类型化控制动作
- `CollaborationRuntime` 与 `spawn_agent / send_input / wait_agent`
- dynamic tool 的 pending / replay / timeout / transcript 提交能力

同时，参考仓库 `~/data/ailabs/adk-python` 已形成一套较成熟的分层模型：

- `BaseAgent` 负责 agent tree、`parent_agent`、`sub_agents`、查找与唯一命名
- `LlmAgent` 负责 reasoning、tools、transfer
- `SequentialAgent` / `ParallelAgent` / `LoopAgent` 只负责编排
- `AgentTool` 把 agent 暴露为 tool
- `RemoteA2aAgent` / A2A executor 负责远程协议适配

另外，参考仓库 `~/data/ailabs/claude-code`，其 `AgentTool` / subagent runtime 也提供了另一组对 Talos 很有价值的实现经验：

- agent 通过显式 definition/profile 决定 prompt、model、tools、permission、hooks、memory、isolation
- fresh subagent 与 fork-like subagent 是两种不同的子调用语义
- 子 agent 默认使用隔离上下文，只显式继承必要能力
- 子 agent transcript、metadata、resume 都是第一等运行时对象
- background subagent 通过任务状态、进度更新、完成通知回流主线程

因此，本文后续设计按以下方式收敛：

- **控制面**参考 ADK：`AgentGraph`、workflow agent、`Transfer`、`RemoteA2A`
- **`LlmAgent` 内部执行面**参考 Claude Code：agent profile、子上下文隔离、tool 过滤、transcript/resume、fresh/fork-like 子调用

---

## 2. 设计目标

### 2.1 目标

1. 为 Talos 建立统一、强类型的多 agent 模型
2. 保持 `general_turn` 为唯一主执行路径
3. 让 `LlmAgent` 成为核心原子执行单元
4. 明确区分本地 transfer、本地 agent-as-tool、异步 child session、远程 A2A 四种能力
5. 复用现有 `MemoryEngine`、`WorkspaceMemory`、`LlmProvider`、`ToolRegistry`、`agent-kernel runtime`
6. 为后续实现提供可渐进落地的接口草案与测试边界

### 2.2 非目标

1. 不新增独立 intent classifier
2. 不引入基于 prompt 前缀的规则路由
3. 不新增绕过 `general_turn` 的第二条 LLM 主执行链
4. 不把 A2A 直接做成新的核心执行模式
5. 不使用弱类型 metadata JSON 作为核心编排协议

---

## 3. 核心判断

### 3.1 控制面参考 ADK 的是什么

应借鉴 ADK 的**控制面分层边界**，而不是直接照搬其 runner/session 结构：

- `BaseAgent` / agent tree 的关系建模
- `LlmAgent` 与 workflow agent 的职责拆分
- `transfer_to_agent` 作为一种受约束的特殊 tool
- `AgentTool` 作为“agent 以 tool 形式调用”的语义
- `RemoteA2aAgent` 作为远程 adapter，而非核心运行时

### 3.2 `LlmAgent` 内部参考 Claude Code 的是什么

应借鉴 Claude Code 的**子 agent 运行时设计**：

- 用显式 `AgentProfile` 决定每个 agent 的 prompt、model、tools、permission、skills、memory、isolation
- 区分 **fresh child** 与 **fork-like child** 两种调用方式
- 构造隔离的 child invocation context，而不是默认共享父 agent 全部可变状态
- 在 agent 运行时层面做 tool pool 过滤，而不是只在 prompt 上描述“哪些工具应该用”
- 把 transcript、run metadata、resume 都做成显式运行时对象
- 把长任务的进度和完成结果走统一任务/恢复链路，而不是让主 agent 轮询猜测

### 3.3 Talos 不应照搬什么

- 不应再造一套 ADK 风格的独立 runner
- 不应让 A2A 侵入本地 agent 编排语义
- 不应把 `spawn_agent` 低层原语直接等同于全部 subagent 机制
- 不应把 workflow agent、transfer、agent tool、remote agent 混成同一种“调用”
- 不应照搬 Claude Code 的 `task-notification` XML、mailbox、tmux/swarm backend 等 CLI 产品语义
- 不应让 `LlmAgent` 内部优化反过来主导控制面建模

### 3.4 Talos 应收敛出的 4 类语义

Talos 后续多 agent 机制应显式区分：

1. `Transfer`
   同一 session 内，将当前 turn 的主执行权切换到另一个 agent
2. `InvokeAsTool`
   把另一个 agent 当作特殊 tool 调用，并将结果返回当前 agent
3. `SpawnSession`
   创建独立 child session，允许异步等待、恢复、关闭
4. `RemoteA2A`
   远程 agent 的传输与协议适配层，可被 `Transfer` 或 `InvokeAsTool` 复用

### 3.5 ADK 的 Sequential / Parallel / Loop 职责

ADK 中的 `SequentialAgent`、`ParallelAgent`、`LoopAgent` 本质上都不是新的 LLM 执行内核，而是 **workflow shell agent**：

- `SequentialAgent`
  顺序调度多个 `sub_agents`，并维护 `current_sub_agent` 这类恢复状态
- `ParallelAgent`
  为每个 `sub_agent` 创建隔离 branch / context，并行执行后合并事件流
- `LoopAgent`
  循环执行一组 `sub_agents`，直到 `escalate` 或达到 `max_iterations`

这些 agent 的共同点是：

- 自己不承担核心 reasoning
- 自己不直接定义 planner 语义
- 自己不绕开统一 runner 主路径
- 依赖 `BaseAgent` 的 agent tree、父子关系和恢复状态机制

因此，ADK 中这三类类型的核心职责是：**编排其他 agent，而不是替代 `LlmAgent`。**

### 3.6 Talos 当前与 ADK 的一致点和不一致点

**一致点**

- Talos 当前的 `SequentialKernelAgent`、`ParallelKernelAgent`、`LoopKernelAgent` 也都属于编排层，而不是 reasoning 层
- 它们同样不直接构造 planner，也不直接持有 tool calling 主循环

**不一致点**

1. **Talos 当前是 execution mode，不是 agent graph node**

   当前 `Sequential | Parallel | Loop` 主要通过 `kernel.execution_mode` 触发，并在请求到达时临时包装一组 child agent 执行。

2. **Talos 当前 child agent 主要是同质 LLM child**

   当前实现会基于同一个 `AgentImpl` 在运行时批量构造多个 `LlmKernelAgent` child，而不是显式编排一棵有身份、有关系的 agent tree。

3. **Talos 当前缺少 workflow 级恢复状态**

   ADK 的 workflow agent 会保存 `current_sub_agent`、`times_looped` 等状态；Talos 当前 composite agent 主要在一次 `run()` 中聚合结果，没有形成完整的 workflow state 模型。

4. **Talos 当前 loop 语义偏执行控制，不是 workflow 语义**

   Talos `LoopKernelAgent` 主要依赖 child 返回的 `continue_loop` / `requested_transfer`；ADK `LoopAgent` 的 loop 终止条件则是 workflow 层显式约束。

### 3.7 结论

Talos 与 ADK 在这一层**高层职责一致**，即：

- `Sequential / Parallel / Loop` 都应该只是编排层
- `LlmAgent` 才是原子 reasoning 单元

但 Talos 当前**落地形态并不一致**：

- ADK：workflow agent 是 agent tree 中的一等 agent 类型
- Talos：workflow 仍更接近 kernel execution mode wrapper

因此，Talos 后续如果要向 ADK 的多 agent 设计对齐，正确方向不是继续扩展 `execution_mode`，而是：

```text
从：
  execution_mode = Sequential | Parallel | Loop
过渡到：
  AgentNodeKind::SequentialWorkflow | ParallelWorkflow | LoopWorkflow
```

`execution_mode` 后续应只保留为兼容期入口，长期目标应是由 `AgentGraph` 驱动 workflow agent 编排。

---

## 4. 总体架构

### 4.1 分层图

```text
                          ┌─────────────────────────────┐
                          │         AgentGraph          │
                          │  node / edge / relation     │
                          └──────────────┬──────────────┘
                                         │
                 ┌───────────────────────┼────────────────────────┐
                 │                       │                        │
                 ▼                       ▼                        ▼
      ┌──────────────────┐   ┌──────────────────┐    ┌────────────────────┐
      │     LlmAgent      │   │  WorkflowAgents   │    │  RemoteA2AAdapter  │
      │ general_turn core │   │ seq/par/loop only │    │ transport adapter  │
      └─────────┬────────┘   └─────────┬────────┘    └──────────┬─────────┘
                │                      │                          │
                └──────────────┬───────┴──────────────┬──────────┘
                               │                      │
                               ▼                      ▼
                      ┌────────────────┐    ┌────────────────────┐
                      │ Delegation API │    │ Session Collaboration│
                      │ transfer/invoke│    │ spawn/wait/close     │
                      └───────┬────────┘    └──────────┬──────────┘
                              │                        │
                              ▼                        ▼
                        ┌────────────────────────────────────┐
                        │        general_turn runtime        │
                        │ request -> model -> tools -> stop  │
                        └────────────────────────────────────┘
```

### 4.2 基本原则

- `LlmAgent` 是唯一的 reasoning + tool-calling 原子单元
- workflow agent 不直接持有 LLM 行为，只负责编排多个 agent node
- `Transfer` 是控制动作，不返回 tool result
- `InvokeAsTool` 是工具动作，必须返回 `ToolOutputPayload`
- `SpawnSession` 是异步协作动作，应复用 dynamic tool pending/resume 机制
- `RemoteA2A` 是 adapter，而不是新的 planner / flow / kernel
- `Sequential / Parallel / Loop` 的长期归属是 `AgentGraph` 的 workflow node，而不是 `execution_mode` 包装器

---

## 5. 目标语义模型

### 5.1 Agent 节点类型

```rust
pub enum AgentNodeKind {
    Llm,
    SequentialWorkflow,
    ParallelWorkflow,
    LoopWorkflow,
    RemoteA2A,
}

pub struct AgentNodeId(String);

pub struct AgentNodeDescriptor {
    pub id: AgentNodeId,
    pub name: String,
    pub description: String,
    pub kind: AgentNodeKind,
}
```

### 5.2 关系类型

```rust
pub enum AgentRelationKind {
    ParentChild,
    Peer,
    ToolCallable,
    TransferAllowed,
}

pub struct AgentEdge {
    pub from: AgentNodeId,
    pub to: AgentNodeId,
    pub kind: AgentRelationKind,
}
```

### 5.3 委托类型

```rust
pub enum DelegationMode {
    Transfer,
    InvokeAsTool,
    SpawnSession,
}

pub enum AgentTargetRef {
    Local { node_id: AgentNodeId },
    RemoteA2A { node_id: AgentNodeId },
}
```

### 5.4 子上下文继承策略

```rust
pub struct ChildContextInheritance {
    pub inherit_memory: bool,
    pub inherit_workspace: bool,
    pub inherit_tool_policy: bool,
    pub inherit_plugins: bool,
    pub inherit_prompt_context: bool,
}
```

这里不使用松散 metadata，而是显式声明继承维度，避免隐式语义。

---

## 6. 4 类能力的职责边界

### 6.1 Transfer

**定义**

当前 agent 不再继续回答，直接把主执行权切换给另一个 agent。

**适用场景**

- 专家路由
- 同一问题在多个专家 agent 之间切换
- 用户后续消息应由被转移 agent 接续处理

**不适用**

- 获取一个子任务结果后继续由当前 agent 总结
- 后台异步长任务

**Talos 落点**

- 使用特殊 tool：`transfer_to_agent`
- tool schema 中 `agent_name` 使用 enum 约束可选目标
- tool 执行结果不写普通 `ToolOutputPayload::Function`
- tool 只写 `EventActions.transfer_to`
- `Runner` 或 flow 收到后切换 active agent

### 6.2 InvokeAsTool

**定义**

当前 agent 调用另一个 agent 完成子任务，并把结果当作 tool result 回填到当前 turn。

**适用场景**

- “让分析 agent 先查资料，再由当前 agent 汇总”
- “让 code-review agent 只输出 findings”
- skill / specialist agent 的局部调用

**不适用**

- 需要后续多轮继续由子 agent 保持会话主导权的场景

**Talos 落点**

- 实现 `AgentToolHandler`
- 通过 `ToolOutputPayload::Function` 返回结果
- 当前 turn 不切换 active agent
- 允许本地 agent 和 remote A2A agent 共用此能力

### 6.3 SpawnSession

**定义**

创建独立 child session，允许异步运行、等待、关闭、恢复。

**适用场景**

- 长耗时任务
- 需要单独 transcript / audit trail 的子任务
- 后台协作 worker

**Talos 落点**

- 保留现有 `spawn_agent / send_input / wait_agent / close_agent`
- 但把它明确定位为 session collaboration primitive
- `wait_agent` 不再依赖仅进程内 watch
- 应复用 dynamic tool pending/resume transcript 机制

### 6.4 RemoteA2A

**定义**

远程 agent 的协议 adapter。

**适用场景**

- 远程 agent registry
- 跨服务 agent 调用
- 外部系统通过 A2A 协议暴露 agent

**Talos 落点**

- 本地提供 `RemoteA2aAgentAdapter`
- 出站支持把远端 agent 暴露为可 `Transfer` / 可 `InvokeAsTool` 的目标
- 入站支持 `TalosA2aExecutor`，将 Talos agent 对外暴露为 A2A server

---

## 7. LlmAgent 应如何完善

### 7.1 LlmAgent 的职责

Talos 中的 `LlmAgent` 应只负责：

- prompt 构建
- planner 模式选择
- `general_turn` 调用
- tool execution
- delegation 决策输出

不负责：

- workflow 编排树执行
- child session 生命周期管理
- A2A transport 细节

### 7.2 新增 `LlmAgentProfile`

`LlmAgent` 内部应引入显式 profile，而不是把 prompt/model/tools/permission 规则散在各调用点。

```rust
pub struct PromptTemplateId(String);
pub struct ModelId(String);
pub struct ToolName(String);
pub struct SkillId(String);
pub struct McpBindingId(String);
pub struct TranscriptGroupId(String);
pub struct AgentRunId(String);
pub struct MemoryViewRef(String);

pub enum AgentSystemPromptRef {
    Inline(String),
    Template(PromptTemplateId),
}

pub enum AgentModelPolicy {
    Inherit,
    Fixed(ModelId),
    Allowed(Vec<ModelId>),
}

pub struct EffortLevel(String);
pub struct AgentMemoryScope(String);

pub enum AgentToolPolicy {
    AllowAll,
    Allowlist(Vec<ToolName>),
    DenyBySource(Vec<ToolSource>),
}

pub enum AgentPermissionMode {
    Inherit,
    Default,
    BubbleToParent,
    NoPrompts,
}

pub struct LlmAgentProfile {
    pub system_prompt: AgentSystemPromptRef,
    pub model_policy: AgentModelPolicy,
    pub effort: Option<EffortLevel>,
    pub tool_policy: AgentToolPolicy,
    pub permission_mode: AgentPermissionMode,
    pub memory_scope: AgentMemoryScope,
    pub skill_preloads: Vec<SkillId>,
    pub mcp_bindings: Vec<McpBindingId>,
    pub isolation: AgentIsolationMode,
    pub max_turns: Option<u16>,
}

pub enum AgentIsolationMode {
    None,
    WorkspaceClone,
    RemoteSandbox,
}
```

说明：

- 上述类型是 `LlmAgentProfile` 最小可实现闭包，不作为“TBD 留空项”保留到后续阶段。
- 这相当于把 Claude Code `AgentDefinition` 里的运行时关键信息强类型化。
- `LlmAgentProfile` 由配置和 agent graph 节点解析得到，运行时只消费 profile，不在执行链路临时拼接大量分散参数。
- `AgentSystemPromptRef` 必须区分“内联 prompt”与“模板引用”，否则 `PromptBuilder` 无法判断是直接展开还是按 key 装配。
- `AgentModelPolicy` 必须在 spec 中明确，否则 child agent 是否允许切换 model 会退回实现期临时决策。
- `EffortLevel`、`AgentMemoryScope` 在本文先以 newtype 占位；正式实现时应迁移到对应 typed config / domain 模块。
- `AgentPermissionMode` 是 `LlmAgentProfile` 的必要组成部分；实现时应映射到现有权限/审批配置，而不是在 child context factory 中再临时拼字符串。
- `ToolSource` 这里明确**复用** `talos-rs/crates/action/tools/src/spec.rs` 中现有定义：`Local / Mcp / Dynamic / Collaboration / Skill`，不得在多 agent 方案中再新建一套同名枚举。
- `MemoryViewRef` 在本文先作为最小引用类型占位，表示从 `MemoryEngine` / `WorkspaceMemory` 派生出来的只读内存视图句柄；正式实现时应落为明确的 memory domain 类型，而不是继续留作匿名占位。

### 7.3 子 agent 调用语义：`Fresh` 与 `ForkLike`

`LlmAgent` 内部应显式区分两种子调用模式：

```rust
pub enum ChildInvocationMode {
    Fresh,
    ForkLike,
}

pub enum ChildResumePolicy {
    Disabled,
    TranscriptDriven,
}

pub struct ChildAgentRunOptions {
    pub mode: ChildInvocationMode,
    pub inheritance: ChildContextInheritance,
    pub transcript_group: Option<TranscriptGroupId>,
    pub resume_policy: ChildResumePolicy,
}
```

语义定义：

- `Fresh`
  子 agent 不继承父 agent 当前 turn 的 transcript 前缀，只接收显式 briefing、必要 memory 与受控 tool policy。
- `ForkLike`
  子 agent 继承父 agent 当前 prompt 前缀和必要运行态，适合把长研究、长实现、分支探索从主上下文中摘出去。

约束：

- `Transfer` 不使用 `ForkLike`
- `InvokeAsTool` 默认 `Fresh`
- `SpawnSession` 可按配置支持 `Fresh` 或 `ForkLike`
- `ForkLike` 必须受 `ChildContextInheritance` 和深度限制约束，不能默认继承全部运行时状态

### 7.4 Child invocation context 与 tool 过滤

`LlmAgent` 内部应像 Claude Code 一样，显式构造 child invocation context，而不是直接复用父 context。

```rust
pub struct ChildInvocationContext {
    pub request: AgentRunTurnRequest,
    pub effective_profile: LlmAgentProfile,
    pub effective_tools: Vec<RegisteredToolSpec>,
    pub effective_memory_view: MemoryViewRef,
    pub run_metadata: AgentRunMetadata,
}
```

需要新增一个专门的解析器：

```rust
pub trait AgentExecutionContextFactory: Send + Sync {
    fn build_child_context(
        &self,
        parent: &InvocationContext,
        target: &AgentNodeDescriptor,
        options: &ChildAgentRunOptions,
    ) -> TalosResult<ChildInvocationContext>;
}
```

关键要求：

- child 的 `effective_tools` 必须由 profile、permission、调用模式、运行时能力共同裁剪
- child 的 permission mode 必须显式确定，不能默认继承父 agent 的临时 UI 交互能力
- child 的 memory / workspace / plugins / MCP 必须按 `ChildContextInheritance` 受控透传
- async child、workflow child、agent-as-tool child 可以拥有不同的 tool allowlist
- `AgentToolPolicy` 的求值属于 `AgentExecutionContextFactory::build_child_context` 的核心职责，不应留到 handler 层临时判断
- `effective_tools` 明确复用现有 `RegisteredToolSpec`，来源于 `talos-rs/crates/action/tools/src/spec.rs`；本文不再新建 `ToolSpec`

### 7.5 在 request 阶段注入 delegation 能力

和 ADK 一样，transfer 目标应由 request processor 注入，而不是硬编码在系统 prompt 常量里。

```text
LlmRequestEnvelope
  ├── available_tools
  ├── memory / context
  ├── transfer_targets
  ├── invokable_agent_tools
  └── planning_instruction
```

建议新增：

```rust
pub struct DelegationPromptContext {
    pub transfer_targets: Vec<AgentTransferTarget>,
    pub invokable_agents: Vec<AgentToolTarget>,
}

pub struct AgentTransferTarget {
    pub name: String,
    pub description: String,
}

pub struct AgentToolTarget {
    pub tool_name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}
```

`PromptBuilder` 应把这两类对象纳入 tools/dynamic prompt，而不是只平铺普通工具摘要。

### 7.6 Transcript 与 resume metadata

`LlmAgent` 运行态应补一套最小 metadata 存储，支持 child transcript 记录和 resume。

```rust
pub struct AgentRunMetadata {
    pub run_id: AgentRunId,
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub node_id: AgentNodeId,
    pub invocation_mode: ChildInvocationMode,
    pub description: Option<String>,
    pub isolation: AgentIsolationMode,
    pub workspace_path: Option<PathBuf>,
    pub transcript_group: Option<TranscriptGroupId>,
}

pub struct AgentRunOutput {
    pub status: AgentRunStatus,
    pub output_text: Option<String>,
    pub output_items: Vec<TalosOutputItem>,
}

pub enum AgentRunStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

pub trait AgentRunMetadataStore: Send + Sync {
    fn write(&self, metadata: &AgentRunMetadata) -> TalosResult<()>;
    fn update_completion(
        &self,
        run_id: &AgentRunId,
        output: AgentRunOutput,
    ) -> TalosResult<()>;
    fn read(&self, run_id: &AgentRunId) -> TalosResult<Option<AgentRunMetadata>>;
    fn list_by_session(&self, session_id: &str) -> TalosResult<Vec<AgentRunMetadata>>;
}
```

要求：

- `SpawnSession` 和可恢复的 `InvokeAsTool` 必须记录 metadata
- resume 时优先按 metadata 恢复 workspace / transcript / invocation mode
- 不引入 Claude Code 的 XML 通知格式；Talos 统一走已有 dynamic / transcript 恢复链路
- `write` 只负责初始写入；child 完成、失败、取消都必须通过 `update_completion` 落盘，保证跨 turn 恢复链路有终态可读
- `AgentRunOutput.output_items` 直接复用现有 `TalosOutputItem`，避免把运行时输出再次退回到弱类型 JSON 结构

### 7.7 在 response 阶段输出 typed delegation intent

建议不要让 transfer 继续依赖松散文本或外部推断，而是在工具或 action 层输出强类型结果：

```rust
pub enum DelegationDecision {
    None,
    Transfer { target: AgentNodeId },
}
```

`InvokeAsTool` 则继续通过普通 tool call 返回，不需要单独动作类型。

---

## 8. Workflow Agent 设计

### 8.1 SequentialWorkflowAgent

作用：串行执行多个 child agent。

```text
input
  -> child_1
  -> child_2
  -> child_3
  -> final merged result
```

约束：

- 不直接产生 tool call
- 不直接选择 planner
- 只调度 child agent 执行顺序
- 中间状态通过强类型 state 保存当前 child 索引
- 必须支持 pause / resume，不得退化成“一次 run 内部临时 for-loop”

### 8.2 ParallelWorkflowAgent

作用：并行执行多个 child agent，并进行结果聚合。

约束：

- child agent 运行在隔离 branch / state bag
- 聚合策略必须是强类型配置
- 不允许 workflow agent 自己再做 reasoning
- 必须是显式 workflow node，而不是运行时把同一个 agent 克隆 N 份的执行技巧

### 8.3 LoopWorkflowAgent

作用：重复执行单个 child agent 若干轮，直到 stop policy 满足。

约束：

- loop policy 明确在配置中声明
- 不通过文本推断继续/停止
- 必须显式维护 loop state，例如当前 child、已循环次数、退出原因

### 8.4 Workflow Agent 与 execution mode 的关系

建议后续明确区分：

```text
execution_mode
  = 内核底层调度策略（保守保留，逐步弱化）

workflow agent
  = agent graph 中的一等节点类型（长期主模型）
```

迁移方向：

1. 现阶段允许 `execution_mode` 继续存在，保证兼容
2. 新设计不再围绕 `execution_mode` 扩展多 agent 语义
3. 新增 workflow node 后，`execution_mode` 只保留给单节点内部调度或兼容配置解释
4. 中长期由 `AgentGraph` 决定执行拓扑，而不是 `kernel.execution_mode`

### 8.5 决策条款

为避免后续实现时再次把两套语义混在一起，本文明确以下决策：

1. `Sequential / Parallel / Loop` 的**架构归属**定义为 workflow agent，而不是 kernel 核心能力。
2. `execution_mode` 的**长期职责**不再承载多 agent 产品语义，只承载底层执行调度或兼容入口。
3. 任何新增的多 agent 产品能力，**必须**优先落到 `AgentGraph` / `AgentNodeKind` / workflow node，而不是继续扩展 `execution_mode`。
4. 若某需求需要“多个 agent 有身份、有关系、可恢复、可转移、可审计”，则**必须**建模为 workflow node 或 agent graph 关系，不得实现为 execution wrapper。
5. 仅当需求是“同一个 agent runtime 的内部执行策略差异”，才允许继续使用 `execution_mode`。

### 8.6 判定矩阵

```text
问题：该能力应该落在哪？

1. 是否存在具名 child agent？
   是 -> workflow node / agent graph
   否 -> 继续看 2

2. 是否需要 pause / resume / current_child / loop_count 之类显式状态？
   是 -> workflow node / agent graph
   否 -> 继续看 3

3. 是否只是同一个 agent 的底层执行调度差异？
   是 -> execution_mode
   否 -> workflow node / agent graph
```

### 8.7 兼容矩阵

| 场景 | 短期实现 | 长期归属 |
|------|----------|----------|
| 单个 `LlmAgent` 普通执行 | `execution_mode = Single` | 保留 |
| 单个 `LlmAgent` + transfer 变体 | `FlowKind::Auto` | 保留，不进入迁移范围 |
| 多个同质 child 的临时顺序执行 | 允许兼容 `Sequential` | 迁移到 `SequentialWorkflowAgent` |
| 多个同质 child 的临时并行执行 | 允许兼容 `Parallel` | 迁移到 `ParallelWorkflowAgent` |
| 单 child 重试/循环执行 | 允许兼容 `Loop` | 迁移到 `LoopWorkflowAgent` |
| 专家路由 / agent tree transfer | 不应落在 `execution_mode` | `Transfer + AgentGraph` |
| specialist agent 作为工具调用 | 不应落在 `execution_mode` | `AgentToolHandler` |
| 异步 child session 协作 | 不应落在 `execution_mode` | `SpawnSession` |
| 远程 A2A agent | 不应落在 `execution_mode` | `RemoteA2A` |

说明：

- `FlowKind::Auto` 的本质是“单 agent + allow_transfer”，不是 workflow-like `execution_mode` 扩展。
- 进入兼容收缩路径的只有 `execution_mode = Sequential | Parallel | Loop`，不包括 `FlowKind::Auto`。

---

## 9. AgentGraph 与 Registry

### 9.1 AgentGraph

```rust
pub trait AgentGraph: Send + Sync {
    fn root(&self) -> AgentNodeId;
    fn find(&self, id: &AgentNodeId) -> Option<AgentNodeDescriptor>;
    fn find_by_name(&self, name: &str) -> Option<AgentNodeDescriptor>;
    fn children(&self, id: &AgentNodeId) -> Vec<AgentNodeDescriptor>;
    fn transfer_targets(&self, id: &AgentNodeId) -> Vec<AgentNodeDescriptor>;
    fn invokable_targets(&self, id: &AgentNodeId) -> Vec<AgentNodeDescriptor>;
}
```

### 9.2 AgentRegistry

```rust
pub trait AgentRegistry: Send + Sync {
    fn graph(&self) -> &dyn AgentGraph;
    fn resolve_runtime(&self, id: &AgentNodeId) -> TalosResult<Arc<dyn AgentRuntime>>;
}
```

### 9.3 AgentRuntime

```rust
pub struct AgentRunTurnRequest {
    pub session_id: String,
    pub invocation_id: String,
    pub project_root: Option<PathBuf>,
    pub input: Vec<TalosInputItem>,
    pub profile: LlmAgentProfile,
}

pub struct AgentRunTurnResult {
    pub output_text: String,
    pub output_items: Vec<TalosOutputItem>,
    pub stop_reason: StopReason,
}
```

说明：

- `AgentRunTurnRequest` / `AgentRunTurnResult` 是 `AgentRuntime::run_turn()` 的最小闭包，不再留空到实现阶段。
- `project_root` 必须进入 `AgentRunTurnRequest`，因为现有 `InvocationContext` 已显式承载该字段；`Fresh` child context 不能在 runner 层丢失 workspace/project 绑定。
- `input` 直接复用现有 `TalosInputItem`，而不是重新造一套 agent 输入 block。
- `output_items` 直接复用现有 `TalosOutputItem`，让 `general_turn`、memory、tool result 投影都能共用已有输出模型。

### 9.4 AgentRuntime

```rust
#[async_trait]
pub trait AgentRuntime: Send + Sync {
    fn descriptor(&self) -> &AgentNodeDescriptor;

    async fn run_turn(
        &self,
        request: AgentRunTurnRequest,
    ) -> TalosResult<AgentRunTurnResult>;
}
```

这样 Talos 才能真正从“默认只有一个 llm agent”演进到“图上的多个 agent runtime”，而不需要再用 `default_agent_kind` 做占位配置。

---

## 10. Transfer 机制方案

### 10.1 请求侧

扩展现有 `AgentTransferRequestProcessor`：

- 从 `AgentGraph` 计算当前 agent 可转移目标
- 构造 `transfer_to_agent` 特殊工具
- 把 `transfer_to: Option<String>` 迁移为枚举约束的 `AgentNodeId`
- 在 prompt 中追加“什么时候应该 transfer，什么时候应该继续回答”的简短规则

### 10.2 执行侧

```text
model emits tool_call transfer_to_agent(target=x)
  -> ToolRegistry dispatch
  -> TransferToolHandler validates target
  -> outcome writes EventActions.transfer_to = x
  -> current general_turn ends
  -> AgentImpl.run() observes result.transfer_to
  -> coordinator-level registry resolves target runtime
  -> current turn records target as next active agent
  -> subsequent continuation tail-calls or resumes target agent
```

实现落点：

- 当前 `transfer_to` 已能写入 `EventActions.transfer_to`，但结果侧路由还未闭合；Phase 1 必须在 `orchestration/agent/src/agent.rs` 的 `AgentImpl::run()` 中消费 `result.transfer_to`。
- `Transfer` 的语义是“把控制权交给另一个已注册 agent”，因此路由点应位于 coordinator / orchestrator 层，而不是下沉到单个 `LlmKernelAgent` 内部。
- 在 Phase 1c 完成前，`Transfer` 仍属于“请求侧可表达、结果侧未真正生效”的死链路。

### 10.3 继续执行规则

需要补一个 active-agent 恢复策略：

```text
if last pending tool response belongs to agent A:
    resume A
else if last transfer target is B and B is still transferable:
    resume B
else:
    fall back to root/default agent
```

这部分与 ADK `Runner._find_agent_to_run()` 的思想一致，但 Talos 应基于现有 session/event 模型落地，不应复制其 session 结构。

---

## 11. Agent-as-Tool 机制方案

### 11.1 目标

提供 `AgentToolHandler`，让 agent 能以 tool 形式被调用。

### 11.2 调用流程

```text
current LlmAgent
  -> tool call: agent_review(arguments)
  -> AgentToolHandler
       -> resolve target agent runtime
       -> build child turn request
       -> run child agent via existing kernel runtime
       -> capture final text / structured result
       -> map to ToolOutputPayload::Function
  -> append tool result to current turn
  -> current LlmAgent continues
```

### 11.3 关键约束

- 必须复用现有 kernel runtime，不单独起第二套 runner
- 子调用深度必须有限制，避免无限递归
- 默认串行执行，不标记 `ParallelSafe`
- 审批默认沿用 `ToolSource::Skill | Collaboration` 的强审批语义，或单独抽出 agent delegation governance
- 需要显式支持 `Fresh` / `ForkLike` 两种调用模式；默认只开放 `Fresh`
- `ForkLike` 模式下必须保证 transcript / replacement state / prompt prefix 恢复的一致性

### 11.4 建议接口

```rust
pub struct AgentToolSpec {
    pub target: AgentTargetRef,
    pub invocation_mode: ChildInvocationMode,
    pub input_schema: serde_json::Value,
    pub output_schema: Option<serde_json::Value>,
    pub inheritance: ChildContextInheritance,
    pub max_depth: u8,
}

pub struct AgentToolHandler {
    pub spec: AgentToolSpec,
}
```

说明：

- `input_schema` / `output_schema` 使用 `serde_json::Value` 是对 JSON Schema 的**显式豁免**。
- 这里承载的是 Schema 规范本身，而不是泛型 metadata；实现时不得把这处豁免扩散为“任意运行时元数据都可用 JSON”。

---

## 12. Spawn Session 机制方案

### 12.1 当前问题

当前 `CollaborationRuntime` 已可运行，但还停留在进程内任务句柄层：

- `wait_agent` 依赖 watch channel
- 父子关系未进入强类型 session metadata
- pending / resume 语义未统一到 dynamic tool transcript

### 12.2 目标方案

`SpawnSession` 只承担“独立 child session 协作”：

```text
spawn_agent
  -> create child session
  -> write parent/child relation metadata
  -> return child session handle

wait_agent
  -> emit pending dynamic-style state
  -> parent turn ends with pending
  -> child session completes later
  -> submit typed completion result back as dynamic result
  -> parent turn resumes
```

### 12.3 关键决策

- `wait_agent` 必须落到 transcript 驱动恢复，不再只是内存态等待
- child session 输出映射到 `ToolOutputPayload::Function`
- child session 自己保留完整 transcript，父 session 只记录协作请求和最终结果
- child session 的 resume 必须依赖 `AgentRunMetadata` / transcript，而不是仅依赖进程内句柄

### 12.4 Cross-turn 恢复时序

`SpawnSession` 的恢复不是“同一 turn 内继续采样”，而是显式的跨 turn 恢复链路，最小流程应定义为：

```text
child session completed
  -> child session runner observes terminal output
  -> AgentRunMetadataStore.update_completion(child_run_id, output)
  -> lookup parent_session_id from AgentRunMetadataStore
  -> DynamicToolResultSubmitter.submit(parent_session_id, call_id, tool_name, output_text, success)
  -> parent session 按现有 dynamic tool result 提交链路恢复
```

补充说明：

- 这里可以复用 dynamic tool 的结果事件模型，但**不能**把 `SpawnSession` 简化成“同一 turn 内的 pending tool”。
- dynamic tool result 在这里承担的是跨 turn 恢复触发物，而不是当前采样循环内的立即 tool result。
- parent session 的恢复应复用现有 dynamic tool result 提交链路，不在 spec 中额外假设新的 `trigger_resume` API 已存在。
- `CollaborationRuntime` 当前在代码中是 struct；本文不再隐含要求先把它抽成 trait。

---

## 13. Remote A2A 方案

### 13.1 设计定位

Remote A2A 不是新的 planner，也不是新的 flow，而是 `AgentRuntime` 的一种 adapter。

### 13.2 出站调用

```rust
pub struct RemoteA2aAgentAdapter {
    pub descriptor: AgentNodeDescriptor,
    pub card: RemoteAgentCard,
    pub timeout_ms: u64,
    pub full_history_when_stateless: bool,
}
```

它可被两种能力复用：

- `Transfer` 到远程 agent
- `InvokeAsTool` 调用远程 agent

### 13.3 入站服务

```rust
pub trait TalosA2aExecutor: Send + Sync {
    async fn execute(
        &self,
        request: TalosA2aRunRequest,
    ) -> TalosResult<TalosA2aRunStream>;
}
```

职责：

- A2A request -> Talos request 转换
- Talos event / stop reason / pending state -> A2A task update 映射
- input required / auth required / failed / completed 显式映射为任务状态

### 13.4 关键原则

- Remote A2A 的状态机保持在 adapter 层
- Talos 内部只看 `AgentRuntime`
- 不把 A2A task 状态扩散到 kernel 核心类型

---

## 14. 配置方案

### 14.1 新增配置方向

配置不应散成大量标量参数，建议统一放在 `config` 模块：

```rust
pub struct AgentGraphConfig {
    pub root: String,
    pub nodes: Vec<AgentNodeConfig>,
}

pub enum AgentNodeConfig {
    Llm(LlmAgentNodeConfig),
    Sequential(SequentialAgentNodeConfig),
    Parallel(ParallelAgentNodeConfig),
    Loop(LoopAgentNodeConfig),
    RemoteA2A(RemoteA2aAgentNodeConfig),
}

pub struct DelegationConfig {
    pub max_transfer_depth: u8,
    pub max_agent_tool_depth: u8,
    pub allow_remote_transfer: bool,
    pub allow_remote_invoke: bool,
}
```

最小配置变更：

```rust
pub struct AgentKernelConfig {
    // existing fields...
    pub agent_graph: Option<AgentGraphConfig>,
}
```

说明：

- `agent_graph: Option<AgentGraphConfig>` 是 Phase 1 的最小配置入口。
- `build_kernel_agent_for_request()` 先检查 `agent_graph`，命中则走 graph-based 构建；旧 `execution_mode` 分支仅作为兼容路径暂时保留。

### 14.2 当前配置兼容策略

`binding.default_agent_kind` 当前仅是占位，后续应迁移到 graph-based 配置。

迁移前可以临时保留：

```text
if no agent_graph configured:
    build single-node graph with default llm agent
```

这是配置兼容，不是运行时 fallback 逻辑。

对于当前 `execution_mode`，建议采用以下迁移解释：

```text
if agent_graph configured:
    graph-defined workflow nodes are the only source of truth
    execution_mode must be absent or Single-compatible
    Sequential / Parallel / Loop are rejected during config validation

if agent_graph not configured:
    allow current execution_mode for compatibility
```

也就是说：

- `Single` 可以继续作为默认单节点执行方式
- `Sequential / Parallel / Loop` 不应再作为长期多 agent 架构主入口
- 新增 workflow 能力时，优先落到 `AgentNodeConfig`，而不是继续往 `execution_mode` 塞语义

### 14.3 配置迁移规则

建议把迁移规则明确成配置解释顺序：

```text
1. 如果声明了 agent_graph:
     agent_graph 是唯一真实来源
     workflow 拓扑、节点关系、remote/local 类型都以 graph 为准

2. 如果未声明 agent_graph:
     允许沿用现有单 agent 配置
     并在兼容模式下解释 execution_mode

3. 一旦某功能进入 workflow node:
     不再允许新增同语义的 execution_mode 配置项
```

建议补一个显式兼容标志：

```rust
pub enum MultiAgentConfigMode {
    LegacyExecutionMode,
    AgentGraph,
}
```

用于在加载配置时尽早报错，而不是进入运行时才发现两套配置同时生效。

配置校验时机建议明确为：

- 配置反序列化完成后
- `KernelBuilder` 构造前

最小校验逻辑：

```text
if agent_graph is configured and execution_mode in {Sequential, Parallel, Loop}:
    return Err("workflow-like execution_mode cannot be used together with agent_graph")
```

这样“workflow node 是唯一真实来源”才能从文档约束落成实际门禁。

### 14.4 非法配置组合

以下组合建议在配置校验阶段直接报错：

1. `agent_graph` 已配置，同时又配置 workflow-like `execution_mode = Sequential | Parallel | Loop`
2. `agent_graph.root` 不存在于 `nodes`
3. `TransferAllowed` 指向不存在节点
4. `ToolCallable` 指向不存在节点
5. workflow node 没有 child
6. workflow node 的 child 出现重复引用但未显式声明允许复用

这样可以避免执行期再出现“到底谁是 source of truth”的歧义。

---

## 15. Prompt 设计要求

`PromptBuilder` 后续应增加 agent 编排上下文，但仍保持统一装配入口。

建议新增两类静态/动态段：

1. `Delegation` 段
   描述当前 agent 可转移目标、可调用的 specialist agent tool
2. `Agent Role` 段
   描述当前 agent 在 graph 中的职责边界

示意：

```text
System
Tooling
Delegation
Planning Instruction
Project Context
Memory
User Input
```

约束：

- 不新增第二套 prompt builder
- 不在调用点拼接 transfer/agent-tool 文本
- 统一由 `PromptBuilder` 生成
- 可变的 agent 列表、tool 列表、delegation target 列表应优先走动态段，避免频繁破坏静态 prompt cache 前缀

---

## 16. 与现有 Talos 模块的映射

| 模块 | 新职责 | 备注 |
|------|--------|------|
| `infra/config` | `AgentGraphConfig`、`DelegationConfig`、remote A2A config | 统一配置入口 |
| `orchestration/agent` | agent registry / graph 组装、active agent 恢复策略 | 不直接承载协议细节 |
| `orchestration/agent-kernel` | `LlmAgent`、workflow agent、transfer processor、agent tool runtime、child context factory、resume metadata | 保持统一 `general_turn` |
| `action/tools` | `TransferToolHandler`、`AgentToolHandler`、session collaboration tools | 特殊 tool 编排层 |
| `gateway/http` | A2A 入站端点、dynamic/session collaboration 提交接口 | 协议适配 |
| `common/prompt` | delegation prompt sections | 不感知 transport |

### 16.1 最小实现插入点

- `orchestration/agent/src/agent/kernel.rs`
  在 `build_kernel_agent_for_request()` 增加 `agent_graph` 分支；第一版允许先只构建 graph root 对应的单节点 runtime，再逐步接入 workflow node。
- `orchestration/agent/src/agent.rs`
  在 `AgentImpl::run()` 中补 `result.transfer_to` 的结果侧路由；该处应依赖 coordinator 持有的 registry/active-agent 状态做 tail-call 或下一轮恢复决策。
- `orchestration/agent-kernel/src/runner.rs`
  在 `DefaultRunner::run_turn()` 用 `AgentExecutionContextFactory::build_child_context()` 替换直接手工铺开的 `InvocationContext` 构造，至少先覆盖 `Fresh` 路径。
- `orchestration/agent/src`
  `AgentRunMetadataStore` 以依赖注入方式挂到 `AgentImpl`，初始实现可采用 in-memory `DashMap`；后续如需持久化再替换后端。
- `common/types::SessionService`
  现有 `submit_dynamic_tool_result(...)` 作为 `DynamicToolResultSubmitter` 的首个宿主实现，不额外引入第二套 resume 提交接口。

---

## 17. 核心接口清单

| 类型 | 作用 | 核心方法 |
|------|------|----------|
| `LlmAgentProfile` | agent 内部执行配置 | profile fields |
| `AgentGraph` | 维护 agent 关系图 | `find` / `children` / `transfer_targets` |
| `AgentRegistry` | 解析节点到 runtime | `resolve_runtime` |
| `AgentRuntime` | 统一本地/远程执行接口 | `run_turn` |
| `AgentExecutionContextFactory` | 构造 child invocation context | `build_child_context` |
| `AgentRunMetadataStore` | child run metadata 持久化 | `write` / `update_completion` / `read` / `list_by_session` |
| `AgentTransferRequestProcessor` | 注入 transfer prompt/tool | `run` |
| `TransferToolHandler` | 执行 transfer 动作 | `handle` |
| `AgentToolHandler` | 执行 agent-as-tool | `handle` |
| `RemoteA2aAgentAdapter` | 远程 agent adapter | `run_turn` |
| `TalosA2aExecutor` | Talos 对外 A2A 服务 | `execute` |
| `CollaborationRuntime` | legacy child session 任务句柄 | `spawn` / `send` / `close`；`wait` 进入删除路径 |

---

## 18. 核心 API 伪代码

### 18.1 Transfer

```rust
async fn execute_transfer(
    ctx: &mut InvocationContext,
    target: AgentNodeId,
) -> TalosResult<()> {
    ctx.execution_state.delegation = Some(DelegationDecision::Transfer { target });
    Ok(())
}
```

### 18.2 Agent-as-Tool

```rust
async fn execute_agent_tool(
    registry: &dyn AgentRegistry,
    context_factory: &dyn AgentExecutionContextFactory,
    spec: &AgentToolSpec,
    parent: &InvocationContext,
) -> TalosResult<ToolOutputPayload> {
    let runtime = registry.resolve_runtime(spec.target.local_id()?)?;
    let child_ctx = context_factory.build_child_context(
        parent,
        runtime.descriptor(),
        &ChildAgentRunOptions {
            mode: spec.invocation_mode,
            inheritance: spec.inheritance.clone(),
            transcript_group: None,
            resume_policy: ChildResumePolicy::Disabled,
        },
    )?;
    let result = runtime.run_turn(child_ctx.request).await?;
    Ok(ToolOutputPayload::Function {
        content: result.output_text,
        success: true,
        structured_content: project_tool_structured_output(&result.output_items),
    })
}
```

其中：

```rust
fn project_tool_structured_output(
    items: &[TalosOutputItem],
) -> Option<serde_json::Value>;
```

说明：

- `ToolOutputPayload::Function` 现有接口仍要求 `structured_content: Option<serde_json::Value>`。
- 这里不在 `AgentRunTurnResult` 中重复保存一份弱类型 JSON，而是在 agent-as-tool 边界把 `TalosOutputItem` 投影为可选结构化结果。

### 18.3 Remote A2A

```rust
async fn execute_remote_a2a(
    adapter: &RemoteA2aAgentAdapter,
    request: AgentRunTurnRequest,
) -> TalosResult<AgentRunTurnResult> {
    let a2a_request = convert_talos_request_to_a2a(request)?;
    let a2a_result = adapter.client.run(a2a_request).await?;
    convert_a2a_result_to_talos(a2a_result)
}
```

### 18.4 SpawnSession Completion

```rust
#[async_trait]
pub trait DynamicToolResultSubmitter: Send + Sync {
    async fn submit(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()>;
}

async fn propagate_child_completion(
    metadata_store: &dyn AgentRunMetadataStore,
    dynamic_result_submitter: &dyn DynamicToolResultSubmitter,
    child_run_id: &AgentRunId,
    output: AgentRunOutput,
    call_id: &str,
    tool_name: &str,
) -> TalosResult<()> {
    metadata_store.update_completion(child_run_id, output.clone())?;
    let metadata = metadata_store
        .read(child_run_id)?
        .ok_or_else(|| TalosError::not_found("child run metadata"))?;
    let parent_session_id = metadata
        .parent_session_id
        .ok_or_else(|| TalosError::invalid_state("missing parent session id"))?;
    dynamic_result_submitter.submit(
        &parent_session_id,
        call_id,
        tool_name,
        render_agent_run_output_text(&output),
        matches!(output.status, AgentRunStatus::Completed),
    ).await?;
    Ok(())
}
```

说明：

- `DynamicToolResultSubmitter` 的首个实现应直接包装现有 `AgentImpl::submit_dynamic_tool_result(...)`，不在 spec 中假设新的 session 恢复 API。
- `SpawnSession` 语义下父 agent 不再同步等待 child；child 终态通过 `DynamicToolResultSubmitter` 主动回传，因此 `CollaborationRuntime::wait()` 的 watch-based 逻辑进入删除路径，而不是继续扩展。

---

## 19. 分阶段落地顺序

### 阶段 1: AgentGraph 与 Transfer 收敛

目标：

- 建立 `AgentGraph` / `AgentRegistry`
- 本地 `transfer_to_agent` 特殊工具
- active agent 恢复策略
- 明确 workflow node 与 `execution_mode` 的兼容边界

完成标准：

- 单 session 内 root/sub/peer transfer 可工作
- transfer target 使用 enum 约束
- 不新增第二条执行主路径
- `execution_mode` 与 `workflow node` 的 source of truth 已明确并落到配置校验

建议拆成 3 个代码级子步骤：

- `1a. AgentKernelConfig + 配置校验`
  在 `AgentKernelConfig` 增加 `agent_graph: Option<AgentGraphConfig>`，并用 `MultiAgentConfigMode` 在配置反序列化后、`KernelBuilder` 前执行冲突校验。
- `1b. build_kernel_agent_for_request() graph 分支`
  在 `orchestration/agent/src/agent/kernel.rs` 先加 graph 分支；第一版只要求能从 graph root 构建单节点 runtime，旧 `execution_mode` 分支暂作为 deprecated path 保留。
- `1c. AgentImpl.run() transfer 路由闭环`
  在 `orchestration/agent/src/agent.rs` 中消费 `result.transfer_to` 并路由到 target agent。

补充：

- `1c` 是当前最高优先级，因为现状下 `transfer_to` 已能进入 kernel request / event，但结果侧被丢弃，属于死代码链路。

### 阶段 2: LlmAgent Runtime Refine

目标：

- 引入 `LlmAgentProfile`
- 引入 `ChildInvocationMode`
- 引入 child context factory 与 tool 过滤
- 引入 transcript / metadata store

完成标准：

- `LlmAgent` 内部运行配置不再散落在调用链上
- child invocation 支持 `Fresh` 与 `ForkLike` 明确建模
- child transcript / metadata 可用于 resume
- tool pool 由统一工厂裁剪，不再依赖调用点临时拼装

建议拆成 4 个可独立验收的子步骤：

- `2a. LlmAgentProfile`
  从 config / agent graph 构造 `LlmAgentProfile`；`LlmKernelAgent` 构造函数改为显式接收 profile（或只从 profile 派生执行配置），不再直接透传整份全局 `AgentKernelConfig` 作为 agent 身份配置来源。
- `2b. Fresh ContextFactory`
  先只实现 `Fresh` 路径；在 `orchestration/agent-kernel/src/runner.rs` 的 `DefaultRunner::run_turn()` 用 `AgentExecutionContextFactory` 生成 child context，`AgentToolPolicy` 生效，tool 过滤和 inheritance 单测通过。
- `2c. MetadataStore`
  在 `orchestration/agent/src` 通过依赖注入挂入 `AgentRunMetadataStore`；初始实现采用 in-memory `DashMap`，补 `write / update_completion / read / list_by_session` 与 child run 终态查询单测。
- `2d. ForkLike`
  最后实现 `ForkLike` 路径，补 prompt prefix、replacement state、workspace 继承边界单测。

### 阶段 3: AgentToolHandler

目标：

- 本地 agent-as-tool
- specialist agent 作为特殊 tool 注入
- 深度限制与继承策略生效
- 将 `Sequential / Parallel / Loop` 的新能力实现优先落到 workflow node，而不是继续扩展 execution wrapper
- 复用 `LlmAgentProfile` 与 child context factory，不新增临时子 runtime

完成标准：

- 当前 agent 可调用另一个 agent 并拿到 tool result
- tool result 能进入现有 `general_turn` 闭环
- 新增 specialist/workflow 能力不再通过 execution wrapper 暗中构造同质 child

### 阶段 4: SpawnSession 收敛

目标：

- collaboration tools 与 dynamic pending/resume 统一
- parent/child session metadata 强类型化

完成标准：

- `wait_agent` 不再只是进程内等待
- child session 结果可驱动 parent turn 恢复

建议拆成 2 个子步骤：

- `4a. DynamicToolResultSubmitter 接线`
  以 `AgentImpl` 为 `DynamicToolResultSubmitter` 的首个实现，直接包装现有 `submit_dynamic_tool_result(...)`；`SpawnSession` child completion 通过该接口回传到父 session。
- `4b. 删除 watch-based wait`
  `CollaborationRuntime.wait()` 不再作为长期语义保留；在 `SpawnSession` transcript-driven completion 路径稳定后删除现有 watch 轮询逻辑。

执行顺序说明：

- 代码级落地顺序建议为 `1a -> 1b -> 1c -> 2a -> 2b -> 2c -> 4a -> 2d -> 4b -> 5`。
- 这样可以先打通 `Transfer` 和 `Fresh` child runtime 的最小闭环，再补 `ForkLike` 与旧 `wait()` 的删除收尾。

### 阶段 5: Remote A2A

目标：

- `RemoteA2aAgentAdapter`
- `TalosA2aExecutor`

完成标准：

- 本地可将 remote agent 当 transfer target 或 agent tool 调用
- Talos 可对外输出 A2A task/event

---

## 20. 测试与门禁

### 20.1 单测

- `LlmAgentProfile`：profile 解析、默认值、配置覆盖
- `AgentExecutionContextFactory`：`Fresh` / `ForkLike` 继承边界、tool 过滤、memory 透传
- `AgentToolPolicy`：`AllowAll` / `Allowlist` / `DenyBySource` 的裁剪结果
- `AgentRunMetadataStore`：写入、完成态更新、按 session 查询、resume 路径
- `AgentGraph`：唯一命名、父子关系、查找、目标推导
- 配置校验：`LegacyExecutionMode` / `AgentGraph` 冲突检测
- `AgentTransferRequestProcessor`：prompt/tool 注入正确
- `AgentImpl.run()`：`transfer_to` 命中后能更新 active agent 或路由到目标 runtime
- `build_kernel_agent_for_request()`：配置 `agent_graph` 时优先走 graph 分支，兼容模式下仍可解释 legacy `execution_mode`
- `TransferToolHandler`：合法目标、非法目标、peer/parent 限制
- `AgentToolHandler`：输入输出 schema、深度限制、上下文继承
- `RemoteA2aAgentAdapter`：请求映射、错误映射、超时映射

### 20.2 集成测试

- 单 turn transfer
- 多 turn active agent 恢复
- fresh child invoke
- fork-like child invoke
- agent-as-tool 当前 agent 汇总子 agent 结果
- sequential workflow pause/resume
- parallel workflow branch isolation 与结果聚合
- loop workflow 的显式 stop policy
- child session wait/resume
- child session completion -> parent session resume
- remote A2A 调用最小链路

### 20.3 回归门禁

- 相关 crate 单元测试
- `tests/integration/run_e2e.sh`
- 至少一条覆盖多 agent 的新 e2e 场景

---

## 21. 风险与待确认问题

1. `Transfer` 后用户后续消息的 active agent 恢复策略，需要和现有 `transfer_to` 事件模型统一。
2. `Fresh` 与 `ForkLike` 的继承边界需要在 memory、workspace、tool result replacement、prompt prefix 四个层面一次定死，否则 resume 与 cache 稳定性都会反复摇摆。
3. remote A2A 的状态映射是否需要对 `auth_required` / `input_required` 做内核级 stop reason 扩展，还是保持在 gateway adapter 层。
4. workflow agent 的聚合输出格式是否统一为文本，还是允许结构化输出组合，需要在实现前明确。
5. `execution_mode` 的保留边界需要进一步定死，否则 workflow node 与 execution wrapper 容易长期并存并造成重复语义。
6. 当前测试和 e2e 大多围绕单 agent 与工具调用，workflow node 引入后需要补足恢复态与多 turn 场景覆盖，否则很容易只在 happy path 上成立。
7. `InvocationContext.transfer_to` 当前同时承载“请求侧 hint”和“执行侧 decision”两种语义；迁移到 `DelegationDecision` 时必须拆成两个独立字段，避免提示值覆盖执行结果或反向污染 prompt 注入。
8. `AgentRunMetadataStore` 与现有 session/transcript 存储的边界需要明确，避免 child run metadata 重复散落到多个模块。
9. `LlmKernelAgent` 当前大量直接消费 `AgentKernelConfig`；迁移到 `LlmAgentProfile` 时需要明确哪些字段保留为 runtime-wide config，哪些字段必须下沉为 per-agent profile，避免 profile 与 global config 双写。

---

## 22. 最终决策摘要

Talos 后续多 agent / A2A 机制采用以下路线：

- 以 `LlmAgent` 为唯一原子执行单元
- workflow agent 只编排，不承担 LLM 语义
- 多 agent 能力拆分为 `Transfer`、`InvokeAsTool`、`SpawnSession`、`RemoteA2A`
- A2A 仅作为 remote adapter，不进入 kernel 核心主链
- 全部能力继续复用统一 `general_turn`
- 控制面采用 `AgentGraph + workflow node` 模型
- `LlmAgent` 内部引入 `LlmAgentProfile`、`Fresh/ForkLike` 子调用、child context factory、tool 过滤、transcript-driven resume metadata
- `Sequential / Parallel / Loop` 长期升级为 `AgentGraph` 中的一等 workflow node
- `execution_mode = Single` 保留为稳定主入口，`FlowKind::Auto` 作为 transfer 变体同样保留
- `Sequential / Parallel / Loop` 进入兼容收缩路径
- 新增多 agent 能力以 `AgentGraph` 为唯一真实来源，不再继续向 `execution_mode` 扩展语义

这套方案既吸收了 ADK 的分层经验，也保持了 Talos 当前“统一主路径、强类型配置、特殊 tool 编排”的架构方向。
