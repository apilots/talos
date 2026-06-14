# Talos 架构评审问题整改方案

## 1. 背景

`spec/arch-review-2026-04.md` 对当前 Talos 架构进行了全量评审，指出了：

- planner 语义不实
- `SessionState` 结构重复
- `serde_json::Value` 过度使用
- 关键路径存在 `unwrap/expect`
- tools / kernel / memory 边界不够清晰
- 多处 placeholder / dead code 未完成

这份整改方案的目标，不是机械接受所有评审建议，而是将问题分为：

1. 必须立即整改的问题
2. 应该纳入近期重构的问题
3. 可以延后、避免过度工程化的问题

---

## 2. 评审结论校正

### 2.1 评审中成立的部分

以下问题判断基本成立：

1. `planner` 目前更多是 strategy / prompt shaping，而非真正 planner engine
2. `SessionState` 的五份 `HashMap` 已造成明显重复
3. 工具参数和若干运行时状态存在 `Value` 泛化过度
4. 关键路径 `unwrap/expect` 需要清理
5. transcript / session / memory 的共享类型边界不清晰

### 2.2 评审中表述过重的部分

以下问题需要校正措辞：

1. `agent-kernel` 与 `tools` 不是编译意义上的循环依赖，问题本质是边界过宽
2. `PlanReactLoop` 不是“完全没有实现”，而是“语义只实现了一半”
3. request/response pipeline 的重复是真实存在，但优先级低于状态模型和类型边界

---

## 3. 整改原则

1. 先收口语义，再收口类型，再做结构抽象
2. 保持 `general_turn` 为唯一主执行路径
3. 强类型优先，不继续扩散 `String` / `Value` 模式字段
4. 不为“架构漂亮”引入第二套执行链或过早泛型化
5. 所有新增默认行为和开关统一收敛到 `config`
6. 与 Plan-ReAct 方案的实施时序保持一致，避免先删后补

---

## 4. 优先级分层

### 4.1 P0: 立即整改

这些问题已经影响稳定性或会放大线上风险。

| 问题 | 现状 | 目标 |
|------|------|------|
| 关键路径 `unwrap/expect` | 网关、kernel、tools 关键路径仍可能 panic | 核心主路径不再因可恢复错误 panic |
| planner/config 语义不清 | `planner_mode` 使用字符串，语义漂移 | 改为强类型枚举，收口规划模式 |
| 工具参数 decode 分散 | 工具内部直接 `as_str().unwrap_or` | 参数校验前移到统一边界层 |

### 4.2 P1: 近期整改

这些问题已经明显影响可维护性，但不一定立即造成崩溃。

| 问题 | 现状 | 目标 |
|------|------|------|
| `SessionState` 五份 bag 重复 | 多处 `match namespace` | 改为统一 bag 结构 |
| transcript / runtime / persistence 模型耦合 | 改一个结构牵动三层 | 拆分 domain model 和 record model |
| kernel 对 `ToolRegistry` 依赖过宽 | 具体实现能力暴露过多 | 收窄为 definitions/executor 边界 |
| `FlowProfile` 混入 planner 语义 | flow profile 同时承载 flow 和 planner 含义 | 清理为只表达 flow 语义 |

### 4.3 P2: 延后整改

这些问题真实存在，但不应抢占前两档优先级。

| 问题 | 原因 |
|------|------|
| request/response pipeline 泛型化 | 收益有限，且两条链未来可能继续分化 |
| 大规模 dead code 清理 | 应配合模式收口一起做，避免误删未来要补的能力 |
| llm-router / reasoning / decision crate 占位清理 | 价值低于当前主路径语义与类型整改 |

---

## 5. 目标架构方向

### 5.1 总体路径

```text
P0: 稳定性与语义收口
    ├── panic 清理
    ├── planner_mode 强类型化
    └── tool input decode 前移

P1: 状态与边界收口
    ├── SessionState 统一化
    ├── transcript 模型拆层
    ├── ToolRegistry 依赖面收窄
    └── FlowProfile 语义清理

P2: 结构清理
    ├── placeholder 清理
    ├── pipeline 去重
    └── 历史命名和残留接口删除
```

### 5.2 不建议的路径

```text
不要这样做：

发现 planner 语义不清
    └── 直接新建第二套 Plan-ReAct executor

发现 pipeline 有重复
    └── 先做泛型化，再回头修状态模型

发现 Value 过多
    └── 一次性重写全部工具和 session schema
```

这些做法会让整改成本失控，并且偏离当前最关键的主路径问题。

---

## 6. 重点整改项设计

### 6.1 P0-1: 清理关键路径 panic

#### 范围

- `gateway/http`
- `orchestration/agent-kernel`
- `action/tools`

#### 原则

1. 可恢复错误必须返回 `TalosError`
2. 只允许在测试、不可达分支、进程启动时使用 `expect`
3. error message 必须包含上下文

