# Talos Prompt Builder / Agent Kernel 架构方案（合并定稿）

## 1. 总体结论
1. 采用“模型主导 + 策略护栏”路线。
2. 不做独立意图识别器，不把 `Scenario` 作为外部输入必需项。
3. 全量复用现有系统能力，避免重复造轮子。
4. 仅新增最小模块：
   - `common/prompt` 统一 PromptBuilder
   - `orchestration/agent` 的 agent 绑定注册
   - `orchestration/agent` 的 planner 模式解析

## 2. 全局复用基线（已存在能力）

### 2.1 LLM 与工具调用（复用）
- 复用 `crates/common/llm/provider`：`LlmProvider`, `ChatRequest`, `ChatMessage`, `ToolStream`。
- 复用 `crates/common/llm/*` provider 实现（anthropic/openai/ollama/router）。
- 不新增第二套模型调用层。

### 2.2 Tool 系统（复用）
- 复用 `crates/action/tools::ToolRegistry`：
  - `get_tool_definitions()`
  - `execute_with_context()`
  - 输出截断与策略限制
- Skill/A2A 按 Tool 方式接入，不新增 `CapabilityRegistry`。

### 2.3 Memory 系统（复用）
- 复用 `crates/cognition/memory`，并新增统一入口 `MemoryStore`（内存优先 + 持久回填）。
- `UnifiedMemory` 负责长期语义条目沉淀；`MemoryStore` 负责会话 turn/summary 持久化与 prompt context 装配。
- 会话持久格式升级为 markdown v2（turn/summary 均包含稳定 ID）。
- 不新增 `MemoryEngine`。

### 2.4 Kernel 运行时（复用）
- 复用 `crates/orchestration/agent-kernel`：
  - `LlmKernelAgent`
  - `KernelRuntime`
  - `BuiltInPlanner` / `PlanReActPlanner`
  - `KernelExecutionMode`
- 不新增第二套运行时主循环。

## 3. 目标架构（最小新增）

### 3.1 PromptBuilder（新增核心）
- 位置：`crates/common/prompt`
- 职责：
  - 按 `PromptPhase` 统一组装 messages
  - 聚合工具摘要、memory 切片、用户输入
  - 执行 token 预算与裁剪
- 非职责：
  - 不做意图识别
  - 不执行工具
  - 不持久化记忆

### 3.2 Agent 绑定注册（新增轻量）
- 位置：`crates/orchestration/agent`
- 职责：
  - 根据配置与请求上下文绑定具体 KernelAgent
  - 未命中默认回退 `LlmKernelAgent`
- 备注：不改动 `agent-kernel` 主执行语义

### 3.3 Planner 模式解析（新增轻量）
- 位置：`crates/orchestration/agent`
- 职责：
  - 解析 planner 模式（默认 BuiltIn）
  - 支持显式启用 PlanReAct

## 4. 调用流程（ASCII）
```text
+--------------------------------+
| AgentImpl::run / run_stream    |
+--------------------------------+
                |
                v
+--------------------------------+
| 读取配置 + 请求 metadata         |
| - planner_mode                  |
| - kernel_mode                   |
| - max_tool_rounds 等            |
+--------------------------------+
                |
                v
+--------------------------------+
| 绑定 KernelAgent                |
| - registry 命中                 |
| - fallback: LlmKernelAgent      |
+--------------------------------+
                |
                v
+--------------------------------+
| KernelRuntime::run_turn         |
+--------------------------------+
                |
                v
+--------------------------------+
| LlmKernelAgent / general_turn   |
| - PromptBuilder.build(...)      |
| - LlmProvider.stream_with_tools |
| - ToolRegistry.execute...       |
| - Unified/WorkspaceMemory更新   |
+--------------------------------+
                |
                v
+--------------------------------+
| Canonical output / stream events|
+--------------------------------+
```

## 5. Planner 设计

### 5.1 BuiltIn（默认）
- 轻约束，低 token 成本。
- 适用于绝大多数默认对话与工具调用场景。

### 5.2 PlanReAct（显式启用）
- 强约束规划->行动->总结。
- 适用于复杂任务或需要可审计规划轨迹。

### 5.3 选择规则
- 默认来源：配置文件。
- 请求可通过 metadata 显式覆盖（受策略开关控制）。
- 非法值回退 BuiltIn 并记录告警。

## 6. PromptBuilder 设计（无 Scenario 外部输入）

