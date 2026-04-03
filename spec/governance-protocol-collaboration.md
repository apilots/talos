# Talos 后续架构设计：治理收口 · 协议生命周期 · 协作工具

## 背景

当前 Talos tools 子系统已完成核心主干迁移。本文档记录三个阶段的规划：
已完成内容归档于"现状确认"节，仍需推进的工作在各层"设计目标"节展开。

---

## 第一层：统一治理层（现状确认）

本层核心工作**已完成**，以下为当前实际状态，供后续层次参考。

### 1.1 已完成的实现

**`RegistryOrchestrator`（metadata 驱动，非硬编码）**

```
RegistryOrchestrator::plan(invocation, context, spec)
┌─────────────────────────────────────────────────────┐
│  1. pre_approved?    → Skipped (emit decision)      │
│  2. no kernel_config → Skipped (emit decision)      │
│  3. spec.governance → resolve_governance()          │
│       argument_override::None                       │
│         → 使用 spec.governance 原始值               │
│       argument_override::ExecCommandPermissions     │
│         → 读取 arguments["sandbox_permissions"]     │
│         → 按 approval_policy 决定 approval          │
│  4. build ToolExecutionPlan                         │
│  5. emit_decision (via GovernanceEventSink)         │
└─────────────────────────────────────────────────────┘
```

**关键类型（均已在 `tools-sandbox/src/lib.rs` 定义）：**

```rust
pub struct ToolGovernanceSpec {
    pub default_approval: ToolApprovalRequirement,
    pub preferred_sandbox: ToolSandboxMode,
    pub argument_override: ToolGovernanceArgumentOverride,  // 枚举，非 Option
    pub retry_without_sandbox: bool,
}

pub enum ToolGovernanceArgumentOverride {
    #[default] None,
    ExecCommandPermissions,
}

pub struct ToolGovernanceDecision {
    pub tool_name: String,
    pub call_id: String,
    pub outcome: GovernanceOutcome,
    pub sandbox_mode: ToolSandboxMode,
    pub decision_reason: GovernanceDecisionReason,
}

pub trait GovernanceEventSink: Send + Sync {
    fn on_decision(&self, decision: &ToolGovernanceDecision);
}
```

**`RegisteredToolSpec`（`tools/src/spec.rs`）** 已包含 `governance: ToolGovernanceSpec` 字段。

**`ToolSource::Skill`** 已存在，`default_governance_for_source()` 已为 `Skill | Collaboration` 配置默认强审批。

### 1.2 Layer 1 仍待完成

- **`GovernanceEventSink` 实际接入**：`RegistryOrchestrator` 已有 `with_sink()` 方法，但目前调用侧（`ToolRegistry::dispatch()`）在构建 `RegistryOrchestrator` 时不传入 sink。需在初始化路径上注入一个实现（日志 sink 或 tracing sink）。
- **新工具注册时补充 `ToolGovernanceSpec`**：当前 `local_function()` 便捷构造方法使用 `ToolGovernanceSpec::default()`（approval=Skip），后续注册新工具时必须显式声明 governance，不能依赖 default。

---

## 第二层：外部协议生命周期（External Protocol Lifecycle）

### 2.1 Dynamic Tool 生命周期

#### 现状问题

```rust
// handlers/dynamic.rs — External 模式当前实现
DynamicToolBackend::External => {
    if let Some(result) = load_dynamic_tool_result(context, call_id)? {
        Ok(ToolDispatchOutcome::executed(...))
    } else {
        Ok(ToolDispatchOutcome::pending_dynamic(...))
    }
}
```

缺失：
- 无超时：外部调用方可无限期不响应，session 无限 pending
- 无取消：session 关闭时 pending 状态无清理路径
- 无防重放：同一 `call_id` 可多次写入 context state
- 无外部接口：SDK/CLI 无标准入口提交 dynamic tool result

#### 状态机

```
              注册
               │
               ▼
        ┌─────────────┐
        │  Registered │  工具已注册，等待 LLM 调用
        └──────┬──────┘
               │ LLM 发出 tool_call
               ▼
        ┌─────────────┐
        │   Pending   │  等待外部响应（context state 中无结果）
        └──────┬──────┘
    ┌──────────┼─────────────┐
    │          │             │
    ▼          ▼             ▼
┌──────────┐ ┌─────────┐ ┌──────────┐
│ Resolved │ │TimedOut │ │Cancelled │
└──────────┘ └─────────┘ └──────────┘

Pending → Resolved:   外部通过 SDK 提交结果
Pending → TimedOut:   超过 DynamicToolTimeoutPolicy 配置时长
Pending → Cancelled:  session 关闭，或外部主动取消
```

