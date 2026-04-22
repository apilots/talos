# PromptBuilder Alignment With Claude-Code Prompt Architecture

## 1. 背景

Talos 当前 `PromptBuilder` 已经具备以下基础能力：

- `PromptAssembly` 区分 `static_prefix_sections` 与 `dynamic_suffix_sections`
- `PromptSection` 具备 `kind`、`title`、`cache_scope`、`tier` 等元数据
- `assembler` 已采用“按 section 生成固定提示块”的模式
- `renderer` 已支持基于 `cache_scope` 输出带缓存控制的 `ChatMessage`

这说明 Talos 并不是缺少 prompt 模块化基础，而是还没有把这些能力收敛成一套更严格的 prompt architecture contract。

对比本地 `claude-code` 源码，可以看到其关键优势不在“把 prompt 全部 XML 化”，而在于以下纪律：

1. system prompt 由固定 section 组成，顺序稳定。
2. static prefix 与 dynamic suffix 有明确边界。
3. 会引发 cache 失效的内容被后置到 dynamic 区。
4. 每个区域不只是“塞数据”，而是带有固定的 framing prompt。
5. 最终发给模型前，会先按 boundary 聚合成稳定 block，而不是任意散落。

Talos 下一步不应机械复制 `claude-code` 的 TypeScript 实现细节，而应在保留 Rust `PromptSection` 抽象的前提下，对齐其 prompt architecture discipline。

---

## 2. 设计目标

### 2.1 主要目标

1. 让 `PromptSection` 成为 Talos prompt 编排的唯一基础单元。
2. 为 `PromptBuilder` 建立稳定的 canonical serialization order。
3. 为 static prefix 和 dynamic suffix 建立强约束 boundary contract。
4. 让每个 region 同时具备“稳定名字 + 固定 framing prompt + 实际内容”。
5. 将 renderer 从“逐 section 输出 message”升级为“按 block 分组输出”。
6. 为未来模型侧 cache 命中优化预留稳定的 provider adapter 接口。
7. 本方案可以独立于 context compaction spec 单独实施。

### 2.2 非目标

1. 不复制 `claude-code` 的 provider 专用逻辑，如 billing header、1P feature gates、org/global 专属平台行为。
2. 不把 Talos prompt 全量改造成 XML prompt。
3. 不在本阶段重写 compaction 算法。
4. 不在本阶段引入新的 prompt DSL。

---

## 3. 现状判断

### 3.1 Talos 已具备的基础

Talos 当前实现已经满足以下前提：

- `PromptAssembly` 已有 static/dynamic 双区结构。
- `PromptSectionKind` 已具备较细粒度语义枚举。
- 静态区已采用固定 section 文案函数，如 identity、tooling policy、communication policy。
- diagnostics 已能观察 section 包含、drop、truncation 和 token 估算。

### 3.2 与 `claude-code` 的关键差距

当前差距主要有五类：

1. **缺少显式 boundary contract**
   逻辑上区分了 static/dynamic，但没有把“边界前内容必须稳定、不可被预算治理改写”提升为硬契约。

2. **缺少 canonical serialization order contract**
   当前 section 顺序由 assembler 代码隐式保证，尚未形成文档化、可测试的稳定序列协议。

3. **renderer 粒度偏细**
   目前 renderer 基本按 section 输出 `ChatMessage`，不利于未来 provider 级 cache block 复用。

4. **cache scope 语义偏弱**
   当前 `CacheScope::Static | Workspace | None` 最终都被渲染成较弱的缓存控制语义，缺乏明确的“prefix block 可缓存”抽象。

5. **region framing 还不够统一**
   虽然已有固定 section 文案，但不同 region 的结构还不够一致，尚未形成“名字 + framing + payload”的稳定协议。

---

## 4. 核心设计

### 4.1 总体原则

Talos 对齐 `claude-code` 的方式不是复制其源码结构，而是收敛到如下五条原则：

1. `PromptSection` 是唯一 prompt 编排单元。
2. 所有 system prompt 都必须先进入 section，再进入 render。
3. static prefix 与 dynamic suffix 必须有明确 boundary。
4. boundary 前内容必须稳定、可缓存、不可在运行时预算治理中被删除或重排。
5. 每个 region 必须带固定 framing prompt，而不是只注入裸数据。

### 4.2 Prompt 编排分层

Talos prompt 编排统一分为六层：

```text
PromptBuilder
    -> PromptSection list
        -> DefaultSystemPromptAssembly
            -> RequestContextInjection
                -> PromptBlock list
                    -> provider ChatMessage / request blocks
```

含义如下：

1. `PromptBuilder`
   负责收集输入上下文，不做 provider 细节。

2. `PromptSection`
   负责表达单个语义区域，是默认 system prompt 的最小治理单元。

3. `DefaultSystemPromptAssembly`
   负责把 section 组织成 static prefix 与 dynamic suffix。

4. `RequestContextInjection`
   负责把不属于默认 system prompt 的上下文注入到最终请求中，例如：
   - appended system context
   - prepended user context message
   - current user input

5. `PromptBlock`
   负责真正面向模型请求的 block 聚合，是 renderer 与 provider adapter 的边界对象。

6. `provider ChatMessage / request blocks`
   是最终发给模型的请求结构。

### 4.2.1 Final Request Assembly Contract

参考 `claude-code` 的完整装配路径，Talos 不能只定义默认 system prompt 内部结构，还必须定义最终请求装配顺序。

Talos 的完整请求装配应拆为四类输入源：

1. `DefaultSystemPrompt`
   - 由 `PromptSection` 组装得到
   - 内部分为 `StaticPrefix` 与 `DynamicSuffix`

2. `AppendedSystemContext`
   - 属于系统上下文注入
   - 例如稳定的运行态信息、Git 状态、工作区运行上下文
   - 不属于默认 system prompt 的 canonical section catalog

3. `PrependedUserContextMessage`
   - 属于用户上下文注入
   - 例如当前日期、当前时区、request-local reminder
   - 应作为一条高优先级的特殊系统提醒消息插入到消息序列前部

4. `CurrentUserInput`
   - 当前轮真实用户输入

最终装配顺序要求如下：

```text
1. DefaultSystemPrompt.StaticPrefix
2. DefaultSystemPrompt.DynamicSuffix
3. AppendedSystemContext
4. PrependedUserContextMessage
5. CurrentUserInput
```

说明：

- `DefaultSystemPrompt` 负责长期稳定的 system policy 与动态执行区域。
- `AppendedSystemContext` 负责补充系统运行态上下文，而不是重复 policy section。
- `PrependedUserContextMessage` 负责把时间信息和 request-local reminder 前置到消息流中。
- `CurrentUserInput` 始终保留为最后的当前请求输入。

约束：

1. 不允许把 `PrependedUserContextMessage` 直接塞回 `StaticPrefix`。
2. 不允许把 `AppendedSystemContext` 混入 region catalog 中伪装成静态 section。
3. 不允许把所有上下文都混成一个大 system block，这会破坏职责边界与缓存策略。

### 4.3 Canonical Serialization Order

Talos 必须定义一份固定的 canonical region order。默认顺序如下：

#### Static Prefix

1. `IdentityCore`
2. `EvidenceToolingPolicy`
3. `ToolChoicePriority`
4. `RequestedToolPreferenceTemplate`
5. `FailureRecoveryPolicy`
6. `TaskProtocolLibrary`
7. `CommunicationPolicy`
8. `BootstrapSoul`
9. `BootstrapIdentity`
10. `BootstrapUser`
11. `BootstrapHeartbeat`
12. `BootstrapMemory`
13. `BootstrapAgents`

