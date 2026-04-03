# Talos Plan-ReAct 细化设计方案

## 1. 背景

当前 Talos 已具备统一的 `general_turn` 工具调用闭环，也已引入：

- `PlannerKind::BuiltIn`
- `PlannerKind::PlanReact`
- `FlowProfile::BuiltInDirect`
- `FlowProfile::BuiltInModelThinking`
- `FlowProfile::PlanReactLoop`

但从当前实际实现看，`PlanReactLoop` 还不是一个语义完整的 Plan-Re-Act 执行方案，主要仍体现为：

1. request 阶段追加 planning instruction
2. response 阶段按 tag 处理 reasoning / final answer
3. 工具执行仍复用统一 `general_turn` 主路径

这与 `adk-python` 的真实实现高度相似。ADK 的 planner 也不是独立执行引擎，而是：

- 在 request processor 中修改 LLM request
- 在 response processor 中整形模型响应
- 由统一 runner / flow 维持 tool-call -> tool-result -> next-turn 的闭环

因此，Talos 不应再把 Plan-Re-Act 设计成“第二条独立执行链”，而应在统一 `general_turn` 主路径上补足其控制语义。

---

## 2. 设计目标与非目标

### 2.1 目标

1. 将 Talos 中的 Plan-Re-Act 收敛为统一主路径上的一种强类型 planning mode
2. 明确 3 种 planning mode 的语义边界
3. 保持 `general_turn` 作为唯一工具执行主路径
4. 让 Plan-Re-Act 具备明确的 loop 终止条件、replan 触发条件和状态推进规则
5. 与现有 `PromptBuilder`、`ToolRegistry`、`LlmProvider`、`SessionService` 保持兼容

### 2.2 非目标

1. 不新增独立 intent classifier
2. 不新增基于规则分流的 `TaskKind`
3. 不引入第二套 tool executor
4. 不把 planner 扩展为“直接执行工具”的 orchestrator
5. 不为了兼容旧语义继续保留模糊命名

---

## 3. 现状问题

### 3.1 命名和职责错位

当前 `PlanReactLoop` 这个命名暗示存在一条独立 loop engine，但真实情况是：

- `flow` 仍负责主执行推进
- `general_turn` 仍负责工具调用闭环
- `planner` 仅负责 prompt / response shaping

因此，当前问题不是“Plan-Re-Act 完全不存在”，而是“语义已经存在一半，但命名和边界描述不准确”。

### 3.2 模式混用了 strategy 和 flow

当前几种 profile 混杂了三类概念：

- 有没有 tools
- 用不用模型原生 thinking
- 是否启用 NL planning 风格

这会导致：

1. 配置含义不稳定
2. 测试粒度难以稳定
3. 未来扩展更多 planner mode 时容易继续堆 profile

### 3.3 Plan-Re-Act 缺少显式 loop policy

虽然已有多轮工具回合，但缺少对以下语义的清晰定义：

- 何时判定本轮完成
- 何时必须 replan
- 何时判定无进展空转
- 何时中止整个 turn

---

## 4. 核心设计

### 4.1 模式定义

建议将 Talos 的规划能力重新定义为 3 种 `PlannerMode`：

```rust
pub enum PlannerMode {
    None,
    BuiltIn,
    PlanReAct,
}
```

语义如下：

| 模式 | 语义 | 主要行为 |
|------|------|----------|
| `None` | 普通通用回合 | 不注入 planning instruction，不启用原生 thinking |
| `BuiltIn` | 模型原生 thinking 模式 | 通过 provider request 注入 native thinking config |
| `PlanReAct` | 自然语言规划 + 工具循环模式 | 注入 planning instruction，并对响应做 planning-aware postprocess |

> 枚举变体统一使用 `PlanReAct`（ReAct = Reasoning + Acting，领域标准写法），代码和文档中不再使用 `PlanReact` 或 `PlanReactLoop`。

这 3 种模式参考 ADK 的 planner 实现方式，但在 Talos 中落成统一配置和统一 loop policy。

### 4.2 分层模型

建议将执行分层明确为：

```text
PlannerMode
    │
    ├── request planning effect
    ├── response planning effect
    ▼
SingleFlow / AutoFlow
    │
    ▼
general_turn loop
    │
    ├── model step
    ├── tool batch
    ├── observation append
    └── stop / continue decision
```

这里最关键的约束是：

- planner 不直接执行工具
- planner 不拥有主循环
- `general_turn` 是唯一工具执行路径

### 4.3 Plan-Re-Act 状态机

Plan-Re-Act 应补足为以下最小状态机：

