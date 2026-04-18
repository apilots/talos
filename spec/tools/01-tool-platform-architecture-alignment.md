# Tool Platform Architecture Alignment

## 1. 目标

本篇定义 Talos 工具平台层的统一改造方案，目标是让 Talos 的工具系统具备与 Claude Code 同级别的承载能力，而不是停留在“工具可执行”的最小模型。

平台层要解决的核心问题：

1. 统一工具契约
2. 统一输入校验与权限判断
3. 统一并发/串行调度语义
4. 统一结果预算与持久化策略
5. 统一上下文回写能力
6. 统一展示摘要与模型提示信息

---

## 2. 现状判断

Talos 当前骨架已经具备良好基础：

- `ToolRegistry` 负责注册与分发
- `ToolRouter` 负责 invocation normalize
- `RegistryOrchestrator` 负责 sandbox/approval 规划
- `ToolHandler` 区分 local / dynamic / mcp

这条主链路是正确的，不建议推翻重写。

当前真正的短板是：**工具描述能力太弱，导致执行前后大量语义缺失。**

### 2.1 不该破坏的部分

以下骨架明确保留：

- `ToolRegistry`
- `ToolRouter`
- `RegistryOrchestrator`
- `ToolHandler`
- `McpConnectionManager`

本次改造是 **增强这些骨架之上的平台能力**，不是推翻主链路。

### 2.2 当前框架层具体问题

#### 问题 1：公开工具名与内部执行标识混杂

当前 Talos 的注册语义更接近：

```text
internal name == public name == prompt visible name
```

这会导致：

- 内部重构空间不足
- 迁移到 Claude Code naming 时冲击面过大
- alias 一旦引入，容易变成多套公开名字并存

#### 问题 2：治理只覆盖 sandbox / approval，不覆盖工具全生命周期

当前治理中心在 `RegistryOrchestrator`，但它关注的仍然主要是：

- sandbox mode
- approval requirement
- escalation override

缺失的治理点包括：

- 输入合法性校验
- 权限规则匹配
- destructive 判定
- 并发安全判定
- interrupt/cancel 语义
- deferred tool 治理
- 结果预算治理

#### 问题 3：上下文状态不足以支撑系统工具平台

Claude Code 的大量安全与正确性约束，本质都依赖上下文状态，例如：

- 文件是否读过
- 文件读后是否发生外部修改
- 工具是否已经被批准/拒绝
- 当前批次工具是否允许并发
- 大结果是否已经持久化

Talos 现在缺少这类统一上下文。

#### 问题 4：框架层没有统一“工具治理接口”

Talos 当前更像：

```text
注册 schema -> execute
```

而 Claude Code 更像：

```text
schema + validate + permission + read_only + concurrency + result_policy + display + execute
```

如果 Talos 不增加这一层，后续每个工具都只能各自重复实现。

---

## 3. 目标架构

### 3.1 总体执行流

```text
Model Tool Use
    |
    v
Tool Name Resolve
    |
    v
Input Validation
    |
    v
Permission + Governance Resolve
    |
    v
Concurrency Scheduling
    |
    v
Executor Call
    |
    +--> Progress Events
    |
    v
Context Mutation Commit
    |
    v
Result Budget / Persist / Summarize
    |
    v
Transcript / UI / Next-turn Context
```

### 3.2 架构原则

- 保留现有 `Registry -> Router -> Orchestrator -> Handler -> Runtime`
- 在 `Tool` 之上新增更厚的 descriptor/behavior 层
- 执行前逻辑不得散落在单个工具实现内部
- 文件状态、权限状态、结果预算、工具决策必须进入统一 `ToolContext`
- 平台共性能力必须上收，不能在每个工具里重复发明一遍
- 不做长期兼容性框架分叉，迁移期顺序不等于双轨长期共存

### 3.3 实施原则

为了避免 Talos 在“对齐 Claude Code”的过程中又形成一套新的历史包袱，实施时必须遵守以下原则：

1. 大骨架不推翻，只增强
2. 新能力优先落平台层，不先塞进某个具体工具
3. 平台抽象一旦成立，后续工具必须复用
4. rollout 可以分阶段，但不引入长期兼容层
5. mock/最小实现能直接重写的，优先直接重写

换句话说：

- 保留现有工具主链路，不代表保留当前薄工具契约
- 保留 rollout 顺序，不代表接受旧语义长期并存
- 保留工具扩展性，不代表允许工具层自由生长出平台能力

### 3.4 文档细化工作流

本篇后续每次继续细化前，都必须先做源码对照探索。

最小要求：

- 先读 Claude Code 对应平台层或工具层源码
- 再读 Talos 当前相关实现
- 最后才更新 spec

这样做的原因：

- 平台层 spec 不能脱离 Talos 当前 `ToolRegistry / ToolRouter / RegistryOrchestrator / ToolHandler` 的实际边界
- 也不能只凭记忆描述 Claude Code 的工具契约
- 每一轮 refinement 都要回答“当前 Talos 已有什么骨架、缺什么治理、哪些能力可以直接上收”

---

## 4. 核心类型设计

### 4.1 目标接口伪代码

这里需要先明确一个 Rust 侧约束：

- **不能把 20+ 个行为点都压进一个 `dyn ToolBehavior` trait**

TypeScript 里可以用 class + 默认方法承载很多 hook，但在 Rust 里这会导致：