#### 超时策略

```rust
// 新增至 talos_config::AgentKernelConfig::tools
pub struct DynamicToolTimeoutPolicy {
    /// 等待外部响应的最大时长（None = 永不超时，不推荐用于生产）
    pub response_timeout: Option<Duration>,
    pub on_timeout: DynamicToolTimeoutBehavior,
}

pub enum DynamicToolTimeoutBehavior {
    /// 返回错误给 LLM，由模型决策下一步
    ErrorToModel { message: String },
    /// 终止整个 general_turn
    AbortTurn,
}
```

#### 防重放

```text
submit_dynamic_tool_result(session_id, call_id, result)
  │
  ├── 查 session transcript：call_id 是否已有 DynamicToolResponsePayload
  │     有 → 返回 AlreadyResolved 错误（幂等拒绝，不重复写入）
  │
  ├── 查 session 状态：call_id 是否仍为 Pending
  │     否（TimedOut / Cancelled）→ 返回 NotPending 错误
  │
  └── 写入 context state["dynamic_tool_result::{call_id}"]
        + 写入 transcript: DynamicToolResponsePayload
        → 触发 session 继续执行
```

#### 外部接口规范

```rust
// 新增 trait，由 gateway crate 实现并暴露 HTTP/WS 端点
pub trait DynamicToolResultSubmitter: Send + Sync {
    async fn submit(
        &self,
        session_id: &str,
        call_id: &str,
        result: DynamicToolResult,
    ) -> TalosResult<()>;

    async fn cancel(
        &self,
        session_id: &str,
        call_id: &str,
        reason: Option<String>,
    ) -> TalosResult<()>;

    async fn list_pending(
        &self,
        session_id: &str,
    ) -> TalosResult<Vec<PendingDynamicToolCall>>;
}

pub struct DynamicToolResult {
    pub output: String,
    pub success: bool,
    /// 结构化输出，对应 ToolOutputPayload::Function::structured_content
    pub structured_content: Option<serde_json::Value>,
}

pub struct PendingDynamicToolCall {
    pub call_id: String,
    pub tool_name: String,
    /// 工具调用参数（来自 ToolPending::Dynamic::arguments，类型由工具 schema 约束）
    pub arguments: serde_json::Value,
    pub pending_since: DateTime<Utc>,
    pub timeout_at: Option<DateTime<Utc>>,
}
```

> `arguments` 使用 `serde_json::Value` 因工具参数 schema 在运行时确定，由工具注册时的 `ToolDefinition::parameters_schema` 约束，此处不重复建模。

### 2.2 MCP Resource 生命周期

#### 缓存策略

```text
MCP Resource 访问路径（含缓存）：

list_resources(server)
  │
  ├── cache hit (age < resource_ttl=60s) → 返回缓存
  └── cache miss / expired
        → 请求 MCP server
        → 更新缓存
        → 返回结果

list_resource_templates()
  │
  ├── cache hit (age < template_ttl=120s) → 返回缓存
  └── cache miss / expired → 请求 → 更新 → 返回

read_resource(uri)
  └── 不缓存，每次直接请求（内容可能随时变化）

缓存失效触发：
  - TTL 到期
  - MCP server 重连
  - （未来）MCP server 发送 resources/updated 通知
```

```rust
// 新增至 mcp/mod.rs 中的 McpConnectionManager
struct McpResourceCache {
    resources: HashMap<String, CachedEntry<Vec<McpResource>>>,
    templates: Option<CachedEntry<Vec<McpResourceTemplate>>>,
    resource_ttl: Duration,
    template_ttl: Duration,
}

struct CachedEntry<T> {
    value: T,
    cached_at: Instant,
    ttl: Duration,
}

impl<T> CachedEntry<T> {
    fn is_valid(&self) -> bool {
        self.cached_at.elapsed() < self.ttl
    }
}
```

#### MCP Error → ToolOutputPayload 映射规则