```text
             ┌────────────────────┐
             │  Build Step Request │
             └─────────┬──────────┘
                       │
                       ▼
             ┌────────────────────┐
             │  Call Model Step    │
             └─────────┬──────────┘
                       │
         ┌─────────────┼─────────────┐
         │             │             │
         ▼             ▼             ▼
┌────────────────┐ ┌──────────────┐ ┌─────────────────┐
│ Final Answer    │ │ Tool Calls    │ │ No Progress      │
└───────┬─────────┘ └──────┬───────┘ └────────┬────────┘
        │                  │                  │
        ▼                  ▼                  ▼
   ┌─────────┐      ┌──────────────┐    ┌──────────────┐
   │  Stop   │      │ Execute Tools │    │ Replan/Abort  │
   └─────────┘      └──────┬───────┘    └──────────────┘
                           │
                           ▼
                    ┌──────────────┐
                    │  Observation  │
                    └──────┬───────┘
                           │
                           ▼
                    next model round
```

### 4.4 最小 loop policy

对于 `PlannerMode::PlanReAct`，`general_turn` 应遵循以下控制规则：

1. 如果模型返回 `final_answer`，且没有新的 tool calls，则结束
2. 如果模型返回 tool calls，则执行工具并将 observation 写回消息流，进入下一轮
3. 如果模型没有 final answer，也没有 tool calls，则按“无进展”处理
4. 如果工具执行失败，允许模型在下一轮输出 `REPLANNING`
5. 达到 `max_tool_rounds` / `max_llm_calls` / `max_replans` 后必须终止

---

## 5. 目标架构

### 5.1 核心流程

```text
User Input
   │
   ▼
Flow::run_events
   │
   ├── Planner::build_request_effect()
   │       ├── BuiltIn    -> native_thinking
   │       ├── PlanReAct  -> planning_instruction
   │       └── None       -> no-op
   │
   ├── general_turn::execute_step()
   │
   ├── Planner::process_response_effect()
   │       ├── normalize reasoning segments
   │       ├── detect final answer
   │       ├── detect branch override
   │       └── derive planning state delta
   │
   ├── if tool_calls:
   │       execute_general_turn_tool_batch()
   │       append observations
   │       continue
   │
   └── else:
           stop or replan
```

### 5.2 数据结构

```rust
pub struct PlanningRequestEffect {
    pub planning_instruction: Option<String>,
    pub native_thinking: Option<NativeThinkingConfig>,
    pub clear_request_thoughts: bool,
}

pub struct PlanningResponseEffect {
    pub normalized_text: Option<String>,
    pub segments: Vec<PlanningResponseSegment>,
    pub output_text_override: Option<String>,
    pub clear_output_text: bool,
    pub state_delta: Vec<StateDelta>,
    pub branch_override: Option<ExecutionBranch>,
    pub has_tool_calls: bool,
    pub has_final_answer: bool,
}

pub struct PlanReActLoopState {
    pub round: u32,
    pub replan_count: u32,
    pub consecutive_no_progress_rounds: u32,
    pub last_reasoning: Option<String>,
    pub last_observation_summary: Option<String>,
}
```

`PlanReActLoopState` 不是第二套 session 模型，而是 `general_turn` 内部的回合控制状态。  
当前轮的即时结果，例如 `has_tool_calls` / `has_final_answer`，应直接来自 `PlanningResponseEffect`，不在 loop state 中重复保存。

---

## 6. 模块接口清单

| 模块 | 责任 | 建议接口 |
|------|------|----------|
| `infra/config` | 暴露强类型 planning 配置 | `PlannerMode`, `PlanReActPolicy` |
| `agent-kernel/planner` | planner strategy 抽象 | `build_request_effect`, `process_response_effect` |
| `agent-kernel/processor` | 将 planner effect 注入 request/response | `PlanningRequestProcessor`, `PlanningResponseProcessor` |
| `agent-kernel/general_turn` | 唯一工具循环主路径 | `execute_general_turn_step`, `execute_general_turn_tool_batch` |
| `agent-kernel/flow` | 回合推进与 stop/continue 决策 | `run_events`, `select_loop_policy` |
| `common/prompt` | 只负责 prompt 组装，不感知 planner 内部状态机 | `PromptBuilder` |

---

## 7. 核心 API 伪代码

### 7.1 强类型配置

```rust
pub enum PlannerMode {
    None,
    BuiltIn,
    PlanReAct,
}

pub struct PlanReActPolicy {
    pub max_replans: u32,
    pub max_no_progress_rounds: u32,
    pub require_final_answer_tag: bool,
}

pub struct AgentKernelConfig {
    pub planner_mode: PlannerMode,
    pub plan_react: PlanReActPolicy,
    pub max_tool_rounds: u32,
    pub max_llm_calls: u32,
}
```

### 7.2 Planner trait

```rust
pub trait Planner: Send + Sync {
    fn mode(&self) -> PlannerMode;

    fn build_request_effect(
        &self,
        ctx: &InvocationContext,
        request: &LlmRequestEnvelope,
    ) -> PlanningRequestEffect;

    fn process_response_effect(
        &self,
        ctx: &InvocationContext,
        response: &LlmResponseEnvelope,
    ) -> PlanningResponseEffect;
}
```