- 工具实现充满样板默认方法
- trait object 过厚，难以维护
- 很多静态描述信息被错误地放到动态分发层

因此本 spec 采用 **composition first** 的拆分方式。

#### 4.1.1 最小执行 trait

```rust
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    fn descriptor(&self) -> &ToolDescriptor;

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
        sink: Arc<dyn ToolProgressSink>,
    ) -> TalosResult<ToolExecutionResult>;

    fn runtime_behavior(
        &self,
        input: &Value,
        ctx: &ToolRuntimeContext,
    ) -> TalosResult<ToolRuntimeBehavior> {
        Ok(ToolRuntimeBehavior::default())
    }

    fn build_context_mutation(
        &self,
        input: &Value,
        result: &ToolExecutionResult,
        ctx: &ToolRuntimeContext,
    ) -> TalosResult<Option<ToolContextMutation>> {
        Ok(None)
    }
}
```

约束：

- trait 上只保留真正需要动态分发的执行能力
- 大部分静态治理/展示/权限描述进入 descriptor
- 少数输入相关动态判定统一收敛到 `runtime_behavior()`
- 上下文回写意图通过 `build_context_mutation()` 显式生成，而不是藏进结果结构体

这里还需要明确两套 context 的边界：

```rust
pub struct ToolExecutionContext {
    pub tool_context: Arc<ToolContext>,
    pub progress_sink: Arc<dyn ToolProgressSink>,
    pub abort_token: CancellationToken,
    pub runtime_handles: ToolRuntimeHandles,
}
```

```rust
pub struct ToolRuntimeContext {
    pub tool_context: Arc<ToolContext>,
    pub invocation_metadata: ToolInvocationMetadata,
}
```

约束：

- `ToolExecutionContext`
  面向真正执行阶段，允许访问 runtime handles、progress sink、abort token 等执行期能力
- `ToolRuntimeContext`
  面向治理判定和 mutation 构造，只提供轻量、只读、可序列化边界
- `runtime_behavior()` 与 `build_context_mutation()` 一律接收 `ToolRuntimeContext`
- 任何需要执行副作用或长生命周期句柄的逻辑，都不得塞进 `ToolRuntimeContext`

#### 4.1.2 Descriptor / Governance / Presentation 拆分

```rust
pub struct ToolDescriptor {
    pub identity: ToolIdentity,
    pub schema: ToolSchemaDescriptor,
    pub governance: ToolGovernanceDescriptor,
    pub presentation: ToolPresentationDescriptor,
    pub max_result_size_chars: usize,
    pub result_policy: ToolResultPolicy,
}
```

```rust
pub struct ToolSchemaDescriptor {
    pub description: ToolDescription,
    pub input_schema: serde_json::Value,
    pub output_schema: Option<serde_json::Value>,
    pub strict_mode: bool,
}
```

```rust
pub struct ToolGovernanceDescriptor {
    pub search_hint: Option<&'static str>,
    pub should_defer: bool,
    pub always_load: bool,
    pub defaults: ToolDefaults,
    pub validation: ValidationPolicy,
    pub permissions: PermissionPolicy,
}
```

```rust
pub struct ToolPresentationDescriptor {
    pub user_facing_name: &'static str,
    pub summarizer: ToolInvocationSummarizer,
    pub activity_kind: ToolActivityKind,
    pub classifier_projection: Option<ClassifierProjectionPolicy>,
}
```

```rust
pub struct ToolRuntimeBehavior {
    pub read_only: Option<bool>,
    pub destructive: Option<bool>,
    pub concurrency_safe: Option<bool>,
    pub interrupt_behavior: Option<ToolInterruptBehavior>,
    pub primary_path: Option<std::path::PathBuf>,
}
```

这里要强调：

- `ToolBehaviorFlags`
  是 catalog/spec 层默认值，必须是确定的封闭布尔值
- `ToolRuntimeBehavior`
  是运行时覆写层，只有工具确实需要按输入动态修正时才填写 `Some(...)`
- scheduler 的合并规则固定为：

```rust
let effective_read_only =
    runtime_behavior.read_only.unwrap_or(descriptor.governance.defaults.behavior_flags.read_only);
let effective_destructive = runtime_behavior
    .destructive
    .unwrap_or(descriptor.governance.defaults.behavior_flags.destructive);
let effective_concurrency_safe = runtime_behavior
    .concurrency_safe
    .unwrap_or(descriptor.governance.defaults.behavior_flags.concurrency_safe);
```

- 实现不得自发发明第二套 merge 顺序

这套拆分的核心目的：

- 让静态信息变成纯数据结构
- 让默认治理策略在 builder/descriptor 层一次定义
- 让动态 trait 只保留 `execute` 和少量运行时判定
- 避免每个工具 impl 大量 `fn xxx() -> default_value`
- 把 Claude Code 的 `maxResultSizeChars` 明确收敛成 descriptor 直接字段

### 4.1.1 命名约束

这里要特别强调：

- `public_name()` 是唯一公开工具名
- `internal_name()` 仅用于内部 registry/executor/logging
- 若存在 alias，只能是迁移兼容层，不进入长期主 catalog

建议新增：

```rust
pub struct ToolIdentity {
    pub public_name: &'static str,
    pub internal_name: &'static str,
    pub aliases: &'static [&'static str],
}
```

约束：

- `aliases` 默认为空
- 新工具禁止无理由新增 alias
- alias 不得在工具 catalog 中单独渲染

### 4.1.3 对齐 Claude Code 的平台扩展点