```text
MCP JSON-RPC error response
  │
  ├── code -32601 (MethodNotFound)
  │     → ToolOutputPayload::Function { success: false,
  │         content: "MCP method not supported: {message}" }
  │
  ├── code -32602 (InvalidParams)
  │     → ToolOutputPayload::Function { success: false,
  │         content: "Invalid MCP arguments: {message}" }
  │
  ├── code -32603 (InternalError)
  │     → ToolOutputPayload::Function { success: false,
  │         content: "MCP server internal error: {message}" }
  │
  ├── code other
  │     → ToolOutputPayload::Mcp { is_error: true,
  │         content: "MCP error {code}: {message}" }
  │
  └── transport error（连接失败、超时）
        → TalosError::ToolError（向上抛，不吞错误）
```

#### 分页支持

```rust
// 扩展 McpConnectionManager
pub async fn list_resources_paginated(
    &self,
    server: &str,
    cursor: Option<String>,
) -> TalosResult<McpResourcePage>;

pub struct McpResourcePage {
    pub resources: Vec<McpResource>,
    pub next_cursor: Option<String>,
}
```

### 2.3 Transcript Replay 约束

#### 参与 Replay 的事件（状态恢复必须重放）

| 事件类型 | 重放用途 |
|---------|---------|
| `ToolCallEvent` | 确定 LLM 发出了哪些调用 |
| `ToolExecutedEvent` | 确定哪些工具已完成 |
| `ApprovalResponseEvent` | 恢复 `pre_approved_call_ids` 集合 |
| `DynamicToolResponsePayload` | 恢复 `context.state["dynamic_tool_result::*"]` |

#### 仅做审计的事件（不参与 Replay）

| 事件类型 | 说明 |
|---------|------|
| `GovernanceDecisionEvent` | 可由当前 config + spec 重新推导，无需持久化 |
| `SandboxPlanEvent` | 同上 |
| `ToolOutputTruncatedEvent` | 审计用，不影响状态恢复 |

#### 跨 Turn Pending 状态恢复一致性

```text
Turn N 正在执行，dynamic tool pending 发生：

  Turn N
    ├── LLM tool_call [call_id=abc]
    ├── DynamicToolHandler::External → context state 无结果
    │     → ToolDispatchOutcome::Pending(Dynamic)
    │     → 写入 transcript: DynamicToolRequestPayload
    └── Turn N 挂起

  外部提交结果：
    submit(session, call_id=abc, result)
    ├── 校验防重放（transcript 中无已有响应）
    ├── 写入 transcript: DynamicToolResponsePayload
    └── 触发 Turn N 继续

  Turn N 恢复（replay 约束）：
    1. 严格按 transcript 顺序重放
    2. 读到 DynamicToolResponsePayload
       → 写入 context.state["dynamic_tool_result::abc"]
    3. DynamicToolHandler 重新执行
       → context state 有值 → Executed

约束：
  - DynamicToolResponsePayload 写入后不可修改（防重放依赖此保证）
  - pre_approved_call_ids 由 ApprovalResponseEvent 重建
  - 跨 pending 恢复时不重新触发 governance 检查
```

---

## 第三层：Collaboration / A2A / Skill Tool 概念对齐

本层**不出实现代码**，收敛设计问题，形成后续实施前的共识基础。

### 3.1 Skill 的运行时定位

`ToolSource::Skill` 和 `default_governance_for_source()` 已存在，**运行时定位已确定为 Option B（runtime tool source）**。

当前缺失的是实际实现：
- `SkillToolHandler`（实现 `ToolHandler` trait）
- skill 的注册路径（如何从 skill 定义创建 `RegisteredToolSpec`）
- skill handler 内部如何调用子 LLM 或外部 API

**设计约束（已由 `default_governance_for_source` 固化）：**

```rust
// spec.rs — 已有
ToolSource::Collaboration | ToolSource::Skill => ToolGovernanceSpec {
    default_approval: ToolApprovalRequirement::NeedsApproval {
        reason: Some("tool spawns sub-agent work and requires approval".to_string()),
    },
    ...
}
```

### 3.2 A2A 调用模型

**问题：** LLM 是否可直接调用 A2A tool？

**结论：A2A tool 必须经过 orchestrator 限流**

```text
LLM 发出 A2A tool_call
  │
  ▼
RegistryOrchestrator（Skill/Collaboration 默认 NeedsApproval）
  │
  ├── approval_policy = Never    → Forbidden（禁止 A2A 调用）
  ├── approval_policy = OnRequest → Pending（等待用户审批）
  └── pre_approved               → A2AToolHandler 执行
```