#### 目标流程

```text
serialize SSE event
   │
   ├── success -> send event
   └── failure -> log + return structured internal error

create workspace dir
   │
   ├── success -> continue
   └── failure -> TalosError::InitError

create temp file
   │
   ├── success -> continue
   └── failure -> ToolError
```

### 6.2 P0-2: planner/config 强类型化

#### 现状问题

当前类似字段仍为字符串：

- `planner_mode`
- `execution_mode`
- `database_type`

这与项目“枚举不能用 String”的约束冲突。

#### 目标设计

```rust
pub enum PlannerMode {
    None,
    BuiltIn,
    PlanReAct,
}

pub enum ExecutionMode {
    Single,
    Auto,
    Sequential,
    Parallel,
    Loop,
}

pub enum DatabaseType {
    Sqlite,
    Postgres,
}
```

这里明确不将 `A2A` 作为 `ExecutionMode` 变体。  
`A2A` 在 Talos 中应被视为协作协议或工具能力，而不是通用任务主路径上的 runtime execution mode；否则会重新把协作 concern 混入主执行模式。

#### 收益

1. 配置语义稳定
2. 运行时分支更清晰
3. 非法值在加载期失败，而不是执行期漂移

### 6.3 P0-3: 工具参数 decode 前移

#### 现状问题

当前模式近似为：

```text
ToolInvocation.arguments: Value
    -> 各工具内部自行 get/as_str/unwrap
```

这会导致：

1. 校验逻辑分散
2. 错误文案不一致
3. panic 风险扩散

#### 目标设计

```text
model tool call
   │
   ▼
ToolInvocation
   │
   ▼
serde_json::from_value::<TInput>
   │
   ├── decode success -> typed input
   └── decode failure -> ToolValidationError
   ▼
ToolHandler::execute_typed(input)
```

#### 核心接口

```rust
pub trait TypedToolHandler: Send + Sync {
    type Input: serde::de::DeserializeOwned;
    type Output;

    fn definition(&self) -> ToolDefinition;

    async fn execute_typed(
        &self,
        input: Self::Input,
        context: ToolContext,
    ) -> TalosResult<Self::Output>;
}
```

统一 decode 逻辑直接走 `serde_json::from_value::<Self::Input>(args)`，不再单独引入 decoder 对象。  
这里不要求第一阶段把所有工具都改完，但新工具必须走 typed decode，旧工具逐步迁移。

### 6.4 P1-1: 统一 `SessionState`

#### 目标结构

```rust
pub struct SessionState {
    pub bags: BTreeMap<StateNamespace, BTreeMap<String, serde_json::Value>>,
}
```

#### 流程图

```text
StateDelta
   │
   ▼
SessionState::apply(delta)
   │
   ├── locate namespace bag
   ├── upsert key/value
   └── expose read helpers
```

#### 建议接口

```rust
impl SessionState {
    pub fn get(&self, ns: StateNamespace, key: &str) -> Option<&Value>;
    pub fn insert(&mut self, ns: StateNamespace, key: String, value: Value);
    pub fn apply_delta(&mut self, delta: &StateDelta);
    pub fn bag(&self, ns: StateNamespace) -> Option<&BTreeMap<String, Value>>;
}
```

#### 取舍

这里先保留 `Value` 作为 state bag value，是务实选择。  
状态 bag 的统一化优先级高于“立刻把所有 state value 强类型化”。

### 6.5 P1-2: transcript / memory / session 拆层

#### 现状问题

当前 `SessionTranscriptRecord` 等类型横跨：

- memory 持久化
- session service
- agent-kernel processor

这意味着存储模型污染运行时模型。

#### 目标分层

```text
Runtime Domain Model
    │  InvocationSession / SessionEvent / EventActions
    ▼
Mapper Layer
    │
    ▼
Persistence Record Model
       SessionTranscriptRecord / WorkspaceMemoryRecord
```

#### crate 归属

建议 mapper 层放在 `cognition/memory` crate 内部，新建独立模块，例如：

```text
crates/cognition/memory/src/
├── store.rs
├── session_store.rs
├── workspace_store.rs
└── mapper/
    ├── mod.rs
    ├── transcript.rs
    └── workspace.rs
```

原因是：

1. record model 的拥有者本来就是 memory crate
2. mapper 的主要职责是 runtime model 与 persistence record 之间的转换
3. 若把 mapper 放到 agent-kernel，会让存储表示反向渗透回 runtime 层

#### 模块接口表

| 模块 | 输入 | 输出 |
|------|------|------|
| `agent-kernel/session` | runtime model | runtime model |
| `memory/store` | record model | record model |
| `memory/mapper` | runtime <-> record | convert helpers |

### 6.6 P1-3: 清理 `FlowProfile` 语义

#### 现状问题