仅有 `name + schema + execute` 仍然不够，Talos 需要把 Claude Code 已经制度化的工具能力正式上升到平台层。

这里需要明确对齐的能力：

- `search_hint`
  用于 deferred tool 的关键词发现，不再把 ToolSearch 设计成独立旁路。
- `should_defer`
  定义工具默认不注入 prompt，只能通过 ToolSearch/动态 catalog 发现。
- `always_load`
  定义某些工具无论 catalog 多大都必须首轮可见。
- `inputs_equivalent`
  用于去重、幂等判断、并发批内合并和 UI 聚合。
- `is_destructive`
  把“有副作用”和“不可逆”拆开，不再只用 mutating 粗暴表示。
- `interrupt_behavior`
  明确新用户输入到来时是 `cancel` 还是 `block`。
- `backfill_observable_input`
  平台在把工具输入暴露给 transcript、hook、approval、telemetry 之前，可以补齐派生字段。
- `classifier_projection`
  为后续自动安全分类器预留强类型接口，但不要求在 Phase A 必须有完整消费者。

### 4.1.4 默认治理策略必须平台统一提供

Claude Code 的一个关键点不是“字段多”，而是这些字段的默认行为由平台集中提供，而不是让每个工具各自散落实现。

Talos 也需要引入统一默认策略，建议通过 `ToolSpecBuilder` 或 `ToolDefaults` 集中下发：

```rust
pub struct ToolDefaults {
    pub is_enabled: bool,
    pub is_read_only: bool,
    pub is_destructive: bool,
    pub is_concurrency_safe: bool,
    pub interrupt_behavior: ToolInterruptBehavior,
    pub should_defer: bool,
    pub always_load: bool,
    pub strict_mode: bool,
}
```

默认值建议：

- `is_enabled = true`
- `is_read_only = false`
- `is_destructive = false`
- `is_concurrency_safe = false`
- `interrupt_behavior = Block`
- `should_defer = false`
- `always_load = false`
- `strict_mode = false`

补充约束：

- 平台默认必须偏保守，不能把未知工具默认判成并发安全或只读。
- 任何 mutating/network/process/tool wrapper 只要没有显式覆写，就按需要 permission review 处理。
- `check_permissions()` 若工具未实现，必须回落到统一 permission engine，而不是直接放行。
- `prepare_permission_matcher()` 若工具未实现，只能匹配到 tool-name 级别，不允许伪装成更细粒度规则。
- 任何跨多个工具都会重复出现的治理逻辑，都不得继续留在单个工具实现里。

### 4.1.5 结果对象要从“字符串返回”升级成协议对象

Claude Code 的 `ToolResult<T>` 已经把数据、上下文修改、MCP 元数据分开。Talos 也需要建立对应的强类型协议。

```rust
pub struct ToolExecutionResult {
    pub payload: ToolPayload,
    pub appended_messages: Vec<ToolContextMessage>,
    pub persisted_result: Option<ToolStoredResultRef>,
    pub protocol_metadata: Option<ToolProtocolMetadata>,
}
```

```rust
pub struct ToolProtocolMetadata {
    pub structured_content: Option<serde_json::Value>,
    pub extra_meta: Option<serde_json::Value>,
}
```

约束：

- `payload` 是模型面向结果，不得直接等同于 UI 渲染字符串。
- `appended_messages` 用于工具主动回写上下文，例如 plan/todo/agent 类工具。
- `persisted_result` 统一承载大输出文件引用，不允许各工具自行发明路径提示格式。
- `protocol_metadata` 用于 MCP/structured tool result 透传。

这里进一步收紧：

- `ToolExecutionResult` 不直接携带 `context_mutation`
- mutation 通过 `ToolExecutor::build_context_mutation()` 单独生成
- 这样可以把“执行结果”和“状态回写意图”明确拆开

### 4.2 模块接口表

| 模块 | 职责 | 关键对象 |
|---|---|---|
| `tool_catalog` | 维护 public/internal/alias/display mapping | `ToolCatalogEntry` |
| `tool_executor` | 定义最小动态执行 trait | `ToolExecutor`, `ToolRuntimeBehavior` |
| `tool_descriptor` | 承载静态 schema/governance/presentation 信息 | `ToolDescriptor` |
| `tool_defaults` | 集中提供默认治理策略 | `ToolDefaults`, `ToolSpecBuilder` |
| `tool_permissions` | 执行 read/write/network/host 权限判定 | `PermissionDecision` |
| `tool_scheduler` | 处理并发安全工具批次和串行工具执行 | `ToolBatch`, `ToolScheduler` |
| `tool_progress` | 统一进度事件 | `ToolProgressEvent`, `ToolProgressSink` |
| `tool_results` | 结果预算、落盘、预览、引用 | `ToolResultPolicy`, `ToolStoredResult` |
| `tool_result_mapper` | 区分模型结果、UI 摘要、检索文本 | `ToolResultMapper`, `ToolRenderSummary` |
| `tool_render` | 统一 progress/rejected/error 的等价渲染路径 | `ToolRenderMapper`, `ToolRenderMessage` |
| `tool_context` | 统一会话态、文件态、权限态 | `ToolContext`, `FileReadState` |

### 4.3 与当前 Talos 类型的演进关系

为了避免“新 spec 很完整，但落地时又绕开现有代码”，这里直接定义与当前类型的关系。

#### 4.3.1 `RegisteredToolSpec` 的目标演进

