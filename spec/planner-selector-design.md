# PlannerSelector 设计方案

**版本**: v5.0
**状态**: 已评审(Codex) 
**最后更新**: 2026-04-03

---

## 1. 背景与目标

### 1.1 问题

当前 planner 选择完全由 `AgentKernelConfig.planner_mode` 静态配置驱动：

```
config.planner_mode (BuiltIn | PlanReAct)
  → build_planner_for_request()   [agent.rs:435]
  → FlowProfile
```

同一 agent 面对不同性质的任务（问答 vs 单步分析 vs 多步执行）总是使用同一个 planner，效率或效果必然有一方受损。

adk-python 参考说明：其 planner 为 agent 初始化时静态配置，运行时不变，没有动态选择逻辑。Talos `PlannerSelector` 是在其架构之上新增的能力。

### 1.2 目标

- 新增 `PlannerSelection: Static | Auto` 配置，`Auto` 模式下由 LLM 在每个 turn 动态决定 planner。
- `Static` 为默认值，现有行为完全不变，零破坏。
- 删除 `planner_override` 运行时覆盖机制，所有场景统一走配置驱动。

---

## 2. 设计方案

### 2.1 整体流程

```
build_planner_for_request(ctx):
  ├─ planner_selection = Static → 直接使用 planner_mode 配置值
  └─ planner_selection = Auto   → PlannerSelector::select(PlannerSelectionRequest)
                                      ├─ status = Ok  → response.planner_kind
                                      └─ status = Err → planner_mode 配置值（fallback）
```

### 2.2 配置变更

新增 `PlannerSelection` 枚举，删除 `planner_override`，`planner_mode` 保留为 fallback：

```rust
pub enum PlannerSelection {
    #[default]
    Static,  // 固定使用 planner_mode 配置，不做推断
    Auto,    // LLM 动态推断，失败时 fallback 到 planner_mode
}

pub struct AgentKernelConfig {
    pub planner_mode: PlannerMode,           // 保留，Static 直接用；Auto 失败时 fallback
    pub planner_selection: PlannerSelection, // 新增，默认 Static
    // ...其他字段不变
}
```

TOML 配置示例：

```toml
[agent.kernel]
planner_mode      = "built_in"  # Static 直接用此值；Auto 失败时 fallback
planner_selection = "auto"      # 默认 static，显式开启动态推断
```

### 2.3 Selector 执行落点

Selector **不挂在 `AgentImpl` 外层装配阶段**，而是下沉到 kernel turn 入口，即 `LlmKernelAgent::execute_general_task()` 中 `FlowFactory::build()` 之前（`llm_agent.rs:96`）：

```rust
// llm_agent.rs — execute_general_task()
let planner = self.resolve_planner(ctx).await?;  // 新增，替换原 self.planner.clone()
let flow = FlowFactory::build(
    ...
    planner,   // 动态决定的 planner
);
```

`resolve_planner()` 逻辑：
```rust
async fn resolve_planner(&self, ctx: &InvocationContext) -> TalosResult<Arc<dyn Planner>> {
    if self.kernel_config.planner_selection == PlannerSelection::Static
        || !matches!(self.kernel_config.execution_mode, ExecutionMode::Single)
    {
        return Ok(make_planner(self.kernel_config.planner_mode));
    }
    let request = PlannerSelectionRequest::from_ctx(ctx, &self.tool_names());
    let response = self.planner_selector.select(request, self.kernel_config.planner_mode).await;
    // emit Planning event with PlannerSelection payload
    Ok(make_planner(response.planner_kind))
}
```

`LlmKernelAgent` 新增 `planner_selector: PlannerSelector` 字段，删除原有固定 `planner: Arc<dyn Planner>` 字段。

### 2.4 v1 生效边界

`Auto` 仅在 `execution_mode = Single` 时生效；`Sequential / Parallel / Loop` 模式下等同 `Static`。