#### Dynamic Suffix

1. `RequestedToolPreferenceHint`
2. `PlanningInstruction`
3. `RecentTaskContinuity`
4. `SessionSummary`
5. `RecentMessages`
6. `RetrievedMemories`
7. `ExpandedToolCatalog`
8. 其他 session-variant region

要求：

- 顺序必须由单一位置统一定义。
- assembler 中任意新增 section 时，必须显式声明其 canonical slot。
- 不允许模块各自 append，导致顺序漂移。

### 4.4 Static Prefix Boundary Contract

Talos 必须建立明确的 boundary contract。

```text
StaticPrefix
    = all canonical static sections serialized in fixed order

DynamicSuffix
    = all session-variant sections serialized after static prefix
```

要求：

1. `StaticPrefix` 必须跨 turn 保持内容、顺序、序列化格式稳定。
2. `DynamicSuffix` 承载 session、memory、tooling visibility、recent messages 等易变内容。
3. 预算治理不得修改、截断、重排、drop `StaticPrefix`。
4. `Last-Resort Drop` 仅允许作用于 `DynamicSuffix`。
5. `StaticPrefix` 变化必须被记录为显式 cache break event。

### 4.5 Region Framing Prompt Contract

每个 region 必须由三部分构成：

```text
Region = RegionName + FramingPrompt + RegionPayload
```

#### RegionName

- 稳定、可测试、不可本地化
- 使用强类型 `PromptSectionKind`

#### FramingPrompt

- 固定模板
- 解释该区域的职责、优先级、如何被模型使用
- 不能被运行时数据直接覆盖

#### RegionPayload

- 区域具体内容
- 可为空
- 为空时是否输出由 section 自身规则决定

示例：

```text
Section: RecentTaskContinuity
Framing:
- This section contains verified short-term execution continuity.
- Prefer it over recent chat phrasing when deciding how to resume work.
- Do not treat inferred content as verified evidence.

Payload:
- recent_verified_paths: ...
- recent_successful_tools: ...
```

这比仅仅输出：

```text
Recent task continuity:
- recent_verified_paths: ...
```

更接近 `claude-code` 的 section discipline。

### 4.5.1 Region Template Catalog

为避免实施时对 region 自由发挥，Talos 必须把核心 region 固化成模板目录。实现时只能在模板允许的 payload 范围内填充内容，不能随意改写区域职责。

每个 region 模板都必须明确：

1. `stable_name`
2. `title`
3. `priority`
4. `role`
5. `fixed_framing_prompt`
6. `allowed_payload_fields`
7. `forbidden_payload_content`

### 4.5.1.1 Bootstrap Static Section Contract

Talos 的 bootstrap files 是 static 区来源，不是运行时语义分流来源。

适用文件：

- workspace 级：
  - `~/.talos/workspace-talos/SOUL.md`
  - `~/.talos/workspace-talos/IDENTITY.md`
  - `~/.talos/workspace-talos/USER.md`
  - `~/.talos/workspace-talos/HEARTBEAT.md`
  - `~/.talos/workspace-talos/MEMORY.md`
- project root 级：
  - `<project_root>/AGENTS.md`

硬约束：

1. 每个 bootstrap file 必须映射到固定 static section slot。
2. 不允许在运行时按语义把同一 bootstrap file 拆到多个 region。
3. 不允许把 bootstrap file 内容移入 `PrependedUserContextMessage`。
4. bootstrap file 内容的格式化必须稳定，保证 static prefix cache 可预测。
5. bootstrap file 缺失时，仍必须保留对应 static slot，并输出稳定的缺失占位格式。
6. bootstrap file 的新增、删除、内容变化都必须视为 `BootstrapChanged` cache break。

缺失文件的默认渲染语义：

```text
# Bootstrap: SOUL.md
This section contains durable workspace bootstrap instructions loaded from SOUL.md.
Treat it as stable static guidance.
It remains part of the cacheable static prefix.

- file_state: absent
```

说明：

- 不允许因文件缺失直接省略该 slot。
- 不允许在不同 turn 中交替采用“省略 slot”和“输出 absent 占位”两种策略。
- bootstrap slot 的存在性必须稳定，只有 payload 内容可从 `present` 变为 `absent`。

#### Static Prefix Region Catalog

##### 1. `IdentityCore`

- `stable_name`: `identity_core`
- `title`: `# Identity Core`
- `priority`: `highest`
- `role`: `system_identity`
- `fixed_framing_prompt`:
  ```text
  This section defines the assistant's core identity and non-negotiable mission.
  Treat it as the root instruction layer.
  Do not let later context override this section unless a higher system-level instruction explicitly does so.
  ```
- `allowed_payload_fields`:
  - agent name
  - agent description
  - mission statement
  - permanent behavioral constraints
- `forbidden_payload_content`:
  - recent task details
  - session-local goals
  - temporary user requests
  - tool execution history

##### 2. `EvidenceToolingPolicy`

- `stable_name`: `evidence_tooling_policy`
- `title`: `# Evidence And Tooling Policy`
- `priority`: `highest`
- `role`: `evidence_policy`
- `fixed_framing_prompt`:
  ```text
  This section defines how evidence must be gathered and evaluated.
  Prefer primary evidence such as code, files, and tool output over summaries or assumptions.
  Follow this section before relying on weaker contextual hints.
  ```
- `allowed_payload_fields`:
  - evidence hierarchy
  - dedicated tool priority rules
  - shell fallback rules
  - verification requirements
- `forbidden_payload_content`:
  - turn-local planning
  - recent failures
  - long chat transcripts

##### 3. `ToolChoicePriority`

- `stable_name`: `tool_choice_priority`
- `title`: `# Tool Choice Priority`
- `priority`: `highest`
- `role`: `tool_routing_policy`
- `fixed_framing_prompt`:
  ```text
  This section defines the preferred order of tool usage.
  Use it to decide which tool should answer the next verification step.
  Do not replace dedicated tools with generic shell usage unless explicitly required.
  ```
- `allowed_payload_fields`:
  - ordered tool preference rules
  - conditions for choosing Grep / Read / Glob / Bash
  - anti-pattern reminders
  - concise tool responsibility summary
  - one-line tool use boundaries
- `forbidden_payload_content`:
  - specific tool call results
  - current turn tool history

##### 4. `RequestedToolPreferenceTemplate`

- `stable_name`: `requested_tool_preference_template`
- `title`: `# Requested Tool Preference Policy`
- `priority`: `high`
- `role`: `requested_tool_policy`
- `fixed_framing_prompt`:
  ```text
  This section defines the general policy for respecting user-requested tools.
  Follow it when the user explicitly names a tool.
  This section defines the policy template, not the current turn's requested tool set.
  ```
- `allowed_payload_fields`:
  - generic policy for honoring user-requested tools
  - conditions for overriding user-requested tools
- `forbidden_payload_content`:
  - current turn requested tool names
  - conversation-local examples not part of stable policy

##### 5. `FailureRecoveryPolicy`

- `stable_name`: `failure_recovery_policy`
- `title`: `# Failure Recovery Policy`
- `priority`: `high`
- `role`: `error_handling_policy`
- `fixed_framing_prompt`:
  ```text
  This section defines what to do after tool failures or partial evidence.
  Treat failed actions as evidence.
  Do not silently abandon useful synthesis when enough verified evidence already exists.
  ```