### 6.1 外部 API 草案
```rust
use talos_prompt::{PromptBuilder, PromptPhase};

let messages = PromptBuilder::new()
    .agent("talos", "personal assistant")
    .phase(PromptPhase::Execution)
    .tools(&tool_registry)
    .workspace_memory(session_id, &workspace_memory)
    .unified_memory(&memory)
    .user_input(input)
    .metadata(metadata)
    .build()?;
```

### 6.2 输入维度
- `PromptPhase`: `Planning | Execution`
- `PlannerMode`: `BuiltIn | PlanReAct`
- `metadata`（预算/压缩/运行参数）

### 6.3 组装策略
1. 静态段：身份、工具协议、安全约束。
2. 动态段：工具摘要、workspace summary、recent messages、用户输入。
3. 预算裁剪：优先保留静态段与最新上下文。

## 7. 配置统一治理（通过 config 模块）

### 7.1 单一配置入口
- 所有默认运行参数统一放入 `crates/infra/config` 的 `Config`。
- 以 `agent.kernel` 作为 kernel/prompt/planner 的统一默认源。

### 7.2 现有可直接复用字段
- `agent.kernel.max_iters`
- `agent.kernel.max_task_per_turn`
- `agent.kernel.tool_parallelism`
- `agent.kernel.max_llm_calls`
- `agent.kernel.max_tool_rounds`
- `agent.kernel.tool_result_max_bytes`
- `agent.kernel.context_compaction_max_bytes`
- `agent.kernel.default_workspace_id`

### 7.3 建议新增字段（同样进入 config 模块）
- `agent.kernel.planner_mode`: `built_in | plan_react`
- `agent.kernel.allow_request_planner_override`: `bool`
- `agent.kernel.prompt.max_static_tokens`: `u32`
- `agent.kernel.prompt.max_dynamic_tokens`: `u32`
- `agent.kernel.prompt.include_workspace_summary`: `bool`
- `agent.kernel.prompt.include_recent_messages`: `bool`
- `agent.kernel.memory.workspace_dir`: `string`
- `agent.kernel.memory.hot_recent_messages`: `u32`
- `agent.kernel.memory.context_target_messages`: `u32`
- `agent.kernel.memory.enable_summary`: `bool`
- `agent.kernel.memory.summary_recent_count`: `u32`
- `agent.kernel.memory.summary_max_chars`: `u32`
- `agent.kernel.memory.compaction_threshold_bytes`: `u64`
- `agent.kernel.memory.long_term_fact_extraction_enabled`: `bool`
- `agent.kernel.memory.long_term_min_importance`: `f32`
- `agent.kernel.binding.default_agent_kind`: `string`

### 7.4 覆盖规则
1. 配置文件（默认基线）
2. 环境变量覆盖（现有 `TALOS__` 机制）
3. 请求 metadata 覆盖（仅允许白名单键）

### 7.5 请求 metadata 白名单（建议）
- `planner_mode`（需 `allow_request_planner_override=true`）
- `kernel_mode`
- `max_tool_rounds`（上限受 config 约束）
- `tool_output_max_bytes`（上限受 config 约束）
- `context_compaction_max_bytes`（上限受 config 约束）

## 8. 实施任务清单（只做必要改动）
1. `common/prompt`：实现 PromptBuilder（phase + budget + memory/tool 聚合）。
2. `agent-kernel/general_turn`：移除硬编码 system prompt 拼接，改为调用 PromptBuilder。
3. `orchestration/agent`：加入 agent 绑定注册与默认回退。
4. `orchestration/agent`：加入 planner mode resolver（配置默认 + metadata 覆盖）。
5. `infra/config`：补齐 planner/prompt/binding 新增配置项与校验。

## 9. 测试矩阵
1. 默认路径：`LlmKernelAgent + BuiltIn + PromptBuilder`。
2. 显式 `plan_react`：规划约束生效，工具调用不回退。
3. 工具异常路径：tool error、truncated output、超时重试。
4. memory 路径：summary/recent 注入与 compaction 阈值联动。
5. 绑定路径：registry 命中与 fallback 行为。
6. 配置路径：config 默认、环境覆盖、metadata 白名单覆盖。

## 10. 验收标准（DoD）
1. Prompt 组装统一在 `common/prompt`。
2. 不新增重复引擎，核心能力全部复用现有模块。
3. Planner 双模式可由 config 管控并可观测。
4. Agent 编排可扩展绑定，默认稳定回退。
5. 配置统一由 `infra/config` 管理并可校验。