### 7.3 Flow loop

```rust
loop {
    let step_request = build_general_turn_step_request(...);
    let step_response = execute_general_turn_step(step_request, llm_provider).await?;

    let planning = planner.process_response_effect(ctx, &step_response.response);
    apply_planning_effect(&mut turn_state, &planning);

    if planning.has_tool_calls {
        if planning.has_final_answer {
            warn!("plan-react response contains both tool_calls and final_answer; final_answer is dropped for this round");
        }
        let tool_batch = execute_general_turn_tool_batch(...).await?;
        apply_general_turn_tool_batch(&mut turn_state, &tool_batch, ...);
        continue;
    }

    if planning.has_final_answer {
        break;
    }

    if should_replan(&turn_state, &planning, last_tool_batch.as_ref(), &policy) {
        continue;
    }

    return Err(TalosError::PlanningError("plan-react stalled".to_string()));
}
```

两者并存时的规则应明确为：

1. `tool_calls` 优先
2. 当前轮的 `final_answer` 丢弃
3. 记录 warning，便于后续观测模型格式偏差

### 7.4 replan 判断逻辑

`should_replan` 是 loop policy 的核心判断点，建议显式定义为：

```rust
fn should_replan(
    state: &PlanReActLoopState,
    response: &PlanningResponseEffect,
    last_tool_batch: Option<&GeneralTurnToolBatch>,
    policy: &PlanReActPolicy,
) -> bool {
    if response.has_final_answer || response.has_tool_calls {
        return false;
    }

    if let Some(tool_batch) = last_tool_batch {
        if tool_batch.error_count > 0 && state.replan_count < policy.max_replans {
            return true;
        }
    }

    if state.consecutive_no_progress_rounds < policy.max_no_progress_rounds {
        return true;
    }

    false
}
```

这里的 replan 语义需要明确：

1. kernel 不主动生成新的计划文本
2. kernel 只负责把 tool error / observation / trajectory 保留在上下文中
3. 模型在下一轮基于这些上下文自主输出 `REPLANNING`、新的 tool call，或 final answer

因此这里是 model-driven replan，而不是 kernel-driven replanner。

---

## 8. 与 ADK 的对齐和差异

### 8.1 对齐点

1. planner 只做 request/response shaping
2. tool execution 继续走统一 flow
3. tool result observation 由主执行链写回上下文

### 8.2 Talos 增强点

Talos 相比 ADK 需要额外补足：

1. 更明确的 stop / replan policy
2. 更强类型的 planner mode 和 policy config
3. 更清晰的 loop state，避免仅靠 tag 文本驱动

---

## 9. 迁移方案

### Phase 1: 语义收口

1. 将配置层 `planner_mode` 改为强类型枚举
2. 将 planner 抽象统一为 request/response effect 接口
3. 让 `FlowProfile` 只表达 flow 语义，不再混入 planner 语义

### Phase 2: Plan-Re-Act loop policy 落地

1. 引入 `PlanReActLoopState`
2. 明确 no-progress / replan / abort 规则
3. 为 `PlanReAct` 增加稳定的 stop 条件

### Phase 3: 测试和观测

1. 单测覆盖 3 种 planner mode 的 request/response effect
2. 集成测试覆盖：
   - final answer 直接结束
   - tool-call -> observe -> final answer
   - tool failure -> replan -> final answer
   - no-progress -> abort

其中 `tool failure -> replan -> final answer` 的机制应明确为：

1. tool failure 以 observation / tool result error 的形式写回上下文
2. kernel 根据 policy 允许进入下一轮
3. 模型读取这些错误 observation 后，自主输出 `REPLANNING`、新的 tool call，或 final answer

---

## 10. 风险与取舍

### 10.1 不建议继续维持当前命名

如果继续保留 `PlanReactLoop` 但不补全 loop policy，风险是：

1. 配置含义被误解
2. 测试无法稳定描述期望
3. 后续实现会继续在错误抽象上堆逻辑

### 10.2 不建议单独新建第二条执行链

如果为 Plan-Re-Act 新建独立 executor：

1. 会与 `general_turn` 主路径分叉
2. 工具执行、session 写回、事件投影会出现双份逻辑
3. 与当前项目“统一 general_turn”原则冲突

---

## 11. 结论

Talos 的 Plan-Re-Act 不应被设计为一个独立 orchestrator，而应被定义为：

- 统一 `general_turn` 主路径上的一种强类型 planning mode
- 由 planner strategy 提供 request/response effect
- 由 flow + loop policy 提供真正的 Plan-Re-Act 行为闭环

这个方向既与 ADK 的真实设计一致，也更符合 Talos 当前“模型驱动、统一通用任务路径、不做规则分流”的架构原则。