- `allowed_payload_fields`:
  - policy for handling failed tools
  - salvage rules
  - minimum reporting standards
- `forbidden_payload_content`:
  - current turn failure logs
  - runtime-only error messages

##### 6. `TaskProtocolLibrary`

- `stable_name`: `task_protocol_library`
- `title`: `# Task Protocol Library`
- `priority`: `high`
- `role`: `task_execution_protocols`
- `fixed_framing_prompt`:
  ```text
  This section contains stable execution protocols for recurring task types.
  Use it to choose a disciplined sequence of actions for analysis, debugging, planning, and verification.
  Do not treat it as evidence about the current repository by itself.
  ```
- `allowed_payload_fields`:
  - codebase analysis protocol
  - bug investigation protocol
  - change planning protocol
  - environment verification protocol
- `forbidden_payload_content`:
  - repository-specific facts
  - current task progress state

##### 7. `CommunicationPolicy`

- `stable_name`: `communication_policy`
- `title`: `# Communication Policy`
- `priority`: `medium_high`
- `role`: `user_visible_output_policy`
- `fixed_framing_prompt`:
  ```text
  This section defines how user-visible communication should be written.
  Use it to control update cadence, clarity, and reporting style.
  It governs output style, not factual task state.
  ```
- `allowed_payload_fields`:
  - progress update rules
  - final answer rules
  - wording constraints
  - exact-literal-output obligations
- `forbidden_payload_content`:
  - dynamic user preferences discovered mid-turn
  - recent message content

##### 8. `BootstrapSoul`

- `stable_name`: `bootstrap_soul`
- `title`: `# Bootstrap: SOUL.md`
- `priority`: `medium_high`
- `role`: `static_bootstrap_instruction`
- `fixed_framing_prompt`:
  ```text
  This section contains durable workspace bootstrap instructions loaded from SOUL.md.
  Treat it as stable static guidance.
  It remains part of the cacheable static prefix.
  ```
- `allowed_payload_fields`:
  - normalized `SOUL.md` content
- `forbidden_payload_content`:
  - runtime facts
  - recent messages
  - turn-local continuity

##### 9. `BootstrapIdentity`

- `stable_name`: `bootstrap_identity`
- `title`: `# Bootstrap: IDENTITY.md`
- `priority`: `medium_high`
- `role`: `static_bootstrap_instruction`
- `fixed_framing_prompt`:
  ```text
  This section contains durable workspace bootstrap instructions loaded from IDENTITY.md.
  Treat it as stable static guidance.
  It remains part of the cacheable static prefix.
  ```
- `allowed_payload_fields`:
  - normalized `IDENTITY.md` content
- `forbidden_payload_content`:
  - runtime facts
  - recent messages
  - turn-local continuity

##### 10. `BootstrapUser`

- `stable_name`: `bootstrap_user`
- `title`: `# Bootstrap: USER.md`
- `priority`: `medium_high`
- `role`: `static_bootstrap_instruction`
- `fixed_framing_prompt`:
  ```text
  This section contains durable workspace bootstrap instructions loaded from USER.md.
  Treat it as stable static guidance.
  It remains part of the cacheable static prefix.
  ```
- `allowed_payload_fields`:
  - normalized `USER.md` content
- `forbidden_payload_content`:
  - runtime facts
  - recent messages
  - turn-local continuity

##### 11. `BootstrapHeartbeat`

- `stable_name`: `bootstrap_heartbeat`
- `title`: `# Bootstrap: HEARTBEAT.md`
- `priority`: `medium_high`
- `role`: `static_bootstrap_instruction`
- `fixed_framing_prompt`:
  ```text
  This section contains durable workspace bootstrap instructions loaded from HEARTBEAT.md.
  Treat it as stable static guidance.
  It remains part of the cacheable static prefix.
  ```
- `allowed_payload_fields`:
  - normalized `HEARTBEAT.md` content
- `forbidden_payload_content`:
  - runtime facts
  - recent messages
  - turn-local continuity

##### 12. `BootstrapMemory`

- `stable_name`: `bootstrap_memory`
- `title`: `# Bootstrap: MEMORY.md`
- `priority`: `medium_high`
- `role`: `static_bootstrap_instruction`
- `fixed_framing_prompt`:
  ```text
  This section contains durable workspace bootstrap instructions loaded from MEMORY.md.
  Treat it as stable static guidance.
  It remains part of the cacheable static prefix.
  ```
- `allowed_payload_fields`:
  - normalized `MEMORY.md` content
- `forbidden_payload_content`:
  - runtime facts
  - recent messages
  - turn-local continuity

##### 13. `BootstrapAgents`

- `stable_name`: `bootstrap_agents`
- `title`: `# Bootstrap: AGENTS.md`
- `priority`: `medium_high`
- `role`: `static_bootstrap_instruction`
- `fixed_framing_prompt`:
  ```text
  This section contains durable workspace bootstrap instructions loaded from AGENTS.md.
  Treat it as stable static guidance.
  It remains part of the cacheable static prefix.
  ```
- `allowed_payload_fields`:
  - normalized `AGENTS.md` content
- `forbidden_payload_content`:
  - runtime facts
  - recent messages
  - turn-local continuity

#### Dynamic Suffix Region Catalog

##### 11. `RequestedToolPreferenceHint`

- `stable_name`: `requested_tool_preference_hint`
- `title`: `# Requested Tool Preference Hint`
- `priority`: `high`
- `role`: `turn_local_tool_preference`
- `fixed_framing_prompt`:
  ```text
  This section contains the current turn's explicit tool preferences derived from the user's request.
  Use it to bias the next step when the named tool can directly answer the task.
  It does not override higher-level tool safety or evidence policy.
  ```
- `allowed_payload_fields`:
  - requested tool names for current turn
  - short biasing hint for next step
- `forbidden_payload_content`:
  - full tool catalog
  - historical requested tools from older turns

##### 12. `PlanningInstruction`

- `stable_name`: `planning_instruction`
- `title`: `# Planning Instruction`
- `priority`: `high`
- `role`: `turn_local_execution_guidance`
- `fixed_framing_prompt`:
  ```text
  This section contains turn-local planning guidance.
  Use it to decide the next action sequence for the current task.
  Do not let it override the root policy sections.
  ```
- `allowed_payload_fields`:
  - current turn objective refinement
  - execution hints
  - plan mode instructions
- `forbidden_payload_content`:
  - long policy documents
  - raw recent history

##### 13. `RecentTaskContinuity`

- `stable_name`: `recent_task_continuity`
- `title`: `# Session Continuity`
- `priority`: `high`
- `role`: `resume_execution`
- `fixed_framing_prompt`:
  ```text
  This section contains verified short-term execution continuity.
  Prefer it when deciding how to resume work.
  Do not treat inferred or speculative content as verified evidence.
  ```
- `allowed_payload_fields`:
  - recent verified paths
  - recent successful tools
  - recent failed tools
  - recent unhelpful patterns
  - recent answer mode
- `forbidden_payload_content`:
  - unverified inference
  - long transcripts
  - broad memory summaries

##### 14. `SessionSummary`