当前 `FlowProfile` 同时承载了：

1. flow 行为差异
2. planner 差异
3. reasoning 风格差异

这与 Plan-ReAct 方案中“planner mode 和 flow 语义分离”的方向不一致。

#### 目标方向

```text
FlowProfile
    只表达 flow 语义

PlannerMode
    表达 planning strategy

ExecutionBranch
    表达当前轮分支
```

#### 时序约束

这项工作应与 Plan-ReAct 文档的 Phase 1 对齐进入 P1。  
先完成 planner mode 收口和 loop policy 设计，再清理 `FlowProfile` 的混合语义，避免在旧命名上继续扩散实现。

### 6.6 P1-3: 收窄 `ToolRegistry` 依赖面

#### 目标

不要求删除 `ToolRegistry`，而是限制 kernel 看见的只是它需要的能力。

#### 建议边界

```rust
pub trait ToolDefinitionProvider: Send + Sync {
    fn get_tool_definitions(&self) -> Vec<ToolDefinition>;
}

pub trait ToolExecutorPort: Send + Sync {
    async fn execute_batch(
        &self,
        invocations: &[ToolInvocation],
        context: ToolContext,
    ) -> TalosResult<Vec<ToolDispatchOutcome>>;
}
```

#### 原则

1. `PromptBuilder` 只依赖 `ToolDefinitionProvider`
2. `general_turn` / `flow` 只依赖 `ToolExecutorPort`
3. `ToolRegistry` 作为 facade 同时实现上述 trait

---

## 7. 核心 API 伪代码

### 7.1 panic 清理

```rust
fn serialize_sse_event(event: &TalosStreamEvent) -> TalosResult<String> {
    serde_json::to_string(event)
        .map_err(|err| TalosError::Internal(format!("serialize sse event failed: {err}")))
}
```

### 7.2 SessionState 统一化

```rust
fn stage_session_event(ctx: &mut InvocationContext, event: &SessionEvent) {
    for delta in &event.actions.state_delta {
        ctx.session.state.apply_delta(delta);
    }
}
```

### 7.3 工具参数 typed decode

```rust
pub struct ReadFileInput {
    pub path: String,
}

impl ToolInputDecoder<ReadFileInput> for ReadFileDecoder {
    fn decode(&self, value: Value) -> TalosResult<ReadFileInput> {
        serde_json::from_value(value)
            .map_err(|err| TalosError::ToolError(format!("invalid read_file args: {err}")))
    }
}
```

---

## 8. 分阶段实施建议

### Phase 1: 稳定性与语义收口

1. 清理 P0 panic
2. 引入 `PlannerMode` / `ExecutionMode` 强类型枚举
3. 为新增工具建立 typed decode 入口

### Phase 2: 状态与边界收口

1. 重构 `SessionState`
2. 引入 transcript mapper 层
3. 收窄 kernel 对 `ToolRegistry` 的依赖面
4. 清理 `FlowProfile` 的混合语义

### Phase 3: 清理残留和次级重构

1. 删除确认无用的 dead code / placeholder
2. 调整历史命名
3. 视收益决定是否合并 pipeline 重复实现

其中 `flow.planner` 的 `dead_code` 字段不应在 Plan-ReAct 能力补全之前直接删除。  
正确时序应为：

1. 先按 Plan-ReAct 方案落地 planner request/response effect 和 loop policy
2. 若字段被真实使用，则移除 `dead_code` 标注
3. 若实现完成后仍无用途，再在 Phase 3 删除字段

---

## 9. 测试策略

### 9.1 单元测试

1. `SessionState::apply_delta`
2. `PlannerMode` / config 反序列化
3. tool input decode error path
4. mapper 层 runtime <-> record roundtrip

### 9.2 集成测试

1. SSE 事件序列化错误不会 panic
2. workspace 初始化失败会返回结构化错误
3. tool 参数非法时返回 tool validation error
4. session state delta 在多 namespace 下正确持久化

### 9.3 E2E 回归

1. `talos-e2e/run_e2e.sh`
2. general turn with tools
3. plan-react mode
4. session rewind / transcript persistence

---

## 10. 不做事项

以下事项明确不在本轮整改范围内：

1. 不一次性将所有 state value 全部强类型化
2. 不立刻实现完整 llm-router failover
3. 不为了去重而先改 request/response pipeline 为泛型
4. 不把所有占位 crate 在本轮全部补完

---

## 11. 结论

这次架构评审暴露的核心问题不是“Talos 主链路不可用”，而是：

- 语义层命名不准
- 类型边界过松
- 状态模型重复
- 若干关键路径仍缺少工程化收口

因此整改顺序应为：

1. 先修稳定性和强类型配置
2. 再修状态与模块边界
3. 最后再做清理型重构

这样既能快速降低风险，也不会把当前架构演进成一场大规模、低收益的重写。