原因：这些模式下 child agent 的实际 input 与顶层 input 可能不同，顶层 selector 判断不可靠。

### 2.4 PlannerKind

删除 `PlannerKind::None`，仅保留两种：

| PlannerKind | Planner 实现 | FlowProfile |
|---|---|---|
| `BuiltIn` | `BuiltInPlanner` | `ModelThinking`（有工具）/ `DirectAnswer`（无工具） |
| `PlanReAct` | `PlanReActPlanner` | `ReactiveLoop` |

`direct` 不作为独立输出项，无工具时由 FlowProfile 自然降级为 `DirectAnswer`。

### 2.5 通用 LLM 调用状态

放在 `llm-provider` 或 shared types crate，所有调模型的 response 可复用：

```rust
pub enum LlmCallStatus {
    Ok,
    Err {
        kind: LlmCallError,
        message: String,
    },
}

pub enum LlmCallError {
    ProviderError,    // LLM provider 调用异常
    EmptyResponse,    // 响应内容为空
    InvalidResponse,  // JSON 解析失败、字段缺失、未知 mode 等
    Timeout,          // 超时
}
```

### 2.6 PlannerSelector 结构

```rust
// crates/orchestration/agent/src/planner_selector.rs

pub struct PlannerSelectionRequest {
    pub input: String,
    pub recent_events: Vec<SessionEvent>,  // 最近 3 条 UserMessage/AgentMessage
    pub tool_names: Vec<String>,
}

// selector 返回值，同时作为 Planning 事件的 payload
pub struct PlannerSelectionResponse {
    pub planner_kind: PlannerKind,  // 失败时持有 planner_mode fallback 值
    pub latency_ms: u64,
    pub status: LlmCallStatus,
}

pub struct PlannerSelector {
    llm_provider: Arc<dyn LlmProvider>,
}

impl PlannerSelector {
    pub fn new(llm_provider: Arc<dyn LlmProvider>) -> Self {
        Self { llm_provider }
    }

    /// 失败时在 response.status 中记录原因，planner_kind 持有 fallback 值，不返回 Err。
    pub async fn select(
        &self,
        request: PlannerSelectionRequest,
        fallback: PlannerKind,
    ) -> PlannerSelectionResponse {
        let started = Instant::now();
        let result = self.call_llm(&request).await;
        let latency_ms = started.elapsed().as_millis() as u64;
        match result {
            Ok(kind) => PlannerSelectionResponse {
                planner_kind: kind,
                latency_ms,
                status: LlmCallStatus::Ok,
            },
            Err((error_kind, message)) => PlannerSelectionResponse {
                planner_kind: fallback,
                latency_ms,
                status: LlmCallStatus::Err { kind: error_kind, message },
            },
        }
    }
}
```

### 2.7 分类系统提示词（Classifier System Prompt）

```
You are a task routing assistant. Your sole job is to classify the user's request
into one of two execution modes and return a JSON object.

## Execution Modes

### builtin
Use when the task is conversational, analytical, or involves a bounded set of tool calls
where the outcome and required steps are clear upfront.

Includes:
- Pure Q&A, explanation, translation, summarization
- Reading a file and answering a question about it
- Running a single command and reporting the result
- Searching for information and returning it
- Analyzing provided content (code, logs, data)

### react
Use when the task requires multi-step planning where earlier results determine later steps,
or when the goal has inherent uncertainty about the execution path.

Includes:
- Implementing a new feature or module
- Refactoring or restructuring existing code
- Debugging and fixing an error (read → analyze → fix → verify cycle)
- Setting up pipelines, infrastructure, or multi-file changes
- Any task where "what to do next" depends on what was just done

## Decision Rules

1. If no tools are available: always return "builtin".
2. "Read file X and explain Y" → builtin (single read + analysis, clear outcome).
3. "Read file X and implement Y following the same pattern" → react (read informs multi-step impl).
4. "Fix bug Z" without further context → react (requires read → diagnose → patch → verify).
5. When uncertain between builtin and react: prefer "builtin" (lower overhead).

## Input

- `input`: the user's current message
- `recent_history`: last 1–3 conversation turns (user/assistant pairs, truncated to 200 chars each)
- `tools`: comma-separated list of available tool names, or "none"

## Output

Respond ONLY with a JSON object. No explanation, no markdown fences, no extra text.

{"mode": "builtin" | "react"}
```