- `stable_name`: `session_summary`
- `title`: `# Session Summary`
- `priority`: `medium_high`
- `role`: `compressed_history`
- `fixed_framing_prompt`:
  ```text
  This section contains compressed session history.
  Use it as a compact background summary when the full transcript is unavailable.
  Prefer verified continuity and recent direct evidence when conflict exists.
  ```
- `allowed_payload_fields`:
  - compacted session summary
  - summarized intent and progress
  - summarized unresolved items
- `forbidden_payload_content`:
  - raw detailed tool logs
  - speculative conclusions presented as facts

##### 15. `RecentMessages`

- `stable_name`: `recent_messages`
- `title`: `# Recent Messages`
- `priority`: `medium`
- `role`: `recent_raw_context`
- `fixed_framing_prompt`:
  ```text
  This section contains recent raw conversation context.
  Use it as supporting context only.
  Do not let it override policy sections or verified continuity when they conflict.
  ```
- `allowed_payload_fields`:
  - recent user messages
  - recent assistant messages
  - minimal raw conversational context
- `forbidden_payload_content`:
  - full long history
  - duplicate content already present in continuity or summary

##### 16. `RetrievedMemories`

- `stable_name`: `retrieved_memories`
- `title`: `# Retrieved Memories`
- `priority`: `medium_low`
- `role`: `supplementary_memory`
- `fixed_framing_prompt`:
  ```text
  This section contains supplementary retrieved memory.
  Use it as background reference only.
  Do not allow it to override stronger evidence from current code, files, or verified continuity.
  ```
- `allowed_payload_fields`:
  - retrieved memory snippets
  - durable user preferences
  - historical project notes
- `forbidden_payload_content`:
  - duplicated recent transcript
  - weak memory assertions presented as current facts

##### 17. `ExpandedToolCatalog`

- `stable_name`: `expanded_tool_catalog`
- `title`: `# Expanded Tool Catalog`
- `priority`: `low`
- `role`: `capability_reference`
- `fixed_framing_prompt`:
  ```text
  This section describes tools available in the current turn.
  Use it as a capability reference only.
  It should not override task policy, evidence hierarchy, or tool routing rules.
  ```
- `allowed_payload_fields`:
  - visible tool descriptions
  - deferred tool discovery notes
  - capability visibility summary
- `forbidden_payload_content`:
  - tool selection policy
  - execution plans
  - raw tool outputs
  - recent task state

### 4.5.2 Region Rendering Rules

每个 region 的最终渲染格式必须固定，避免同一语义区域在不同实现者手里变成不同文本结构。

默认渲染骨架如下：

```text
<title>
<fixed framing prompt>
<payload block>
```

其中：

1. `title`
   - 必须稳定
   - 必须与 `PromptSectionKind` 一一对应

2. `fixed framing prompt`
   - 必须来自模板目录
   - 不允许由运行时逻辑改写

3. `payload block`
   - 仅允许填充模板声明过的字段
   - 字段顺序必须稳定
   - 缺省字段不输出占位垃圾文本

补充约束：

1. `StaticPrefix` 区域只允许使用稳定标题和固定描述语，不允许使用 XML-like 标签包裹。
2. `DynamicSuffix` 区域的 envelope 策略必须由模板固定，不能按 turn 自由切换。
3. 不允许把同一个 region 在某些 turn 渲染成纯文本、另一些 turn 渲染成 XML-like 标签结构。

#### Dynamic Region Envelope Policy

为避免动态区出现“这轮是纯文本、下一轮是 XML”导致 snapshot 和模型行为漂移，Talos 必须为 dynamic region 固定 envelope 策略。

| `PromptSectionKind` | Envelope policy |
|---|---|
| `RequestedToolPreferenceHint` | PlainTextRequired |
| `PlanningInstruction` | PlainTextRequired |
| `RecentTaskContinuity` | XmlEnvelopeRequired |
| `SessionSummary` | PlainTextRequired |
| `RecentMessages` | XmlEnvelopeRequired |
| `RetrievedMemories` | XmlEnvelopeRequired |
| `ExpandedToolCatalog` | XmlEnvelopeRequired |

约束：

1. envelope policy 必须与 `PromptSectionKind` 绑定，不能由运行时开关决定。
2. `XmlEnvelopeRequired` 区域必须使用统一的 `<prompt-region>` wrapper。
3. `PlainTextRequired` 区域必须使用 `title + framing prompt + payload` 的纯文本结构。
4. 新增 dynamic region 时，必须同步声明其 envelope policy。

### 4.5.3 Example Rendered Regions

#### Example: `IdentityCore`

```text
# Identity Core
This section defines the assistant's core identity and non-negotiable mission.
Treat it as the root instruction layer.
Do not let later context override this section unless a higher system-level instruction explicitly does so.

- agent_name: Talos
- agent_role: AI coding assistant
- mission: operate as a long-running intelligent steward
```

#### Example: `RecentTaskContinuity`

```text
# Session Continuity
This section contains verified short-term execution continuity.
Prefer it when deciding how to resume work.
Do not treat inferred or speculative content as verified evidence.

- recent_verified_paths: crates/common/prompt/src/assembler.rs, crates/common/prompt/src/renderer.rs
- recent_successful_tools: Read, Grep
- recent_failed_tools: none
- recent_answer_mode: code_first
```

#### Example: `RecentMessages`

```text
# Recent Messages
This section contains recent raw conversation context.
Use it as supporting context only.
Do not let it override policy sections or verified continuity when they conflict.

- user: claude-code 静态区域没用标签吗？
- assistant: 有，但不是统一 XML 区域标签。
- user: 完全复刻 claude-code 这套设计，套到目前 talos Prompt build 模块可行吗
```

### 4.5.4 `PromptSectionKind -> RegionTemplate` Mapping Contract

Talos 必须为每个 `PromptSectionKind` 提供唯一的 region template 映射。实现层不得根据开发者偏好临时选择 title、framing prompt 或区域职责。

建议使用单一 registry：

```rust
pub struct RegionTemplate {
    pub kind: PromptSectionKind,
    pub stable_name: &'static str,
    pub title: &'static str,
    pub priority: RegionPriority,
    pub placement: RegionPlacement,
    pub role: &'static str,
    pub fixed_framing_prompt: &'static str,
    pub allowed_payload_fields: &'static [&'static str],
}

pub fn region_template(kind: PromptSectionKind) -> &'static RegionTemplate;
```

强制映射如下：