审批粒度：**per-call**（不是 per-agent）。  
理由：同一子 agent 在不同场景的调用代价差异很大，应每次独立审批。

### 3.3 Collaboration 执行原语映射

原语需求：`spawn_agent / send_input / wait / close`

```text
spawn_agent(agent_kind, initial_prompt)
  → 在 session store 创建子 session
  → 父子关系写入 session metadata
  → 子 session_id 作为 handle 返回

send_input(handle, input)
  → 向子 session 提交新 turn

wait(handle)
  → 主 session 挂起（类比 dynamic tool Pending）
  → 监听子 session KernelStreamEvent
  → 子 session general_turn 完成 → 触发主 session 继续

close(handle)
  → 终止子 session
  → 写入 audit event
  → 清理 session state
```

**子 session 与主 session 的关系：**

```text
主 session (Turn N)
  │
  ├── spawn_agent → 子 session [child_session_id]
  │
  ├── wait(child)
  │     主 session 挂起（类比 DynamicToolResponsePayload 触发继续）
  │     子 session 执行中（独立 transcript）
  │
  └── 子 session 完成
        → 子 session 最终输出 → ToolOutputPayload::Function
        → 写入主 session transcript
        → Turn N 继续
```

**Collaboration tool 输出类型：** `ToolOutputPayload::Function`（不新增类型）。  
子 session 执行过程通过子 session 自己的 transcript 记录，主 session 只记录最终结果。

### 3.4 待解决的设计问题（进入实施前需对齐）

1. **skill handler 内部 LLM 调用**：是直接用 `LlmProvider`，还是递归进入 `AgentKernel`？后者有无限递归风险，需要深度限制。

2. **A2A 子 session 的 tool 权限**：子 session 是否继承父 session 的 `kernel_config`（含 approval_policy），还是使用独立配置？

3. **wait(handle) 的阻塞语义**：主 session 挂起期间是否占用 executor 线程？需要与 dynamic tool Pending 复用同一套 session 挂起机制。

4. **子 session 失败传播**：子 session 内部 `TalosError` 如何映射到主 session 的 `ToolOutputPayload`？直接作为 `success: false` 的 Function 输出？还是让主 session 的 LLM 感知失败原因？

---

## 实施顺序

```
第一阶段（治理层收尾）
  ├── ToolRegistry::dispatch() 初始化时注入 GovernanceEventSink（logging/tracing）
  └── 新工具注册时显式声明 ToolGovernanceSpec，禁止依赖 default()

第二阶段（外部协议产品化）
  ├── Dynamic tool：超时策略 + 状态机 + 防重放
  ├── DynamicToolResultSubmitter SDK 接口 + gateway HTTP 端点
  ├── MCP resource 缓存层（McpResourceCache）
  └── MCP error → ToolOutputPayload 统一映射

第三阶段（设计对齐后实施）
  ├── SkillToolHandler 实现
  ├── A2A tool handler + orchestrator 限流
  └── spawn/wait/close collaboration 原语（复用 dynamic tool Pending 机制）
```

---

## 关键文件参考

| 文件 | 说明 |
|------|------|
| `talos-rs/crates/action/tools/src/spec.rs` | `RegisteredToolSpec`（含 `governance`），`default_governance_for_source()` |
| `talos-rs/crates/action/tools/src/orchestrator.rs` | `RegistryOrchestrator`（metadata 驱动，已完成） |
| `talos-rs/crates/action/tools/src/output.rs` | `ToolDispatchOutcome`，`ToolPending` |
| `talos-rs/crates/action/tools/src/handlers/dynamic.rs` | Dynamic tool handler，External 模式，待加超时 |
| `talos-rs/crates/action/tools/src/mcp/mod.rs` | MCP，待加缓存和错误映射 |
| `talos-rs/crates/action/tools-sandbox/src/lib.rs` | `ToolGovernanceSpec`，`ToolGovernanceDecision`，`GovernanceEventSink` |
| `talos-rs/crates/infra/config/src/lib.rs` | `AgentKernelConfig`，待加 `DynamicToolTimeoutPolicy` |
| `talos-rs/crates/orchestration/agent/src/agent.rs` | Agent 执行入口，`GovernanceEventSink` 注入点 |