### 2.8 调用实现

```rust
async fn call_llm(
    &self,
    request: &PlannerSelectionRequest,
) -> Result<PlannerKind, (LlmCallError, String)> {
    let user_content = build_user_content(request);
    let llm_request = ChatRequest {
        messages: vec![
            ChatMessage::system(CLASSIFIER_SYSTEM_PROMPT),
            ChatMessage::user(user_content),
        ],
        tools: vec![],
        stream: false,
        max_tokens: Some(32),
        temperature: Some(0.0),
        ..Default::default()
    };
    let response = self.llm_provider.chat(llm_request).await
        .map_err(|e| (LlmCallError::ProviderError, e.to_string()))?;

    let text = response.content_text()
        .ok_or_else(|| (LlmCallError::EmptyResponse, "no content".to_string()))?;

    let v: serde_json::Value = serde_json::from_str(text.trim())
        .map_err(|e| (LlmCallError::InvalidResponse, e.to_string()))?;

    match v["mode"].as_str() {
        Some("builtin") => Ok(PlannerKind::BuiltIn),
        Some("react")   => Ok(PlannerKind::PlanReAct),
        other => Err((
            LlmCallError::InvalidResponse,
            format!("unknown mode: {:?}", other),
        )),
    }
}
```

### 2.9 Observability

`PlannerSelectionResponse` 作为现有 `ExecutionEventKind::Planning` 的 payload，扩展已有 `EventPayload` 枚举（`execution.rs:152`），不新增事件类型：

```rust
// execution.rs 中扩展 EventPayload（现有命名）
pub enum EventPayload {
    // ... 现有变体不变 ...
    PlannerSelection(PlannerSelectionResponse),  // 新增
}
```

上报时机：`resolve_planner()` 完成 selector 调用后，在 `FlowFactory::build()` 之前通过 `ExecutionEventHandler` 发出。

---

## 3. 改动范围

| 文件 | 改动类型 | 说明 |
|------|---------|------|
| `agent/src/planner_selector.rs` | **新增** | `PlannerSelector`、`PlannerSelectionRequest`、`PlannerSelectionResponse`、系统提示词常量 |
| `agent/src/agent.rs` | **修改** | 删除 `planner_override`、`set_planner()`、`set_allow_request_planner_override()`；`build_planner_for_request()` 删除；`build_kernel_agent_for_request()` 改为 async |
| `agent/src/lib.rs` | **修改** | 导出 `planner_selector` 模块 |
| `config/src/lib.rs` | **修改** | 新增 `PlannerSelection` 枚举；`AgentKernelConfig` 加 `planner_selection` 字段；删除 `PlannerKind::None` |
| `llm-provider` 或 shared types | **新增** | `LlmCallStatus`、`LlmCallError` 通用枚举 |
| `agent-kernel/src/llm_agent.rs` | **修改** | `execute_general_task()` 新增 `resolve_planner()`；`LlmKernelAgent` 加 `planner_selector` 字段，删除固定 `planner` 字段 |
| `agent-kernel/src/execution.rs` | **修改** | `EventPayload` 枚举新增 `PlannerSelection(PlannerSelectionResponse)` 变体 |
| 受影响的测试 | **修改** | 删除 `planner_override` 相关测试；新增 mock `LlmProvider` 驱动的 selector 单测 |

---

## 4. 测试方案（替代 planner_override）

`planner_override` 删除后，测试通过以下方式覆盖：

### 单元测试：mock LlmProvider 驱动 selector