| `PromptSectionKind` | `stable_name` | `placement` | `priority` |
|---|---|---|---|
| `IdentityCore` | `identity_core` | `StaticPrefixRequired` | `Highest` |
| `EvidenceToolingPolicy` | `evidence_tooling_policy` | `StaticPrefixRequired` | `Highest` |
| `ToolChoicePriority` | `tool_choice_priority` | `StaticPrefixRequired` | `Highest` |
| `RequestedToolPreferenceTemplate` | `requested_tool_preference_template` | `StaticPrefixRequired` | `High` |
| `RequestedToolPreferenceHint` | `requested_tool_preference_hint` | `DynamicSuffixRequired` | `High` |
| `FailureRecoveryPolicy` | `failure_recovery_policy` | `StaticPrefixRequired` | `High` |
| `ExpandedToolCatalog` | `expanded_tool_catalog` | `DynamicSuffixRequired` | `Low` |
| `TaskProtocolLibrary` | `task_protocol_library` | `StaticPrefixRequired` | `High` |
| `CommunicationPolicy` | `communication_policy` | `StaticPrefixRequired` | `MediumHigh` |
| `BootstrapSoul` | `bootstrap_soul` | `StaticPrefixRequired` | `MediumHigh` |
| `BootstrapIdentity` | `bootstrap_identity` | `StaticPrefixRequired` | `MediumHigh` |
| `BootstrapUser` | `bootstrap_user` | `StaticPrefixRequired` | `MediumHigh` |
| `BootstrapHeartbeat` | `bootstrap_heartbeat` | `StaticPrefixRequired` | `MediumHigh` |
| `BootstrapMemory` | `bootstrap_memory` | `StaticPrefixRequired` | `MediumHigh` |
| `BootstrapAgents` | `bootstrap_agents` | `StaticPrefixRequired` | `MediumHigh` |
| `PlanningInstruction` | `planning_instruction` | `DynamicSuffixRequired` | `High` |
| `RecentTaskContinuity` | `recent_task_continuity` | `DynamicSuffixRequired` | `High` |
| `SessionSummary` | `session_summary` | `DynamicSuffixRequired` | `MediumHigh` |
| `RecentMessages` | `recent_messages` | `DynamicSuffixRequired` | `Medium` |
| `RetrievedMemories` | `retrieved_memories` | `DynamicSuffixRequired` | `MediumLow` |

约束：

1. 一个 `PromptSectionKind` 只能映射一个 template。
2. 不允许同一个 `PromptSectionKind` 在不同 turn 使用不同 title。
3. 不允许同一个 `PromptSectionKind` 在 static 和 dynamic 间来回漂移，除非其 `placement` 明确允许。

### 4.5.5 Region Placement Rules

为避免实现时把区域错误地放到 static prefix 或 dynamic suffix，本方案引入固定 placement 规则。

```rust
pub enum RegionPlacement {
    StaticPrefixRequired,
    DynamicSuffixRequired,
}
```

含义如下：

#### `StaticPrefixRequired`

- 必须进入 `StaticPrefix`
- 不允许按 turn 漂移到 `DynamicSuffix`
- 若其内容天然不稳定，则说明设计错误，应先重构内容来源

适用：

- `IdentityCore`
- `EvidenceToolingPolicy`
- `ToolChoicePriority`
- `RequestedToolPreferenceTemplate`
- `FailureRecoveryPolicy`
- `TaskProtocolLibrary`
- `CommunicationPolicy`
- `BootstrapSoul`
- `BootstrapIdentity`
- `BootstrapUser`
- `BootstrapHeartbeat`
- `BootstrapMemory`
- `BootstrapAgents`

#### `DynamicSuffixRequired`

- 必须进入 `DynamicSuffix`
- 不允许放入 `StaticPrefix`
- 原因是内容天然 session-variant、turn-variant，放入 static prefix 会破坏 cache 稳定性

适用：

- `RequestedToolPreferenceHint`
- `PlanningInstruction`
- `RecentTaskContinuity`
- `SessionSummary`
- `RecentMessages`
- `RetrievedMemories`
- `ExpandedToolCatalog`

### 4.5.6 Extension Rules For New Regions

新增 region 时必须遵循固定扩展流程，不允许临时追加一段 prompt 文本后再“之后整理”。

新增 region 的最小要求：

1. 新增 `PromptSectionKind` 枚举成员
2. 为其注册唯一 `RegionTemplate`
3. 明确 `placement`
4. 明确 `priority`
5. 明确 `allowed_payload_fields`
6. 增加 snapshot 测试验证渲染结果
7. 增加顺序测试验证 canonical order

新增 region 前必须回答三个问题：

1. 这是稳定规则还是动态上下文？
2. 这段内容是否会频繁随 turn 改变？
3. 它是否能覆盖已有高优先级区域？

若答案不清晰，则不允许新增。

禁止的扩展方式：

1. 在 assembler 中直接 append 裸字符串
2. 复用现有 `PromptSectionKind` 但改掉 title 或语义
3. 把 dynamic 内容偷偷塞进 static section payload
4. 用一个宽泛 region 承载多个不同职责，导致优先级失真

### 4.5.7 Final Prompt Shape Contract

本方案最终要求的 prompt 文本形态不能只描述默认 system prompt，还必须描述“最终发给 LLM 的完整层次结构”。

#### Layer 1: Default System Prompt

```text
[StaticPrefix Block]
  # Identity Core
  <fixed framing prompt>
  <payload>

  # Evidence And Tooling Policy
  <fixed framing prompt>
  <payload>

  # Bootstrap: SOUL.md
  <fixed framing prompt>
  <payload>

  ...

[DynamicSuffix Block]
  # Requested Tool Preference Hint
  <fixed framing prompt>
  <payload>

  # Planning Instruction
  <fixed framing prompt>
  <payload>

  # Session Continuity
  <fixed framing prompt>
  <payload>

  # Recent Messages
  <fixed framing prompt>
  <payload>

  ...
```

#### Layer 2: Appended System Context

```text
[AppendedSystemContext Block]
  System runtime context for the current request:
  - project_root: ...
  - git_branch: ...
  - git_status: ...
  - workspace_state: ...
```

要求：

- 这是上下文注入块，不是 region catalog 成员
- 结构应稳定，但内容允许按请求变化
- 不使用 XML-like region 标签

### 4.5.7.1 Appended System Context Contract

`AppendedSystemContext` 必须对齐 `claude-code` 的 system context 类别，承载“当前请求的系统运行态上下文”，而不是 project instructions、用户约束或历史连续性。

职责：

1. 提供当前请求的系统运行背景
2. 解释当前工作区、Git 状态、运行环境约束
3. 为模型理解“当前环境是什么样”提供事实背景

不负责：

1. 用户长期规则
2. 项目协作说明
3. 当前日期说明
4. 任务连续性
5. recent messages
6. retrieved memories

允许字段类别：

- workspace identity
  - `project_root`
  - `workspace_state`
- git state
  - `git_branch`
  - `git_status_summary`
  - `has_uncommitted_changes`
- execution environment
  - `phase`
  - `planner_mode`
  - `prompt_mode`
  - `session_mode`
  - `sandbox_mode`
  - `network_access`
  - `approval_policy`

禁止字段类别：

- `current_date`
- `current_timezone`
- `user_preferences`
- `recent_verified_paths`
- `recent_messages`
- `tool_outputs`
- raw `git diff`

默认渲染格式：

```text
[AppendedSystemContext]
System runtime context for the current request:
- project_root: /workspace/talos
- git_branch: feat/prompt-refactor
- git_status_summary: modified 2 files
- has_uncommitted_changes: true
- session_mode: default
- sandbox_mode: danger-full-access
- network_access: enabled
- approval_policy: never
```

约束：

1. `project_root` 只能出现在 `AppendedSystemContext`，不得同时出现在其他 region payload 中。
2. Git 相关字段只能出现在 `AppendedSystemContext`，不得散落到其他 canonical region。
3. `AppendedSystemContext` 必须保持短小、事实化，不允许混入解释性长文。

#### Layer 3: Prepended User Context Message

```text
[PrependedUserContextMessage]
<system-reminder>
Request-local reminder and current time are shown below.
Use them as high-priority request metadata.

<current-date>
...
</current-date>
</system-reminder>
```

要求：

- 该层以消息注入方式存在，不并入默认 system prompt
- 可借鉴 `claude-code` 的 reminder 风格
- 这里允许使用轻量标签，因为它属于上下文注入消息，不属于 static prefix region

### 4.5.7.2 Prepended User Context Message Contract