当前 `RegisteredToolSpec` 主要承载：

- `ToolDefinition`
- `ToolSource`
- `ToolPayloadKind`
- `ToolParallelism`
- `mutating`
- `governance`

后续应演进为：

```rust
pub struct RegisteredToolSpec {
    pub descriptor: ToolDescriptor,
    pub definition: ToolDefinition,
    pub source: ToolSource,
    pub payload_kind: ToolPayloadKind,
    pub behavior_flags: ToolBehaviorFlags,
    pub governance: ToolGovernanceSpec,
    pub result_policy: ToolResultPolicy,
}
```

```rust
pub struct ToolBehaviorFlags {
    pub read_only: bool,
    pub destructive: bool,
    pub concurrency_safe: bool,
    pub requires_user_interaction: bool,
}
```

原则：

- `mutating` 不再作为长期核心字段单独存在，应收敛到 `read_only/destructive` 等更细粒度语义
- `parallelism` 不再只靠静态枚举，保留为 catalog-level 默认值，运行时仍可被 `ToolExecutor.runtime_behavior()` 按输入覆写
- `result_policy` 必须进入 spec，而不是运行后临时拼装

#### 4.3.2 `ToolCatalog` 的目标演进

当前 `ToolCatalog.find(name)` 只按单个字符串查找。

后续需要支持：

- public name 查找
- alias 查找
- internal executor id 查找
- deferred tool 可见性过滤
- always-load 策略

建议演进为：

```rust
pub struct ToolCatalogEntry {
    pub spec: RegisteredToolSpec,
    pub visibility: ToolVisibility,
}
```

```rust
pub enum ToolLookupKey<'a> {
    PublicName(&'a str),
    InternalName(&'a str),
    Alias(&'a str),
}
```

```rust
pub enum ToolVisibility {
    AlwaysLoad,
    DefaultVisible,
    Deferred,
    Disabled,
}
```

#### 4.3.3 `ToolHandler` 的目标演进

当前 `ToolHandler` 更接近“按 source 派发并执行 invocation”。

后续需要升级成：

- 接收已治理过的 invocation
- 调用统一 `ToolExecutor.execute()`
- 回传 `ToolExecutionResult`
- 不在 handler 内重复做 permission/result governance

也就是说，`ToolHandler` 保留 source-specific dispatch 职责，但不再承担平台治理兜底逻辑。

---

## 5. ToolContext 扩展

### 5.1 当前问题

Talos 当前 `ToolContext` 只有：

- `session_id`
- `user_id`
- `project_root`
- `kernel_config`
- `memory_store`
- `collaboration_runtime`

这不足以支撑 Claude Code 风格系统工具。

### 5.2 目标字段

```rust
pub struct ToolContext {
    session_id: String,
    user_id: String,
    project_root: Option<PathBuf>,
    kernel_config: Option<Arc<AgentKernelConfig>>,
    messages_ref: Arc<dyn MessageHistoryView>,

    permission_context: Arc<ToolPermissionContext>,
    read_file_state: Arc<ReadFileStateStore>,
    result_budget_state: Arc<ToolResultBudgetState>,
    decision_state: Arc<ToolDecisionState>,
    tool_catalog_state: Arc<ToolCatalogState>,
    content_replacement_state: Arc<ContentReplacementState>,
    progress_sink: Option<Arc<dyn ToolProgressSink>>,
    abort_token: CancellationToken,

    collaboration_runtime: Option<Arc<CollaborationRuntime>>,
    memory_store: Option<Arc<MemoryStore>>,
    lsp_runtime: Option<Arc<LspRuntime>>,
    browser_runtime: Option<Arc<BrowserRuntime>>,
}
```

```rust
pub trait MessageHistoryView: Send + Sync {
    fn current_turn_messages(&self) -> &[Message];
    fn session_messages(&self, limit: usize) -> Vec<&Message>;
}
```

### 5.3 必须新增的上下文语义

- `read_file_state`
  用于 `Read -> Edit/Write/NotebookEdit` 的前置约束与 stale-write 检测

  这里明确为：

  - **session-scoped state**
  - 不是 per-turn 临时缓存
  - 每个文件记录最近一次成功读取的版本信息
  - 成功的 `Edit/Write/NotebookEdit` 会刷新对应记录
  - 每个 turn 可以额外派生一个 turn-local read set，但那只是调度辅助，不是主状态源

- `permission_context`
  用于统一 host/fs/network/mcp/tool-specific 规则匹配

- `messages_ref`
  用于工具只读访问历史消息、当前轮消息与必要的 transcript 视图，不允许工具直接修改消息流

- `result_budget_state`
  用于 per-tool 与 per-message 两级预算

- `decision_state`
  用于记录用户批准、拒绝、ask/allow/deny 历史

- `tool_catalog_state`
  用于承载 deferred tool、feature-gated tool、动态 MCP tool 的当前可见性快照

- `content_replacement_state`
  用于承载 aggregate result budget 下的大结果替换与引用状态，保证多工具结果裁剪在会话级一致

- `abort_token`
  用于并发工具取消与 sibling cancellation

### 5.4 `read_file_state` 的版本语义

`ReadFileStateStore` 需要记录的不是“读过没读过”这种布尔值，而是版本化状态：

```rust
pub struct ReadFileStateEntry {
    pub path: PathBuf,
    pub observed_at_turn: u64,
    pub observed_at_session_seq: u64,
    pub content_hash: Option<String>,
    pub mtime_unix_ms: Option<u64>,
    pub size_bytes: Option<u64>,
}
```