```rust
// mock provider 返回固定 JSON，驱动 selector 选择指定 planner
struct MockLlmProvider { mode: &'static str }
impl LlmProvider for MockLlmProvider {
    async fn chat(&self, _: ChatRequest) -> TalosResult<ChatResponse> {
        Ok(ChatResponse::text(format!(r#"{{"mode":"{}"}}"#, self.mode)))
    }
}

#[tokio::test]
async fn test_selector_returns_react() {
    let selector = PlannerSelector::new(Arc::new(MockLlmProvider { mode: "react" }));
    let resp = selector.select(mock_request(), PlannerKind::BuiltIn).await;
    assert_eq!(resp.planner_kind, PlannerKind::PlanReAct);
    assert!(matches!(resp.status, LlmCallStatus::Ok));
}
```

### 集成测试：planner_selection = Static

需要固定 planner 的场景（原 `planner_override` 用途）改用配置：

```toml
[agent.kernel]
planner_mode      = "plan_react"
planner_selection = "static"     # 固定使用 plan_react，不走 selector
```

### 可选：PlannerSelectorFactory trait 注入

若测试需要注入自定义 selector（如验证 fallback 行为），通过 factory trait 替换：

```rust
pub trait PlannerSelectorFactory: Send + Sync {
    fn create(&self, llm_provider: Arc<dyn LlmProvider>) -> Box<dyn PlannerSelectorPort>;
}
```

`LlmKernelAgent` 接受 `Arc<dyn PlannerSelectorFactory>`，测试时注入 mock factory。仅在单测需要精细控制时引入，默认走 `DefaultPlannerSelectorFactory`。

---

## 5. async 改造

`resolve_planner()` 是 async 函数，因此 `execute_general_task()` 本身已是 async，**无需额外改造外层调用链**。

`AgentImpl::build_kernel_agent_for_request()` 保持同步，`build_planner_for_request()` 方法删除（selector 已下沉到 `LlmKernelAgent` 内部）。

---

## 5. Benchmark 任务集（验收标准）

**固定回归集**（15 条，smoke test）：

| # | 输入 | 期望 | 类型 |
|---|------|------|------|
| 1 | "1+1 等于多少" | builtin | 纯问答 |
| 2 | "解释 Rust 的所有权模型" | builtin | 纯问答 |
| 3 | "把这段话翻译成英文：..." | builtin | 纯问答 |
| 4 | "读取 Cargo.toml 的内容" | builtin | 单步工具 |
| 5 | "cargo check 有没有报错" | builtin | 单步工具 |
| 6 | "搜索 tokio async runtime 的文档" | builtin | 单步工具 |
| 7 | "读这个文件，解释 foo 函数做了什么" | builtin | 边界：读+分析 |
| 8 | "帮我修一下这个 bug" | react | 边界：诊断→修复→验证 |
| 9 | "读这个文件，按同样的模式实现 bar 接口" | react | 边界：读+多步实现 |
| 10 | "帮我实现一个新的 REST 接口" | react | 多步实现 |
| 11 | "重构 agent.rs，把 planner 相关逻辑拆到单独模块" | react | 多文件重构 |
| 12 | "把数据库 schema 迁移到新版本" | react | 多步执行 |
| 13 | "搭建 CI pipeline" | react | 多步执行 |
| 14 | "运行测试，如果失败帮我修复" | react | 边界：条件分支多步 |
| 15 | "分析最近 5 次 commit 的改动" | builtin | 边界：多次读但结果确定 |

实现后需补充 **30-50 条扩展评估集**，目标误判率门禁 < 10%。

---

## 6. 编译与测试验证

1. `cargo build -p talos-agent` 编译无 warning
2. `cargo test -p talos-agent` 全量通过
3. `tests/integration/run_e2e.sh` 回归通过
4. 手动跑固定回归集 15 条，记录每条 `planner_kind` 结果

---

*最后更新：2026-04-03*