`PrependedUserContextMessage` 必须对齐 `claude-code` 的 user context 注入形式，但在 Talos 当前设计下只承载“当前日期 / 当前时区 / request-local reminder”。

职责：

1. 提供当前日期与当前时区
2. 提供 request-local reminder
3. 让模型把这些内容视为高优先级请求元数据

允许字段类别：

- `request_local_reminder`
- `current_date`
- `current_timezone`

禁止字段类别：

- bootstrap file contents
- `project_root`
- `git_branch`
- `git_status_summary`
- `recent_messages`
- `session_summary`
- `retrieved_memories`

默认渲染格式：

```text
[PrependedUserContextMessage]
<system-reminder>
Request-local reminder and current time are shown below.
Use them as high-priority request metadata.

<request-local-reminder>
- Today is the current session date below.
- Use absolute dates when clarifying relative time references.
</request-local-reminder>

<current-date>
2026-04-22
</current-date>

<current-timezone>
Asia/Shanghai
</current-timezone>
</system-reminder>
```

约束：

1. `current_date` 和 `current_timezone` 只能出现在 `PrependedUserContextMessage`。
2. bootstrap file contents 不得出现在 `PrependedUserContextMessage`。
3. 该层是注入消息，不属于 canonical region catalog，不参与 static prefix cache contract。

#### Layer 4: Current User Input

```text
[UserInput]
<current user input>
```

最终整体视图：

```text
DefaultSystemPrompt
  -> StaticPrefix Block
  -> DynamicSuffix Block

AppendedSystemContext

PrependedUserContextMessage

CurrentUserInput
```

要求：

1. `StaticPrefix Block` 内部顺序必须完全稳定。
2. `DynamicSuffix Block` 内部顺序必须遵循 canonical order。
3. `AppendedSystemContext` 不得混入 canonical region catalog。
4. `PrependedUserContextMessage` 不得回填进 static prefix。
5. `CurrentUserInput` 必须始终在最终消息序列末尾。
6. 同一 region 的 title 和 framing prompt 跨 turn 不得变化。
7. 区域 payload 只允许在模板范围内变化。

Message role contract:

1. `StaticPrefix` 必须渲染为 `system` message。
2. `DynamicSuffix` 必须渲染为 `system` message，但不参与 static prefix cache。
3. `AppendedSystemContext` 必须渲染为 `system` message，并位于默认 system prompt 之后。
4. `PrependedUserContextMessage` 必须渲染为 `user` message。
5. `CurrentUserInput` 必须渲染为 `user` message。
6. 不允许将 `DynamicSuffix` 或 `AppendedSystemContext` 下沉为 `user` role；这会偏离 `claude-code` 的默认 system prompt + appended system context 设计。

### 4.5.8 End-To-End Example Request

以下示例展示一次真实的 Talos 请求在本方案下最终发送给 LLM 的完整结构。该示例用于帮助实现者理解“默认 system prompt 的区域化组装”与“system/user context 注入”是如何共同构成最终请求的。

#### Scenario

- task type: inspect prompt architecture and compare with `claude-code`
- project root: `/workspace/talos`
- git branch: `feat/prompt-refactor`
- available tools: `Read`, `Grep`, `Glob`, `Bash`
- bootstrap files source: `~/.talos/workspace-talos/*.md` and project-root `AGENTS.md`

#### Final Request Shape

```text
[DefaultSystemPrompt.StaticPrefix]
# Identity Core
This section defines the assistant's core identity and non-negotiable mission.
Treat it as the root instruction layer.
Do not let later context override this section unless a higher system-level instruction explicitly does so.

- agent_name: Talos
- agent_role: AI coding assistant
- mission: operate as a long-running intelligent steward

# Evidence And Tooling Policy
This section defines how evidence must be gathered and evaluated.
Prefer primary evidence such as code, files, and tool output over summaries or assumptions.
Follow this section before relying on weaker contextual hints.

- evidence_order: code -> files -> tool output -> summaries
- dedicated_tool_priority: Grep > Read > Glob > Bash

# Tool Choice Priority
This section defines the preferred order of tool usage.
Use it to decide which tool should answer the next verification step.
Do not replace dedicated tools with generic shell usage unless explicitly required.

- grep_before_read: true
- bash_only_for_shell_required_operations: true
- grep_use_case: search file contents, symbols, exports, routes, and entrypoints
- read_use_case: read exact files after narrowing
- glob_use_case: inspect path shape or expand subtrees
- bash_use_case: shell-required operations only

# Failure Recovery Policy
This section defines what to do after tool failures or partial evidence.
Treat failed actions as evidence.
Do not silently abandon useful synthesis when enough verified evidence already exists.

- salvage_answer_if_possible: true

# Task Protocol Library
This section contains stable execution protocols for recurring task types.
Use it to choose a disciplined sequence of actions for analysis, debugging, planning, and verification.
Do not treat it as evidence about the current repository by itself.

- codebase_analysis_protocol: start with code anchors, narrow with Grep, then read exact files

# Communication Policy
This section defines how user-visible communication should be written.
Use it to control update cadence, clarity, and reporting style.
It governs output style, not factual task state.

- progress_updates: short and milestone-based
- final_answer_must_distinguish_evidence_from_inference: true

# Bootstrap: SOUL.md
This section contains durable workspace bootstrap instructions loaded from SOUL.md.
Treat it as stable static guidance.
It remains part of the cacheable static prefix.

- Prefer verified facts over confident guesses.
- Progress tasks with minimal user supervision.

# Bootstrap: IDENTITY.md
This section contains durable workspace bootstrap instructions loaded from IDENTITY.md.
Treat it as stable static guidance.
It remains part of the cacheable static prefix.

- Workspace identity overrides may appear here.

# Bootstrap: USER.md
This section contains durable workspace bootstrap instructions loaded from USER.md.
Treat it as stable static guidance.
It remains part of the cacheable static prefix.

- User prefers concise Chinese reasoning.

# Bootstrap: HEARTBEAT.md
This section contains durable workspace bootstrap instructions loaded from HEARTBEAT.md.
Treat it as stable static guidance.
It remains part of the cacheable static prefix.

- Long-running tasks should report progress at meaningful milestones.

# Bootstrap: MEMORY.md
This section contains durable workspace bootstrap instructions loaded from MEMORY.md.
Treat it as stable static guidance.
It remains part of the cacheable static prefix.

- Important constraints: avoid exposing internal strategy knobs as public config.

# Bootstrap: AGENTS.md
This section contains durable workspace bootstrap instructions loaded from AGENTS.md.
Treat it as stable static guidance.
It remains part of the cacheable static prefix.

- Always answer in Chinese.
- Use English for code identifiers and comments.
- Do not add fallback logic.
- Keep implementation explicit and strongly typed.

[DefaultSystemPrompt.DynamicSuffix]
# Requested Tool Preference Hint
This section contains the current turn's explicit tool preferences derived from the user's request.
Use it to bias the next step when the named tool can directly answer the task.
It does not override higher-level tool safety or evidence policy.

- requested_tools: Read, Grep
- next_step_bias: inspect prompt-builder spec and compare with local claude-code sources

# Planning Instruction
This section contains turn-local planning guidance.
Use it to decide the next action sequence for the current task.
Do not let it override the root policy sections.

- current_goal: explain final prompt shape and compare against claude-code

<prompt-region>
<region-name>recent_task_continuity</region-name>
<region-priority>high</region-priority>
<region-role>resume_execution</region-role>
<region-body>
- recent_verified_paths: spec/prompt-builder-claude-code-alignment-design.md, claude-code/src/constants/prompts.ts
- recent_successful_tools: Read, Grep
- recent_failed_tools: none
- recent_answer_mode: direct_answer
</region-body>
</prompt-region>

<prompt-region>
<region-name>recent_messages</region-name>
<region-priority>medium</region-priority>
<region-role>recent_raw_context</region-role>
<region-body>
- user: 静态区域不是不加标签吗？动态区才加标签
- assistant: 对，静态区不使用 XML-like 标签，动态区才允许 envelope
- user: 改一下
</region-body>
</prompt-region>

<prompt-region>
<region-name>retrieved_memories</region-name>
<region-priority>medium_low</region-priority>
<region-role>supplementary_memory</region-role>
<region-body>
- memory: project prefers concise Chinese reasoning
- memory: avoid exposing internal strategy knobs as public config
</region-body>
</prompt-region>

<prompt-region>
<region-name>expanded_tool_catalog</region-name>
<region-priority>low</region-priority>
<region-role>capability_reference</region-role>
<region-body>
- Read: read exact files
- Grep: search file contents
- Glob: inspect path shape
- Bash: shell-required operations
</region-body>
</prompt-region>

[AppendedSystemContext]
System runtime context for the current request:
- project_root: /workspace/talos
- phase: execution
- planner_mode: built_in
- prompt_mode: full
- git_branch: feat/prompt-refactor
- git_status_summary: modified spec and plan files
- has_uncommitted_changes: true
- session_mode: default
- sandbox_mode: danger-full-access
- network_access: enabled
- approval_policy: never
- workspace_state: local repository with prompt architecture design documents

[PrependedUserContextMessage]
<system-reminder>
Request-local reminder and current time are shown below.
Use them as high-priority request metadata.

<request-local-reminder>
- Use absolute dates when clarifying relative time references.
</request-local-reminder>

<current-date>
2026-04-22
</current-date>

<current-timezone>
Asia/Shanghai
</current-timezone>
</system-reminder>

[CurrentUserInput]
继续
```