```rust
pub enum ToolContextMutation {
    RefreshReadFileState(ReadFileStateEntry),
    RecordToolDecision {
        tool_use_id: String,
        decision: ToolDecision,
    },
    UpdatePermissionMode(PermissionMode),
    UpdateResultBudget(ResultBudgetDelta),
    AppendTaskStateEntry(TaskStateEntry),
}
```

约束：

- `ToolContextMutation` 是封闭枚举，不允许开放式 JSON payload
- Phase A 起新增 mutation 类型必须显式扩展该枚举与 commit 逻辑
- 不允许工具绕开该枚举直接改写平台状态

写入工具的校验规则：

- 当前 session 里必须存在对应文件的最近读取记录
- 若文件当前版本与记录版本不一致，拒绝写入并提示重新 `Read`
- 若是本 session 内工具自己刚刚写过，则以刷新后的记录为准

这意味着：

- 它支持多轮对话持续编辑同一文件
- 也支持识别会话外/用户手动修改造成的 stale-write
- 不会把“上一轮读过”错误地当成永远有效

---

## 6. 权限与治理分层

Talos 当前已有 `approval_policy + sandbox_policy`，这层继续保留，但要补齐权限层。

### 6.1 目标分层

| 层 | 作用 | 示例 |
|---|---|---|
| Validation | 输入合法性 | path 是否存在，pages 参数是否合法 |
| Permission | 用户/配置允许与否 | 目录 deny、域名 ask、MCP server allowlist |
| Governance | sandbox/approval/escalation | `require_escalated`、workspace write |
| Runtime Guard | 执行期保护 | 设备文件拒绝、超大文件拒绝、危险命令拒绝 |

### 6.2 原则

- Validation 失败必须直接报错
- Permission 失败必须有明确拒绝原因
- Governance 负责“能不能申请更高权限”
- Runtime Guard 不允许静默吞掉错误

### 6.2.1 平台错误语义

Phase A 前必须统一定义以下错误语义：

```rust
pub enum ValidationResult {
    Pass,
    Reject {
        message: String,
        error_code: ToolErrorCode,
    },
}
```

```rust
pub enum PermissionDecision {
    Allow {
        updated_input: Option<Value>,
    },
    Ask {
        message: String,
        reason: PermissionReason,
        updated_input: Option<Value>,
    },
    Deny {
        message: String,
        reason: PermissionReason,
    },
    Passthrough,
}
```

约束：

- `ValidationResult::Reject` 表示输入/前置条件错误，不进入 approval UI
- `PermissionDecision::Ask` 表示需要用户交互确认
- `PermissionDecision::Deny` 表示策略层明确拒绝
- `Passthrough` 只用于交给通用 permission engine 继续判断，不表示默认放行

### 6.3 相比当前 Talos 还需增强的治理能力

#### 6.3.1 Tool-level permission matcher

Claude Code 不只是“这个工具能不能用”，而是“这个工具对这个输入能不能用”。

例如：

- `Read(/a/b/c)` 与 `Read(/secret/x)` 不是同一个权限决策
- `WebFetch(domain:github.com)` 与 `WebFetch(domain:internal.example.com)` 也不是

Talos 需要引入 `prepare_permission_matcher()` 层。

#### 6.3.2 Read-only / destructive / concurrency-safe 分层

Talos 当前只有 `mutating` 和 `parallelism` 粗粒度标记，还不够。

需要补齐：

- `is_read_only`
- `is_destructive`
- `is_concurrency_safe(input)`

并支持输入相关判定，而不是固定 tool name 判定。

#### 6.3.3 Interrupt behavior

Claude Code 平台层明确区分：

- 用户新消息到来时是否取消当前工具
- 当前工具是否 block 后续输入

Talos 也需要显式治理，而不是默认一律串行阻塞。

#### 6.3.4 Deferred tool governance

`shouldDefer` 不是工具搜索的小技巧，而是 catalog 治理能力：

- 哪些工具默认不展开
- 哪些工具必须 always-load
- MCP/feature-gated tools 如何进入 ToolSearch

Talos 当前这块还是空白。

#### 6.3.5 Result budget governance

当前 Talos 只有“单次结果截断”。

还需要：

- per-tool inline budget
- per-message aggregate budget
- persisted output path
- preview strategy
- UI / transcript / model-facing 内容一致性约束

### 6.4 默认治理必须集中，而不是散落在工具里

如果 Talos 继续让每个工具自己决定默认值，最终只会得到另一套更难维护的分散逻辑。

平台层需要集中定义：

- 哪些方法有默认值
- 默认值是否保守
- 哪些能力必须显式覆写
- 哪些能力可以从 descriptor 推导

建议规则：

- `validate_input()` 可以默认只做 schema-level validation
- `check_permissions()` 不能默认空实现，必须至少进入通用 permission engine
- `is_read_only()`、`is_concurrency_safe()`、`is_destructive()` 必须有平台默认值
- `interrupt_behavior()` 必须有默认值
- `result_policy()` 必须始终返回完整对象，禁止工具只返回部分字段

### 6.4.1 当前 Talos 里的三类硬编码默认值要被平台接管

根据当前实现，Talos 现在至少有三类“注册期硬编码默认值”：

- `default_parallelism(name)`
- `default_mutation(name)`
- `default_governance(name)`

