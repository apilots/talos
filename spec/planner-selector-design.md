# PlannerSelector 设计方案

**版本**: v1.0
**状态**: 待评审
**最后更新**: 2026-04-03

---

## 1. 背景与目标

### 1.1 问题

当前 planner 选择完全由 `AgentKernelConfig.planner_mode` 静态配置驱动：

```
config.planner_mode (BuiltIn | PlanReAct | None)
  → build_planner_for_request()   [agent.rs:435]
  → FlowProfile
```

这带来两个问题：
1. 同一 agent 面对不同性质的任务（聊天 vs 分析 vs 多步执行）总是使用同一个 planner，效率或效果必然有一方受损。
2. 用户/开发者需要手动感知并切换配置，违背 LLM-first 设计原则。

### 1.2 目标

- 完全由 LLM 在每个 turn 开始时动态决定使用哪个 planner。
- 从配置体系中删除 `PlannerMode`，不再作为 `AgentKernelConfig` 的字段。
- `planner_override`（运行时强制指定）保留，用于测试和集成场景。

---

## 2. 设计方案

### 2.1 整体流程变化

```
旧：
  config.planner_mode
    → build_planner_for_request()

新：
  PlannerSelector::select(input, history, tools)
    → PlannerKind
    → build_planner_for_request()
```

`PlannerSelector` 放在 `crates/orchestration/agent/src/planner_selector.rs`，作为 `DefaultAgent` 的一个字段。

### 2.2 PlannerKind 映射

| LLM 返回 | Planner 实现 | FlowProfile |
|---|---|---|
| `direct` | `BuiltInPlanner`（无工具） | `DirectAnswer` |
| `builtin` | `BuiltInPlanner` | `ModelThinking` |
| `react` | `PlanReActPlanner` | `ReactiveLoop` |

> `BuiltInPlanner` 复用现有实现；`PlannerKind::None` 保留仅用于 `planner_override` 场景。

### 2.3 PlannerSelector 结构

```rust
// crates/orchestration/agent/src/planner_selector.rs

pub struct PlannerSelector {
    llm_provider: Arc<dyn LlmProvider>,
}

impl PlannerSelector {
    pub fn new(llm_provider: Arc<dyn LlmProvider>) -> Self {
        Self { llm_provider }
    }

    /// 根据当前 input、最近 session 历史、可用工具列表，调用 LLM 选择 planner。
    /// 失败时返回 PlannerKind::BuiltIn（保守兜底）。
    pub async fn select(
        &self,
        input: &str,
        recent_events: &[SessionEvent],   // 取最近 3 条 AgentMessage/UserMessage
        tool_names: &[String],            // 已注册工具名列表
    ) -> PlannerKind {
        let prompt = build_classifier_prompt(input, recent_events, tool_names);
        match self.call_llm(prompt).await {
            Ok(kind) => kind,
            Err(_) => PlannerKind::BuiltIn,
        }
    }
}
```

### 2.4 分类系统提示词（Classifier System Prompt）

```
You are a task routing assistant. Your sole job is to classify the user's request
into one of three execution modes and return a JSON object.

## Execution Modes

### direct
Pure conversational response. No tools needed, no file access required.
Examples: explaining concepts, answering factual questions, translating text,
summarizing provided content, discussing ideas.

### builtin
Single-step or shallow analysis that may involve one or a few tool calls.
The outcome is clear and bounded — read a file, run a command, search the web.
Examples: "check if this compiles", "read config.toml", "what does this function do?",
"run the tests", "search for X".

### react
Multi-step planning and execution. The goal requires decomposing into subtasks,
with tool results informing next steps. There is uncertainty or branching in the path.
Examples: "implement feature X", "refactor this module", "debug and fix this error",
"set up the CI pipeline", "migrate the database schema".

## Decision Rules

- If no tools are registered: always return "direct".
- If the request is a question about existing content (code, docs, data already provided
  in context): prefer "builtin" over "react".
- If the request implies sequential actions where earlier results determine later steps:
  prefer "react".
- When uncertain between "builtin" and "react", prefer "builtin" (less overhead).

## Input

You will receive:
- `input`: the user's current message
- `recent_history`: the last 1–3 conversation turns (user + assistant pairs)
- `tools`: list of available tool names

## Output

Respond ONLY with a JSON object. No explanation, no markdown, no extra text.

{
  "mode": "direct" | "builtin" | "react"
}
```

### 2.5 调用实现