#### Example Interpretation

该示例体现了本方案的五个关键点：

1. `StaticPrefix` 只使用稳定标题和固定 framing prompt，不使用 XML-like 标签。
2. `DynamicSuffix` 中的高变化区域可以使用 `<prompt-region>` envelope 增强模型识别。
3. `AppendedSystemContext` 承载 `project_root`、执行模式、Git 状态和运行环境约束，不再重复出现在其他 region。
4. bootstrap files 以固定 static 槽位进入 `StaticPrefix`，不做运行时语义拆分。
5. `ExpandedToolCatalog` 仅承载当前回合工具可见性与能力参考，不再承载工具策略。
6. `PrependedUserContextMessage` 只承载 request-local reminder 和当前日期/时区。
7. `AppendedSystemContext` 与 `PrependedUserContextMessage` 都不属于 canonical region catalog。
8. 当前用户输入始终作为最终请求的末尾输入存在。

### 4.6 PromptBlock 渲染层

Talos 需要在 `PromptAssembly` 和 provider request 之间新增 `PromptBlock` 层。

目标不是引入复杂 DSL，而是把“section 列表”聚合为更稳定的请求 block。

建议结构：

```rust
pub enum PromptBlockKind {
    Attribution,
    StaticPrefix,
    DynamicSuffix,
    AppendedSystemContext,
    PrependedUserContextMessage,
    UserInput,
}

pub struct PromptBlock {
    pub kind: PromptBlockKind,
    pub text: String,
    pub cache_scope: PromptBlockCacheScope,
}
```

好处：

1. provider adapter 不再直接理解 section 细节。
2. 可以像 `claude-code` 一样，把 static prefix 聚成稳定块。
3. 未来不同 provider 若支持不同 cache 语义，只需改 adapter。

### 4.7 Cache Scope Contract

Talos 不必照搬 `claude-code` 的 `global/org`，但必须增强自己的 cache scope 语义。

建议从 section 级和 block 级分开：

#### Section-level

- `StaticPrefixEligible`
- `WorkspaceStable`
- `DynamicOnly`

#### Block-level

- `StablePrefix`
- `ScopedReusable`
- `NoCache`

要求：

1. section 的 `cache_scope` 只表达 section 是否允许进入稳定前缀。
2. 最终缓存策略由 renderer 聚合成 block 后统一决定。
3. provider adapter 不应看到零散的“每个 section 都打一遍 cache 标记”的实现。

### 4.8 XML-like Tags 的使用边界

Talos 不应把所有 system prompt 区域都改成 XML 包裹。

建议边界如下：

1. **StaticPrefix**
   必须采用稳定标题 + framing prompt + payload 的纯文本结构。
   不允许使用 XML-like region wrapper。

2. **DynamicSuffix**
   若需要强化机器可读性，可局部使用轻量 XML-like wrapper，例如：

```text
<prompt-region>
<region-name>recent_task_continuity</region-name>
<region-priority>high</region-priority>
<region-role>resume_execution</region-role>
<region-body>
...
</region-body>
</prompt-region>
```

3. **Tool result / reminders / continuity payload**
   可以使用 XML-like tags，但只应服务于动态语义块，不应破坏 static prefix 稳定性。

结论：

- static prefix 禁止 XML-like region 标签化。
- 若要引入 `<xx><xxx>` 风格结构，只允许用于 dynamic region envelope。

---

## 5. 模块改造方案

### 5.1 `types.rs`

需要新增或调整：

1. `PromptBlockKind`
2. `PromptBlock`
3. 更强语义的 `PromptBlockCacheScope`
4. `PromptAssembly` 中的 block 渲染入口辅助结构

建议：

- 保留现有 `PromptSection`
- 不新增平行的“第二套 section 类型”

### 5.2 `assembler.rs`

需要承担以下责任：

1. 所有 section 构造必须走统一 registry/order。
2. 所有静态 section 输出格式统一成：

```text
<stable title>
<fixed framing prompt>
<payload>
```

3. 动态区 section 也采用统一格式，但允许 payload 缺省。
4. assembler 只负责 section，不直接考虑 provider block。

### 5.3 `renderer.rs`

renderer 需要分两步：

1. `PromptAssembly -> Vec<PromptBlock>`
2. `Vec<PromptBlock> -> Vec<ChatMessage>`

第一步是本方案的关键变化。

目标 block 结构：

```text
┌──────────────────┐
│ Attribution?     │ optional
├──────────────────┤
│ StaticPrefix     │ stable order, stable serialization
├──────────────────┤
│ DynamicSuffix    │ mutable per turn
├──────────────────┤
│ AppendedSystemContext │ request-scoped runtime facts
├──────────────────┤
│ PrependedUserContextMessage │ request-scoped reminder/time metadata
├──────────────────┤
│ UserInput        │ last
└──────────────────┘
```

Message role mapping:

- `Attribution` -> `system`
- `StaticPrefix` -> `system`
- `DynamicSuffix` -> `system`
- `AppendedSystemContext` -> `system`
- `PrependedUserContextMessage` -> `user`
- `UserInput` -> `user`

### 5.4 Provider Adapter

本方案要求 provider adapter 层支持：