这些逻辑目前按 tool name 分支写在 `ToolRegistry` 内，短期可用，但不适合继续扩展。

目标改造方式：

```text
today:
ToolRegistry::register()
  -> match tool.name()
  -> infer parallelism/mutating/governance

target:
ToolExecutor / ToolDescriptor
  -> expose defaults + behavior flags
  -> ToolSpecBuilder builds RegisteredToolSpec
  -> ToolRegistry stores normalized spec
```

约束：

- `ToolRegistry` 不再维护一组随工具数量增长的 `match name { ... }`
- 新工具不得依赖 registry 内部硬编码才能获得正确治理语义
- source 级默认值可以保留，但只能作为 `ToolSpecBuilder` 的兜底，而不是最终判定来源

### 6.5 Deferred / Always-load 是平台治理，不是 prompt 技巧

这部分需要直接对齐 Claude Code。

Talos 平台应明确：

- `should_defer = true`
  表示工具不进入默认 prompt catalog，只能通过 ToolSearch 或动态发现进入当前轮上下文。
- `always_load = true`
  表示工具必须在首轮就暴露给模型，即使系统开启了 deferred tool 策略。
- `should_defer` 与 `always_load` 不能同时为 `true`
- MCP 工具、feature-gated 工具、本地高级工具都应进入同一治理模型，而不是各自维护一套开关

### 6.6 平台规范优先于工具特例

本轮实施要明确一条硬规范：

- 如果某个能力预计会在两个及以上工具中出现，就必须先抽到平台层

典型例子：

- 文件工具的 stale-write 校验
- Bash/WebFetch 的 permission matching
- Grep/Glob/Read 的结果预览与预算控制
- MCP/Agent 工具的 appended messages 与 context mutation

禁止的实现方式：

- `Read` 自己维护一套 read-state，`Edit` 再维护另一套
- `Bash` 单独处理 persisted output，而 `WebFetch` 再复制一套
- `Grep` 自己定义 UI 摘要格式，`Glob` 再定义另一套不兼容摘要协议

允许的实现方式：

- 平台定义 `ReadFileStateStore`、`ToolResultPolicy`、`ToolResultMapper`
- 工具只负责声明自己的输入、特例判定和执行逻辑
- 通用治理与协议映射统一由平台承接

---

## 7. 并发调度语义

### 7.1 对齐目标

Talos 需要具备 Claude Code 风格的“只读批量并发，变更工具串行执行”能力。

### 7.2 调度原则

1. 工具自己申明 `is_read_only`
2. 工具自己申明 `is_concurrency_safe`
3. Scheduler 按批次分组

```text
[Read][Grep][Glob] -> one parallel batch
[Edit]             -> serial
[Read][Read]       -> next parallel batch
[Bash(mutating)]   -> serial
```

### 7.3 最低目标

- 只读工具可并发
- 变更工具默认串行
- 同一主文件写路径存在冲突时必须串行
- Shell 工具并发安全性取决于命令语义，而不是固定 tool name

### 7.4 上下文修改与并发批次的提交规则

Talos 需要把“工具执行完成”与“工具修改上下文”区分开。

建议约束：

- 并发安全工具允许并发执行，但它们的 `context_mutation` 不能无序提交
- 默认只有串行工具或批次 barrier 后才能提交上下文修改
- 若工具声明 `is_concurrency_safe = true` 且 `build_context_mutation()` 返回 mutation，scheduler 必须显式决定提交顺序
- 文件读状态、decision state、result budget state 属于平台状态，更新必须走统一 API

### 7.4.1 Mutation barrier 的具体实现

这里要避免给工具一个可直接写共享状态的 `Arc<DashMap>` 然后期待“自然有序”。

目标实现应为：

```rust
pub struct ToolExecutionResult {
    pub payload: ToolPayload,
    pub appended_messages: Vec<ToolContextMessage>,
    pub persisted_result: Option<ToolStoredResultRef>,
    pub protocol_metadata: Option<ToolProtocolMetadata>,
}
```

```rust
pub struct ToolBatchResult {
    pub executions: Vec<ToolExecutionResult>,
    pub pending_mutations: Vec<PendingContextMutation>,
}
```

提交规则：

1. 工具执行阶段只返回 `ToolExecutionResult`，**不直接写共享状态**
2. 平台随后调用 `build_context_mutation(input, result, ctx)` 生成 `PendingContextMutation`
3. `ToolScheduler` 收集整个批次的 `pending_mutations`
4. 到达 batch barrier 后，由 scheduler 串行调用 `commit_mutation()`
5. `ReadFileStateStore`、`ToolDecisionState`、`ToolResultBudgetState` 等状态更新只能发生在 commit 阶段

这意味着：

- 并发批次内不会出现无序写共享状态
- 平台可以精确定义提交顺序
- 出错时可以丢弃整批未提交 mutation，而不是回滚散落状态
- 若 `build_context_mutation()` 返回 `None`，scheduler 直接跳过 pending queue，不生成空 `PendingContextMutation`

### 7.5 调度器与现有 `ToolParallelism` 的关系

Talos 当前 catalog 已经有 `ToolParallelism::{SerialOnly, ParallelSafe}`，这层不需要删除，但需要降级为“初始声明”。

后续规则应改为：

- `RegisteredToolSpec.behavior_flags.concurrency_safe`
  表示 catalog-level 默认并发属性