```rust
async fn call_llm(&self, prompt: ClassifierPrompt) -> TalosResult<PlannerKind> {
    let request = ChatRequest {
        model: prompt.model,
        messages: vec![
            ChatMessage::system(CLASSIFIER_SYSTEM_PROMPT),
            ChatMessage::user(prompt.user_content),
        ],
        tools: vec![],
        stream: false,
        max_tokens: Some(32),   // 只需要 JSON 小对象
        temperature: Some(0.0), // 确定性输出
        ..Default::default()
    };
    let response = self.llm_provider.chat(request).await?;
    parse_mode_from_response(&response)
}

fn parse_mode_from_response(response: &ChatResponse) -> TalosResult<PlannerKind> {
    let text = response.content_text().ok_or(TalosError::unexpected("empty response"))?;
    let v: serde_json::Value = serde_json::from_str(text.trim())
        .map_err(|_| TalosError::unexpected("invalid json from classifier"))?;
    match v["mode"].as_str() {
        Some("direct")  => Ok(PlannerKind::Direct),
        Some("builtin") => Ok(PlannerKind::BuiltIn),
        Some("react")   => Ok(PlannerKind::PlanReAct),
        _               => Err(TalosError::unexpected("unknown mode")),
    }
}
```

### 2.6 User Content 构造

```rust
fn build_user_content(
    input: &str,
    recent_events: &[SessionEvent],
    tool_names: &[String],
) -> String {
    let history: Vec<String> = recent_events
        .iter()
        .filter_map(|e| match &e.payload {
            SessionEventPayload::Message { message } => {
                let role = match message.role {
                    Role::User      => "user",
                    Role::Assistant => "assistant",
                    _               => return None,
                };
                Some(format!("{}: {}", role, truncate(message.text_content(), 200)))
            }
            _ => None,
        })
        .collect();

    let tools_str = if tool_names.is_empty() {
        "none".to_string()
    } else {
        tool_names.join(", ")
    };

    format!(
        "tools: {}\n\nrecent_history:\n{}\n\ninput: {}",
        tools_str,
        history.join("\n"),
        input
    )
}
```

---

## 3. 改动范围

| 文件 | 改动类型 | 说明 |
|------|---------|------|
| `agent/src/planner_selector.rs` | **新增** | `PlannerSelector` 实现、系统提示词常量 |
| `agent/src/agent.rs` | **修改** | `build_planner_for_request()` 改为 `async`，调用 selector；`DefaultAgent` 加 `planner_selector` 字段；`new()` 初始化 selector |
| `agent/src/lib.rs` | **修改** | 导出 `planner_selector` 模块 |
| `config/src/lib.rs` | **修改** | 删除 `PlannerMode` 枚举、`AgentKernelConfig.planner_mode` 字段及相关 `default_kernel_planner_mode()` 函数 |
| `general_turn.rs` | **修改** | 删除从 `kernel_config.planner_mode` 读取的调用（改由已选定的 planner 传入，不变） |
| 受影响的测试 | **修改** | 删除 `set_default_planner_mode` 相关测试；替换为 mock selector 测试 |

> `planner_override` 字段和 `set_planner()` 方法保留，`PlannerSelector::select()` 只在 `planner_override` 未设置时调用。

---

## 4. 对 build_kernel_agent_for_request 的影响

当前 `build_kernel_agent_for_request()` 是同步函数，内部调用同步的 `build_planner_for_request()`。

修改后需要变为 `async`：

```rust
async fn build_kernel_agent_for_request(
    &self,
    ctx: &InvocationContext,
) -> TalosResult<Arc<dyn KernelAgent>> {
    let planner = self.build_planner_for_request(ctx).await?;
    // 其余逻辑不变
}
```

调用链：`run()` → `build_kernel_agent_for_request()` 已在 async 上下文中，无需额外改动。

---

## 5. 配置迁移

`AgentKernelConfig` 删除 `planner_mode` 字段后，已有 TOML 配置文件中的 `agent.kernel.planner_mode` 字段将被忽略（serde 默认行为），无需强制迁移。

---

## 6. 验证

1. `cargo build -p talos-agent` 编译无 warning
2. `cargo test -p talos-agent` 全量通过
3. 集成测试：发送三类不同请求，日志确认 `PlannerSelector` 返回值符合预期：
   - `"1+1等于多少"` → `direct`
   - `"读取 Cargo.toml 的内容"` → `builtin`
   - `"帮我实现一个新的 REST 接口"` → `react`

---

*最后更新：2026-04-03*