1. 接收 block 级缓存语义
2. 将 `StablePrefix` 与 `NoCache` 明确区分
3. 不要求当前 provider 立即支持 `claude-code` 同等级语义
4. 但接口必须先为未来 provider cache 优化留出位置

---

## 6. 核心 API 草案

### 6.1 Section Construction

```rust
pub struct PromptSectionTemplate {
    pub kind: PromptSectionKind,
    pub title: &'static str,
    pub framing_prompt: &'static str,
    pub cache_scope: SectionCacheScope,
    pub tier: BudgetTier,
    pub required: bool,
}

pub struct PromptSection {
    pub template: &'static PromptSectionTemplate,
    pub payload: String,
    pub role: Option<TalosMessageRole>,
    pub cache_scope: SectionCacheScope,
    pub tier: BudgetTier,
    pub required: bool,
    pub estimated_tokens: usize,
    pub truncation_mode: TruncationMode,
}
```

说明：

- `title` 和 `framing_prompt` 不应作为运行时自由字符串存在，而应来自唯一模板。
- `PromptSection` 只负责承载模板引用与动态 payload，避免实现层偷偷改写 title 或 framing。

### 6.2 Assembly To Blocks

```rust
pub struct RequestContextInjection {
    pub appended_system_context: String,
    pub prepended_user_context_message: String,
}

pub struct PromptAssembly {
    pub static_prefix_sections: Vec<PromptSection>,
    pub dynamic_suffix_sections: Vec<PromptSection>,
    pub request_context_injection: RequestContextInjection,
    pub user_input: String,
    pub diagnostics: PromptDiagnostics,
}

pub enum PromptBlockKind {
    Attribution,
    StaticPrefix,
    DynamicSuffix,
    AppendedSystemContext,
    PrependedUserContextMessage,
    UserInput,
}

pub struct PromptBlock {
    pub kind: PromptBlockKind,
    pub text: String,
    pub cache_scope: PromptBlockCacheScope,
}

pub fn render_prompt_blocks(assembly: &PromptAssembly) -> Vec<PromptBlock>;
pub fn render_chat_messages(blocks: &[PromptBlock]) -> Vec<ChatMessage>;
```

说明：

- `PromptAssembly` 不仅包含默认 system prompt 的 static/dynamic sections，还必须显式携带 request-level context injection。
- `AppendedSystemContext` 和 `PrependedUserContextMessage` 不应在 renderer 阶段临时现拼，而应作为 `PromptAssembly` 的正式输入组成部分。

### 6.3 Cache Break Reporting

```rust
pub enum PromptCacheBreakReason {
    PromptVersionChanged,
    ToolCatalogChanged,
    BootstrapChanged,
    ModelChanged,
    ManualClear,
    ManualCompact,
}

pub struct PromptCacheState {
    pub static_prefix_fingerprint: String,
    pub last_break_reason: Option<PromptCacheBreakReason>,
}
```

---

## 7. 实施阶段

### 阶段 1: Freeze Section Contract

**目标**: 固化 `PromptSection` 是唯一编排单元，并确定 canonical order。  
**成功标准**: `types.rs` 与 `assembler.rs` 不再允许隐式顺序拼接。  
**测试**: 单测校验 section 顺序稳定。

### 阶段 2: Introduce PromptBlock Layer

**目标**: 在 `PromptAssembly` 与 provider message 间增加 `PromptBlock`。  
**成功标准**: renderer 可输出 block，并保持现有行为等价。  
**测试**: 单测校验 static prefix 与 dynamic suffix 被正确聚合。

### 阶段 3: Normalize Region Framing

**目标**: 所有核心 section 统一成“title + framing + payload”格式。  
**成功标准**: 静态区和动态区 section 文案结构一致。  
**测试**: snapshot 测试校验 prompt 结构稳定。

### 阶段 4: Add Cache Contract

**目标**: 为 static prefix 引入显式 fingerprint 和 cache break reason。  
**成功标准**: static prefix 变化可观测。  
**测试**: 单测覆盖工具变化、模型变化、bootstrap 变化触发 cache break。

### 阶段 5: Optional Dynamic Region Envelope

**目标**: 为部分 dynamic region 增加轻量 XML-like envelope。  
**成功标准**: 不影响 static prefix 稳定性，只增强 dynamic 语义。  
**测试**: snapshot 测试与 provider request 对比。

---

## 8. ASCII 流程图

```text
                         ┌──────────────────────┐
                         │    PromptBuilder     │
                         └──────────┬───────────┘
                                    │
                                    ▼
                         ┌──────────────────────┐
                         │   PromptSection[]    │
                         │ canonical ordering   │
                         └──────────┬───────────┘
                                    │
                                    ▼
                         ┌──────────────────────┐
                         │   PromptAssembly     │
                         │ static + dynamic     │
                         └──────────┬───────────┘
                                    │
                                    ▼
                         ┌──────────────────────┐
                         │    PromptBlock[]     │
                         │ prefix / suffix      │
                         └──────────┬───────────┘
                                    │
                                    ▼
                         ┌──────────────────────┐
                         │ provider adapter     │
                         │ cache-aware render   │
                         └──────────┬───────────┘
                                    │
                                    ▼
                         ┌──────────────────────┐
                         │ model request blocks │
                         └──────────────────────┘
```

---

## 9. 模块接口清单

| 模块 | 责任 | 关键变更 |
|---|---|---|
| `common/prompt/types.rs` | section、assembly、block、cache contract 类型定义 | 新增 `PromptBlock*`，增强 cache scope |
| `common/prompt/assembler.rs` | section 构造与 canonical ordering | 引入统一 section registry 和 framing/payload 结构 |
| `common/prompt/renderer.rs` | assembly 到 provider blocks/messages 渲染 | 从逐 section 渲染升级为 block 分组渲染 |
| `common/prompt/tests.rs` | prompt 架构稳定性验证 | 增加顺序、snapshot、cache break 测试 |
| `llm provider adapter` | provider 请求组装 | 消费 block 级缓存语义 |

---

## 10. 风险与权衡

### 10.1 主要收益

1. static prefix 更稳定，后续更容易命中模型 cache。
2. prompt 结构更清晰，便于持续治理。
3. 未来引入 compaction、dynamic prompt trimming、provider cache 优化时，边界更清楚。
4. 可以独立实施，不依赖上下文压缩系统先落地。

### 10.2 主要风险

1. 若一次性重写所有 section 文案，容易引发模型行为漂移。
2. 若过早引入全量 XML-like tags，可能让 static prefix 不稳定。
3. 若 block 层引入后 provider adapter 未同步，可能造成行为倒退。

### 10.3 风险控制

1. 先引入 `PromptBlock` 层，再调整 section 文案。
2. 先固化 static prefix，再讨论 dynamic XML envelope。
3. 所有关键结构变化都需要 snapshot 测试与 e2e 验证。

---

## 11. 结论

Talos 可以对齐 `claude-code` 的 prompt architecture，但不应机械复制其 TypeScript 实现。

正确路线是：

1. 保留 Talos 当前 `PromptSection` / `PromptAssembly` 抽象。
2. 新增 `PromptBlock` 层，建立更清晰的 render boundary。
3. 固化 canonical region order 与 static prefix contract。
4. 让每个 region 拥有固定 framing prompt，而不是仅作为数据容器。
5. 把 XML-like 结构限制在 dynamic region，而不是全量 prompt。

这条路线既能复用 `claude-code` 的成熟经验，又能保持 Talos 当前 Rust prompt 模块的连续性与可维护性。