- `ToolExecutor.runtime_behavior(input, ctx)`
  产出 runtime-level 最终判定
- `ToolScheduler`
  以 runtime-level 判定为准分批

其中 runtime 与 default 的最终合并规则必须固定为：

```rust
let effective_behavior = EffectiveToolBehavior {
    read_only: runtime_behavior
        .read_only
        .unwrap_or(registered_spec.behavior_flags.read_only),
    destructive: runtime_behavior
        .destructive
        .unwrap_or(registered_spec.behavior_flags.destructive),
    concurrency_safe: runtime_behavior
        .concurrency_safe
        .unwrap_or(registered_spec.behavior_flags.concurrency_safe),
    interrupt_behavior: runtime_behavior
        .interrupt_behavior
        .unwrap_or(registered_spec.governance.defaults.interrupt_behavior),
    primary_path: runtime_behavior.primary_path,
};
```

这样可以兼容两类需求：

- `Read/Grep/Glob` 这类稳定只读工具可默认并发
- `Bash` 这类工具必须按输入动态判定

---

## 8. 结果治理

### 8.1 当前问题

Talos 现在主要做按字节截断，不足以支撑大输出工具。

### 8.2 目标结果策略

| 能力 | 说明 |
|---|---|
| Inline Result | 小结果直接回模型 |
| Persisted Result | 大结果落盘，模型看到预览 + 路径引用 |
| Aggregate Budget | 同一轮多个工具结果要有总预算 |
| Structured Result | 尽量保留结构化字段，不只拼字符串 |
| Preview Summary | UI 与模型看到的预览必须统一 |

### 8.3 统一策略对象

```rust
pub struct ToolResultPolicy {
    pub max_inline_bytes: usize,
    pub max_inline_tokens: usize,
    pub persist_large_output: bool,
    pub aggregate_budget_scope: AggregateBudgetScope,
    pub preview_lines: usize,
    pub preserve_structured_content: bool,
}
```

```rust
pub enum AggregateBudgetScope {
    Message,
    Turn,
    Session,
}
```

### 8.4 模型结果、UI 展示、检索文本必须分层

Claude Code 已经把这些能力拆开，Talos 也不能再让一个字符串同时承担三种责任。

平台至少需要三类输出：

```rust
pub trait ToolResultMapper {
    fn map_for_model(&self, result: &ToolExecutionResult) -> TalosResult<ModelToolResult>;
    fn summarize_for_ui(&self, result: &ToolExecutionResult) -> TalosResult<ToolRenderSummary>;
    fn extract_search_text(&self, result: &ToolExecutionResult) -> TalosResult<Option<String>>;
}
```

约束：

- `map_for_model()` 面向 LLM 协议，负责 inline preview、persisted output reference、structured content。
- `summarize_for_ui()` 面向 transcript / TUI / Web 展示，不得依赖模型协议字符串反向解析。
- `extract_search_text()` 面向全文索引和 transcript search，不允许凭空生成用户看不到的文字。

### 8.4.1 TUI / Transcript 等价渲染路径

Claude Code 除了最终结果映射，还显式区分了至少三类工具渲染路径：

- `renderToolUseProgressMessage`
- `renderToolUseRejectedMessage`
- `renderToolUseErrorMessage`

Talos 也需要在平台层提供对应抽象，避免 TUI/Web 只能靠最终结果字符串反推状态。

建议新增：

```rust
pub trait ToolRenderMapper {
    fn render_progress(
        &self,
        event: &ToolProgressEvent,
    ) -> TalosResult<Option<ToolRenderMessage>>;

    fn render_rejected(
        &self,
        input: &Value,
        rejection: &ToolRejection,
    ) -> TalosResult<Option<ToolRenderMessage>>;

    fn render_error(
        &self,
        input: &Value,
        error: &ToolExecutionError,
    ) -> TalosResult<Option<ToolRenderMessage>>;
}
```

```rust
pub struct ToolRenderMessage {
    pub style: ToolRenderStyle,
    pub text: String,
    pub structured: Option<serde_json::Value>,
}
```

```rust
pub enum ToolRenderStyle {
    Progress,
    Rejected,
    Error,
    Summary,
}
```

约束：

- `render_progress()` 对应运行中进度块
- `render_rejected()` 对应 permission deny / user reject / validation reject 的 UI 等价路径
- `render_error()` 对应运行期错误展示
- TUI / Web / transcript 统一消费平台 render message，而不是每层单独发明格式

### 8.4.2 `ToolProgressSink` 与现有 TUI/SSE 的接线

Phase A 前必须明确进度事件不直接耦合某个 UI。

建议边界：

```rust
pub trait ToolProgressSink: Send + Sync {
    fn emit(&self, event: ToolProgressEvent) -> TalosResult<()>;
}
```

```rust
pub enum ToolProgressEvent {
    Started { tool_use_id: String, label: String },
    Update { tool_use_id: String, message: String },
    Completed { tool_use_id: String },
    Failed { tool_use_id: String, message: String },
}
```

接线方式：

- `ToolProgressSink` 只负责产出平台事件
- `agent-kernel` 把平台事件转换为统一 `KernelStreamEvent`
- SSE/TUI/Web 继续消费现有流事件，不直接依赖 tool runtime

这样可以保证：

- 工具平台不直接感知渲染层
- 现有 TUI/SSE 主链路不需要为每个工具单独接线

### 8.4.3 `classifier_projection` 的下游消费者

这项能力先定义边界，后续再渐进接入。

当前约束：

- Phase A 不要求必须有完整自动分类器
- `classifier_projection` 先作为可选字段进入 `ToolPresentationDescriptor`
- 只有在后续引入 auto-mode security classifier 时，才读取这部分数据
- 若当前无人消费，默认可为 `None`，不阻塞平台层落地

### 8.5 结果治理必须一次抽象到位

这一层尤其不能走“先让每个工具各做一版，后面再统一”的路线。

原因很直接：

- 一旦 `Read`、`Grep`、`Bash`、`WebFetch` 各自产生不同的 preview/persist/reference 规则
- 后续再统一时，测试、prompt、UI、transcript 全部会被牵连

所以结果治理需要一次性在平台层定下：

- persisted output 的引用格式
- preview 的裁剪策略
- aggregate budget 的统计边界
- structured content 的保留方式
- transcript searchable text 的抽取规范

---

## 9. Prompt 与 Tool Catalog 集成

工具平台改造后，PromptBuilder 也要同步调整：

1. 使用 External Tool Name，而不是内部 executor 名
2. 工具描述必须包含字段级 schema 描述
3. 对特性工具与 deferred 工具做条件注入
4. ToolSearch 进入 catalog 后，要显式说明“用于发现未展开工具”

### 9.1 PromptBuilder 与 ToolCatalog 的新边界

为了避免 prompt 侧再重复维护一套工具规则，边界必须明确：

- `ToolCatalog`
  负责决定哪些工具当前可见、可调用、是否 deferred、展示什么 public name
- `PromptBuilder`
  只消费 catalog 的 prompt-facing 视图，不自行推导工具别名、并发属性、治理策略

建议新增：

```rust
pub struct PromptToolView {
    pub public_name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub strict_mode: bool,
    pub is_deferred: bool,
}
```

PromptBuilder 只拿 `Vec<PromptToolView>`，而不是直接遍历内部 registry spec。

---

## 10. 与现有 Talos 模块的关系

| 现有模块 | 处理方式 |
|---|---|
| `ToolRegistry` | 保留，升级成 public/internal/alias registry |
| `RegistryOrchestrator` | 保留，前置接入 validation/permission 结果 |
| `ToolHandler` | 保留，但从“仅调 execute”升级为“驱动 ToolExecutor 生命周期” |
| `PromptBuilder` | 调整工具目录渲染策略 |
| `agent-kernel` | 接入新的调度器与结果预算 |
| `talos-tools-sandbox` | 保留，继续承载 governance 层 |

### 10.1 现有骨架的接线改造顺序

建议按以下顺序接入，而不是多处同时改：

1. `ToolExecutor` / `ToolDescriptor` / `ToolDefaults` 与 `ToolScheduler`
   作为 Phase A 的同一个 milestone 并行设计，把统一契约、spec builder 与 mutation barrier 一起定下来，避免 `ToolExecutionResult` 和 commit 语义反复返工。
2. `RegisteredToolSpec` / `ToolCatalog`
   让 catalog 先能表达 public/internal/alias/deferred/result policy。
3. `ToolRegistry`
   改为注册 `ToolExecutor` 或从现有 `Tool` 适配生成 `ToolExecutor`。
4. `RegistryOrchestrator`
   在 approval/sandbox 之前接入 validation、permission、runtime behavior 判定。
5. `ToolHandler`
   收敛为 source dispatch + execute bridge。
6. `ToolResultMapper`
   最后统一模型结果、UI 摘要、persisted output 引用。

### 10.2 四个现有核心模块分别怎么变

#### `ToolRegistry`

保留职责：

- 注册工具
- 暴露 catalog
- 根据 invocation 找 handler

增强职责：

- 通过 `ToolSpecBuilder` 归一化 descriptor/defaults
- 维护 public/internal/alias 的查找索引
- 不再手写 name-based parallelism/mutation/governance 分支

#### `ToolRouter`

保留职责：

- 从原始 tool call 构造 `ToolInvocation`
- 校验 payload kind

增强职责：

- 将 alias / internal name 解析到 canonical public tool identity
- 为 deferred/MCP 工具提供统一 normalize 入口

#### `RegistryOrchestrator`

保留职责：

- 生成 execution plan
- 对接 approval / sandbox

增强职责：

- 在生成 plan 前执行 validation
- 结合 permission matcher 做 tool-input 级权限判定
- 读取 behavior flags 和 runtime 判定结果
- 输出 scheduler 可消费的治理结果

#### `ToolHandler`

保留职责：

- local / mcp / dynamic source dispatch

收缩职责：

- 不再承担 name-based governance fallback
- 不再各自决定结果落盘/预览规则
- 不再重复维护上下文回写逻辑

---

## 11. Phase A ADR 前置项

进入 Phase A 编码前，必须先完成一篇小型 ADR，至少钉死以下三件事：

1. `ToolExecutor` / `ToolDescriptor` / policy structs 的拆分方案
2. `ReadFileStateStore` 的 session-scoped 版本语义
3. `context_mutation` 的 batch barrier / commit 时机

## 12. 验收标准

- Talos 工具平台能承载 Claude Code 同类工具契约
- 所有核心工具都能接入统一 validation/permission/context/result 流程
- 不再新增“只会 execute 的裸工具”
- 后续新增工具必须进入该平台层，而不是绕过它直接注册
- 模型公开工具名收敛为单一 canonical name，alias 仅保留迁移期兼容
