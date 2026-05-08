# Context Window Threshold Compaction Governance Design

## 1. 背景

Talos 当前的上下文治理已经进入第二阶段：

1. PromptBuilder 架构已经完成一次较大重构，默认请求装配已经稳定为：
   - `StaticPrefix`
   - `DynamicSuffix`
   - `AppendedSystemContext`
   - `PrependedUserContextMessage`
   - `UserInput`
2. builder 阶段已经具备窗口预算计算、动态区裁剪、bootstrap 固定槽位注入、prompt warning 诊断。
3. `general_turn::prepare` 阶段已经具备一次“历史压缩再组装”的能力：在窗口超限或历史区被裁掉时，先 flush memory，再用收缩后的 `PromptLoadPolicy` 重建 prompt。

但是，旧版 `context-window` 方案仍混杂了三类已经过期的表述：

1. 把已经部分或全部落地的能力继续写成“未来态”，或混入旧状态名，例如把不存在的 `CompactedRecovery` 写进当前 session 状态机。
2. 还在引用已经删除或重构的 prompt 区域，例如 `RuntimeSessionContext`、`ProjectContext`。
3. 把“未来想删除的配置”写成了“当前已经删除”，与现有实现和配置文件不一致。

本次重写目标不是重新发明一套新架构，而是：

- 以当前已落地实现为基线；
- 统一更新过期的 prompt 区域、装配链路和 warning 语义；
- 明确哪些是现状，哪些是后续演进要求；
- 为后续继续收敛配置和补完更强的恢复协议提供干净基线。

相关 PromptBuilder 架构、region catalog、message role contract 不在本方案重复定义，统一以独立文档为准：

- [prompt-builder-claude-code-alignment-design.md](/Users/xujiang/data/ailabs/talos/spec/prompt-builder-claude-code-alignment-design.md)

---

## 2. 文档范围

本方案只定义 Talos 的上下文窗口治理与压缩策略，重点覆盖：

1. `context_window` 的来源与预算语义。
2. builder 阶段的预算裁剪行为。
3. `prepare` 阶段的历史压缩触发与恢复基线。
4. warning、诊断和配置项的治理边界。

本方案不重复定义：

1. PromptBuilder 的静态/动态 region 模板。
2. `PromptBlock`、`PromptBlockCacheScope`、message role contract。
3. bootstrap 文件如何映射到 static slots 的细节。

---

## 3. 当前实现基线

### 3.1 当前请求装配链路

当前 Talos 的请求构建链路如下：

```text
PromptBuilder
    -> PromptAssembly
        -> static_prefix_sections
        -> dynamic_suffix_sections
        -> request_context_injection
            -> appended_system_context
            -> prepended_user_context_message
        -> user_input
    -> PromptBlock[]
    -> ChatMessage[]
```

当前 message role 已严格对齐 `claude-code` 模式：

- `StaticPrefix` -> `system`
- `DynamicSuffix` -> `system`
- `AppendedSystemContext` -> `system`
- `PrependedUserContextMessage` -> `user`
- `UserInput` -> `user`

这意味着：

1. 上下文治理不能再假设“动态内容全部走 user role”。
2. `DynamicSuffix` 仍是 default system prompt 的动态部分，只是不参与 stable prefix cache。
3. `AppendedSystemContext` 是 request-scoped runtime facts，但仍属于 system prompt 体系的一部分。

### 3.2 当前窗口预算计算

当前 builder 仍采用显式预算模型：

```text
effective_prompt_budget
  = context_window
  - reserved_output_tokens
  - compaction_buffer_tokens
```

实现结构对应：

- `PromptWindowUsage.context_window`
- `PromptWindowUsage.reserved_output_tokens`
- `PromptWindowUsage.compaction_buffer_tokens`
- `PromptWindowUsage.effective_prompt_budget`
- `PromptWindowUsage.static_prefix_tokens`
- `PromptWindowUsage.dynamic_suffix_tokens`
- `PromptWindowUsage.total_tokens`
- `PromptWindowUsage.within_effective_budget`

这一层目前是**已实现基线**，不是未来态设想。

### 3.3 当前 builder 阶段裁剪行为

当前 `assemble()` 流程中，已落地的窗口治理行为如下：

1. 先构建 `static_prefix_sections` 与 `dynamic_suffix_sections`
2. 估算 token
3. 若 `total_tokens > effective_prompt_budget`
   - 仅裁剪 `DynamicSuffix`
   - 通过 `trim_dynamic_suffix_to_budget()` 进行低优先级 drop
4. 重新计算 `PromptWindowUsage`
5. 输出 `PromptDiagnostics`
6. 后续由 `prepare` 阶段基于 diagnostics 派生并发出 `PromptWarning`

这里有两个硬约束已经体现在实现中：

1. static prompt 不参与 last-resort drop。
2. 允许被裁掉的只有 dynamic 区域，例如：
   - `ExpandedToolCatalog`
   - `RecentMessages`
   - `RecentToolHistory`
   - `RecentToolOutputs`
   - `RetrievedMemories`
   - 低优先级 `SessionSummary`

当前 `DynamicSuffix` 的裁剪优先级以显式常量 `DYNAMIC_SUFFIX_DROP_ORDER` 为准，不应从 `BudgetTier` 自动推导。当前顺序如下：

1. `ExpandedToolCatalog`
2. `RetrievedMemories`
3. `RecentToolOutputs`
4. `RecentToolHistory`
5. `RecentMessages`
6. `SessionSummary`
7. `RequestedToolPreferenceHint`
8. `RecentTaskContinuity`

解释：

- `ExpandedToolCatalog` 属于动态工具可见性参考，最先可牺牲。
- `RetrievedMemories` 属于辅助背景，其次可牺牲。
- `RecentMessages`、`RecentToolHistory`、`RecentToolOutputs` 与 `SessionSummary` 属于历史上下文，优先级高于工具目录和 memory，但低于 turn-local intent 和 continuity。
- `RequestedToolPreferenceHint` 与 `RecentTaskContinuity` 属于当前轮执行偏好与恢复所需信息，只在预算持续超限时才进入 drop 路径。
- `PlanningInstruction` 当前是 required section，不参与普通 builder trim，不应写入 drop 顺序。
- 同为 `Important` 或 `Supplemental` 的 section，实际删除顺序仍以显式 drop 常量为准，而不是按 tier 自动排序。

补充原则：

- `PromptDynamicSectionDropped` 是低等级退化信号，不应被视为主压缩策略。
- 常规治理路径应优先通过 summary / memory flush / context compaction 收缩上下文。
- 只有在 builder 已经必须满足即时预算约束时，才进入 dynamic section drop。

收敛目标必须进一步明确：

- 一旦预置 compaction 路径补齐，首次触发上下文压力时不应直接进入 dropped path
- 正常顺序应为：
  1. 先尝试 summary / memory flush / history compaction
  2. 仅当窗口仍超限，或 compaction 无法产出可接受结果时
  3. 才进入 `DynamicSuffix` section drop

换句话说：

- `PromptDynamicSectionDropped` 不应成为“达到压缩阈值后的默认第一反应”
- 它只能是“压缩后仍无法满足预算”的最后退化手段

### 3.4 当前 prepare 阶段压缩行为

当前 `general_turn::prepare` 中，已落地的第二层治理不是完整状态机，而是一次“压缩后重组装”流程：

```text
build prompt
    -> if within budget and history not dropped
        -> continue
    -> else
        -> flush session/workspace memory
        -> rebuild prompt with compacted load policy
        -> if summary exists after rebuild
            -> accept compacted assembly
```

当前触发条件是：

1. `window_usage.window_resolved == true`
2. 且满足以下任一条件：
   - `within_effective_budget == false`
   - `PromptAssembly.diagnostics.dropped_sections` 中包含任一历史类 section：
     - `RecentMessages`
     - `SessionSummary`

当前 compact 后的缩减策略主要体现在 `build_compacted_prompt_load_policy()`：

- `include_session_summary = true`
- `include_recent_messages = true`
- `include_recent_tool_history = true`
- `include_recent_tool_outputs = true`
- `session_recent_limit = session_summary_recent_count.max(1)`
- `recent_tool_history_limit = tool_exchange_keep_last.max(1)`
- `recent_tool_outputs_limit = tool_output_keep_last.max(1)`
- `include_retrieved_memories` 沿用配置
- compact 输入过长时，先裁旧 `tool output`，再裁旧 `assistant/tool exchange`，最后才裁最老 `history turn`

这说明当前实现的确定性质是：

- “memory flush + history reload + layered compaction + summary retention”

而不是旧文档里写的：

- 完整的 `SessionContinuity JSON compact model` 路径
- 冻结 backlog
- `CompactedRecovery` 状态恢复

本文对治理优先级的确定定义如下：

1. 优先保住 summary / compact 这类结构化收缩路径
2. `PromptDynamicSectionDropped` 只是 builder 为了满足即时预算做的低等级退化
3. 若一个 session 持续反复出现 dynamic drop，应优先排查 compaction 是否未及时触发，而不是默认接受 drop 成为常态

本文对目标治理顺序的固定定义如下：

1. 超过压缩触发阈值时，优先进入 preemptive compaction
2. 仅当 compaction 失败、未触发、或 compaction 后仍超预算时，才允许 section drop
3. 因此 `PromptDynamicSectionDropped` 在成熟路径里应是“后置告警”，而不是“首跳告警”

### 3.5 当前 continuity 基线

当前已实现的 `SessionContinuity` 已从纯字符串列表升级为一版更强类型的连续性结构，但还不是早期方案里设想的完整 compact summary：

```rust
pub struct SessionContinuity {
    pub recent_goal: String,
    pub verified_paths: Vec<VerifiedPathEvidence>,
    pub tool_summaries: Vec<ContinuityToolSummary>,
    pub pending_tasks: Vec<String>,
    pub recent_unhelpful_patterns: Vec<String>,
    pub recent_answer_mode: Option<ContinuityAnswerMode>,
    pub last_failure: Option<String>,
    pub suggested_next_step: Option<String>,
}
```

这意味着当前 continuity 的职责是：

- 为下一轮提供“最近目标、路径证据、工具结果和回答模式”的短期连续性
- 为恢复路径提供最小可执行真源：
  - 最近目标
  - 最近失败
  - 待继续项
  - 稳定下一步建议
- 让恢复 prompt 能区分“看过哪些路径”与“哪些工具成功/失败过”

而不是：

- 承担完整任务恢复的唯一强类型真源

当前已经落地的强类型成员包括：

- `VerifiedPathEvidence { path, source_tool }`
- `ContinuityToolSummary { tool_name, outcome }`
- `pending_tasks: Vec<String>`，按确定规则生成最小待继续项
- `suggested_next_step: Option<String>`，按确定规则生成恢复提示
- `SessionMemorySummary` / `MemoryPersistenceDecision` 的输出校验失败路径，当前也已落地：
  - `SessionMemorySummary.title` 不能为空
  - `MemoryPersistenceDecision.rationale` 不能为空
  - JSON 合法但结构校验失败时，会走 retry，而不是被当作成功 compact output 接受

这部分已经是当前 continuity 基线，不能再写成“尚未落地”。

旧文档里那套更重的 `VerifiedEvidence / ToolCallSummary` 命名与结构，不属于当前系统依赖，也不应再写成当前实现真源。

### 3.6 当前 warning family 基线

当前窗口治理支持以下四类 prompt 相关 warning family：

1. `PromptBootstrapFileMissing`
2. `PromptBootstrapFileTruncated`
3. `PromptBootstrapFileDropped`
4. `PromptDynamicSectionDropped`

其中当前行为已经更新为：

1. bootstrap warning 会按稳定 key 去重：
   - `missing:<kind>:<path>`
   - `truncated:<kind>:<path>`
   - `dropped:<kind>:<path>`
2. dynamic section warning 也已按 section 列表去重：
   - `dynamic_dropped:<SectionA,SectionB>`

这和旧文档里“dynamic warning 不去重”的表述已经不一致，必须以当前实现为准。

补充说明：

- 从当前主路径看，最常见的是 `PromptBootstrapFileTruncated`、`PromptBootstrapFileDropped` 与 `PromptDynamicSectionDropped`
- `PromptDynamicSectionTruncated` 不属于当前实现，也不应再出现在 warning 清单、dedup key 或协议说明里
- 因此文档中提到 warning family 时，应理解为“系统支持的告警类型”，不等于“线上同频出现的主症状”

### 3.7 当前 `AppendedSystemContext` 字段边界

当前 `AppendedSystemContext` 已经对齐为“事实型 runtime context”，实际字段范围包括：

- `project_root`
- `git_branch`
- `git_status_summary`
- `has_uncommitted_changes`
- `phase`
- `planner_mode`
- `prompt_mode`
- `sandbox_mode`
- `approval_policy`
- `session_directory`
- `connection_mode`
- `workspace_state`

当前文档需要特别避免再写入以下旧字段：

- `session_mode`
- `network_access`

这两个字段在最近一轮实现里已经移除，因为它们此前没有稳定真实来源，属于伪事实。

---

## 4. 当前主要问题

### 4.1 `UseModelDefault` 上限修复已落地，模型画像语义仍需收敛

当前最大的现实问题不再是 `UseModelDefault` 完全失控，而是模型画像语义仍可能把 budget 认知带偏。

当前默认配置仍是：

- `reserved_output_tokens.kind = use_model_default`
- `compaction_buffer_tokens = 4096`
- 许多模型元数据仍是：
  - `context_window = 32768`
  - `max_tokens = 81920`

旧问题在于：若 `UseModelDefault` 直接取 `model_max_output_tokens`，则会得到：

```text
effective_prompt_budget = 32768 - 81920 - 4096
```

这一类预算失真在当前实现里已经修正。当前逻辑为：

```rust
reserved_output_tokens =
    min(model_max_output_tokens, derive_reserved_output_tokens(context_window))

derive_reserved_output_tokens(context_window) =
    (context_window / 8).clamp(1024, 8192).min(context_window)
```

同时，`compaction_buffer_tokens` 也已带有防御性上限：

```rust
compaction_buffer_tokens = min(configured_buffer_tokens, context_window / 4)
```

因此，文档不能再把这一问题写成“尚未修正”。

当前仍需治理的 residual gap 在于：

1. 模型画像中的 `max_tokens` 语义仍需收敛
   - 若 provider 返回的 `max_tokens` 表示“理论最大输出能力”，而不是 prompt budget 里的“建议预留输出预算”，则 registry 层仍需明确其语义
2. 配置与模型画像的组合仍需监控
   - 即使已有保守上限，异常画像或极端配置仍可能过早触发 history compaction 或 fallback trim
3. 相关排障应优先检查 budget 解析结果，而不是再把负预算推导当作当前现象

结论：

- `UseModelDefault` 的保守上限修复已经是现状基线
- 后续重点应转向模型元数据语义治理与观测，而不是继续把该修复列为“未落地”

### 4.2 bootstrap 文件仍采用字符上限截断

当前 static bootstrap slots 已经稳定，但实际注入仍受配置控制：

- `bootstrap.max_chars_per_file`

且当前实现中，builder 侧实际使用的是 `max_chars_per_file` 截断单文件内容。

这意味着：

1. `AGENTS.md`、`SOUL.md` 等大文件仍会触发 `PromptBootstrapFileTruncated`
2. 这不是异常，而是当前已设计行为
3. 旧文档中“bootstrap 全部改为预计算摘要，不再受字符限额控制”的说法并不符合现状

### 4.3 动态区压力仍主要集中在三类内容

当前最常见的 dynamic 压力来源仍是：

1. `RecentMessages`
2. `SessionSummary`
3. `ExpandedToolCatalog`

其中：

- `RecentMessages` 已经聚合为单一 region，而不是一条消息一个 section
- `ExpandedToolCatalog` 现在承担的是动态工具可见性注入，不是普通工具说明区，不能从设计上删除

### 4.4 session 级 compaction 状态机已落地，恢复协议仍待增强

旧文档把完整的：

- `Normal`
- `Compressing`
- `CompactedRecovery`
- `Blocked`

写成了系统当前状态机，但其中的 `CompactedRecovery` 并不是当前实现的真实状态名。

当前现实情况是：

1. 已有 prompt 级诊断与 history compaction
2. 已有 session reset API
3. 已落地显式 `Normal / Compressing / Recovery / Blocked` session state：
   - 当 `prepare` 判定需要进入 history compaction 时，会先把 session state 标记为 `Compressing`
   - 若 compact 成功并完成一次 turn，结束时会把 session state 写回 `Recovery`
   - 若没有发生成功 compaction，则会回到 `Normal`
   - 当 compact + fallback drop 后仍无法恢复预算时，当前 session 会被标记为 `Blocked`
   - 后续 turn 在 `prepare` 入口会直接拒绝继续执行，直到 `reset_session` 或新建 session
4. `reset_session` 已能通过清空 transcript/state delta 的方式清掉已重放的 compaction state
5. 已有最小 backlog / resume gate：
   - agent 层同 session 第二个请求不再直接报错，而是进入 session-local backlog
   - 当前 active turn 结束后，会自动拉起下一个 queued turn
   - `cancel_active_turn` 不再提前移除 active slot，而是只发 abort，真正的 slot 释放与 backlog 恢复由任务退出路径统一处理

因此本次重写后，文档必须区分：

- `Normal / Compressing / Recovery / Blocked` 已是现状基线
- backlog / resume 的最小串行队列已是现状
- 更强的“显式恢复未完成任务协议”仍是下一阶段目标

---

## 5. 治理原则

### 5.1 单一观测真源

Talos 的上下文治理真源应始终围绕 `context_window` 展开：

```text
context_usage = estimated_input_tokens / context_window
```

但当前实现中，直接用于执行裁剪的仍是：

```text
total_tokens <= effective_prompt_budget
```

因此文档上必须明确区分：

1. **当前执行判定**
   - 基于 `effective_prompt_budget`
2. **治理观测指标**
   - 基于 `context_usage`

两者不是冲突，而是分层：

- `context_usage` 用于统一治理判断和阈值讨论
- `effective_prompt_budget` 用于当前 builder 的具体裁剪计算

### 5.2 动态区优先治理

当前架构已经确定了以下硬约束：

1. 任何 last-resort drop 仅允许作用于 `DynamicSuffix`
2. `StaticPrefix` 不允许因窗口压力被 drop
3. bootstrap static slots 也不允许被动态移出 static 区

因此今后所有 context-window 治理都必须建立在以下事实之上：

- 真正可被压缩和裁剪的，是 dynamic system prompt 与会话历史上下文

而不是：

- stable prefix policy
- bootstrap static files
- message role contract

### 5.3 分层治理，而非单一末端 drop

Talos 当前和后续都应坚持多层治理，而不是只在末端做 section drop：

1. builder 具备预算裁剪能力
2. prepare 阶段 history compaction
3. session-level state gate 与 recovery 标记
4. 后续再补任务级恢复协议与更强 backlog 可观测性

旧文档里“只有到最后才 drop”的表述太理想化，不符合当前实现。当前实际上已经是“三层治理 + 诊断告警”。

但治理主次必须明确：

- `history compaction`、`summary retention`、`memory flush` 是常规收缩手段
- `dynamic section drop` 只是 budget emergency fallback
- 若系统长期依赖 drop 才能工作，应视为配置或 compact 策略异常
- 在预置 compaction 路径补齐后，达到压缩阈值不应直接触发 section drop
- 只有“窗口仍超限且无法继续通过 compact 收缩”时，才允许主动 drop 一部分 dynamic sections

### 5.4 借鉴 Codex 的 context trimming 策略

Talos 后续改造 compact 路径时，应明确借鉴 Codex 的核心思路：

1. 优先裁剪旧上下文单元，而不是优先 drop prompt section
2. compact 本身若因上下文过长无法产出结果，应先缩短输入，再重试 compact
3. section drop 只作为 compact exhausted 之后的最后兜底

这里的“旧上下文单元”应按以下粒度理解：

1. 旧 tool output 内容
2. 旧 assistant/tool exchange
3. 最老的 compactable history item

不应优先按以下粒度处理：

1. `StaticPrefix` policy regions
2. bootstrap static slots
3. 整块 `DynamicSuffix` region

因此，Talos 的目标顺序应固定为：

1. preemptive compaction
2. 若 compact 输入仍过长，则做 Codex-style old context trimming
3. 重新尝试 compact
4. 仅当 compact 仍无法把窗口拉回预算内时，才允许 `DynamicSuffix` section drop

这条原则的意义是：

- prompt section 是语义层结构，不应成为首选裁剪单位
- 旧 tool output / 旧 history item 才是更接近“可牺牲证据载荷”的收缩对象
- 这样更接近 Codex / Claude Code 的主路径，也更有利于保住 prompt 结构稳定性

---

## 6. 阈值模型

### 6.1 治理阈值基线

本文采用以下阈值作为统一运维与后续实现基线：

- `compress_trigger_ratio = 0.85`
- `effective_limit_ratio = 0.90`
- `hard_limit_ratio = 0.95`

这些阈值当前属于经验基线，不是数学推导常量。当前实现中，`agent.kernel.compaction.trigger_ratio_percent` 与 `agent.kernel.compaction.hard_ratio_percent` 已接入 `general_turn::prepare` 的 runtime trigger 判断；默认值仍分别为 `85` 和 `95`。来源是：

1. 参考 `claude-code` 一类 coding agent 的 context governance 经验值
2. 结合 Talos 当前 builder trim 与 prepare compaction 的两层结构，给出便于运维观察的三区间模型

阈值允许调整，但调整前提必须是：

- 有明确的 prompt diagnostics 数据
- 能解释为什么当前阈值导致过早裁剪或过晚压缩
- 调整后同步更新文档与默认配置

含义如下：

1. `< 0.85`
   - 正常区
   - 以 builder 正常装配为主
2. `0.85 ~ 0.90`
   - 主动压缩区
   - 应优先收缩动态历史与 memory 载荷
3. `0.90 ~ 0.95`
   - 高压区
   - 应进入 history compaction 路径
4. `>= 0.95`
   - 硬限制区
   - 当前优先进入更激进的 history compaction 触发
   - 若 compaction 后仍超预算，才可能进入 `Blocked`

### 6.2 与当前实现的对应关系

当前实现中，这三个阈值仍不是一套独立状态机常量，但已经映射到实际运行时行为：

1. builder 阶段：
   - 当调用方显式传入 `DynamicTrimPolicy::TrimToBudget` 且 `total_tokens > effective_prompt_budget` 时，才会裁剪 dynamic 区
2. prepare 阶段：
   - 当 prompt 超预算、达到 `hard_ratio_percent`、达到 `trigger_ratio_percent`，或历史区已被裁掉时，尝试 history compaction

因此当前文档中的阈值应理解为：

- 已有部分 runtime 映射的治理阈值

而不是：

- 已经 1:1 覆盖 builder / prepare / blocked 全部路径的统一状态机常量

更明确地说：

1. builder 当前真正执行的是 `effective_prompt_budget` 判定
2. prepare 当前已经执行 `trigger_ratio_percent / hard_ratio_percent / 超预算 / 历史区已被裁掉` 四类 compaction 触发
3. 尚未完成的是把 `0.85 / 0.90 / 0.95` 进一步收敛成覆盖全部 blocked / recovery 行为的统一状态机

### 6.3 `context_window` 来源

`context_window` 的解析顺序保持如下：

1. 优先使用 provider / model registry 提供的 `ModelInfo.context_window`
2. 若缺失，则视为模型画像不完整，属于错误配置

当前实现中若 `context_window == 0`，`window_resolved` 会是 `false`，builder 会退化为“不执行有效窗口判定”。

这是当前兼容行为，但本文将其明确归类为异常兼容状态：

- 仅允许短期容错
- 不应成为长期可接受状态

本文的目标约束是：

- `context_window` 缺失时，模型选择或启动阶段直接报错
- `window_resolved = false` 不能作为长期可接受常态

---

## 7. 当前压缩链路

### 7.1 Layer 0: Builder-Time Dynamic Trim Capability

这是当前已稳定实现的 builder 能力，但是否在某条主路径上立即触发，取决于调用方传入的 `DynamicTrimPolicy`。

流程：

```text
assemble prompt
    -> estimate static/dynamic tokens
    -> resolve PromptWindowUsage
    -> if dynamic_trim_policy == TrimToBudget
       and over effective budget
        -> trim DynamicSuffix only
        -> if any optional section was removed
            -> emit dropped section diagnostics
```

特性：

1. 只裁 dynamic
2. 优先裁低优先级 dynamic region
3. 直接反映到 `PromptDiagnostics`
4. 若实际移除了 optional dynamic section，才会产出 `PromptDynamicSectionDropped`
5. 在 `general_turn::prepare` 主路径中，首轮 build 默认使用 `DynamicTrimPolicy::PreserveAll`
6. `TrimToBudget` 不是首轮默认行为，而是在 compaction 之后预算仍未恢复时，作为 fallback rebuild 触发

### 7.2 Layer 1: History Compaction In Prepare

这是当前已落地的第二层治理。

触发条件：

1. 窗口已超预算
2. 或 prompt 已经靠 drop 历史区才勉强过线

动作：

1. flush session memory
2. 可选 flush workspace memory
3. 以更收缩的 `PromptLoadPolicy` 重建 prompt
4. 若有 `SessionSummary` 可用，则接受 compacted assembly

这层压缩的本质是：

- 不重写默认 prompt 架构
- 通过 memory flush + 更短 recent history 实现 prompt 重组

补充说明：

1. 当前 compact helper 已支持在 compact 输入预算过长时，先裁最老的 transcript unit，再重试 compact
2. 当前记录会通过 `trimmed_context_unit_count` 与 `trimmed_context_unit_kind=OldestHistoryItem` 进入 `SessionHistoryCompaction`

当前仍缺少的一步是更细粒度的 Codex-style trimming，而不是“完全没有 compact retry”：

1. compact 输入过长时先按 `old tool output` / `old assistant-tool exchange` 等更细粒度单元裁剪
2. 对不同 compactable unit 采用更递进的重试策略
3. 在 richer retry exhausted 后，再进入 section drop

---

## 8. 当前配置治理

### 8.1 当前仍存在的配置项

与旧版文档不同，这些配置项当前**并未删除**，仍然是系统行为真源的一部分：

| 配置 | 当前作用 |
| --- | --- |
| `agent.kernel.prompt.window.reserved_output_tokens` | 参与 `effective_prompt_budget` 计算 |
| `agent.kernel.prompt.window.compaction_buffer_tokens` | 参与 `effective_prompt_budget` 计算 |
| `agent.kernel.compaction.model` | 覆盖 compact / summary / judge 请求使用的模型 |
| `agent.kernel.compaction.recent_messages_keep` | 控制 prompt 中保留的近期 history 数量 |
| `agent.kernel.compaction.recent_messages_max_chars` | 控制常规 prompt 中 recent messages 总字符预算 |
| `agent.kernel.compaction.compacted_recent_messages_keep` | 控制 compact / recovery 后保留的近期 history 数量 |
| `agent.kernel.compaction.compacted_recent_messages_max_chars` | 控制 compact / recovery 后 recent messages 总字符预算 |
| `agent.kernel.compaction.recent_summaries_keep` | 控制注入的 session summary 条数 |
| `agent.kernel.compaction.summary_max_chars` | 控制 summary 大小 |
| `agent.kernel.compaction.compacted_summary_max_chars` | 控制 compact / recovery 后 summary 总字符预算 |
| `agent.kernel.compaction.source_recent_messages_limit` | 控制 compact 输入读取的近期 transcript 范围 |
| `agent.kernel.compaction.source_recent_tool_results_limit` | 控制 compact 输入读取的近期 tool results 范围 |
| `agent.kernel.compaction.tool_output_keep_last` | 控制 compact 时保留最近多少条 tool outputs |
| `agent.kernel.compaction.tool_output_max_chars` | 控制单条 tool output 注入 compact 输入时的最大字符数 |
| `agent.kernel.compaction.compacted_tool_output_max_chars` | 控制 compact / recovery 后 tool output region 的总字符预算 |
| `agent.kernel.compaction.source_recent_tool_exchanges_limit` | 控制 compact 输入读取的近期 tool exchanges 范围 |
| `agent.kernel.compaction.tool_exchange_keep_last` | 控制 compact 时保留最近多少条 tool exchanges |
| `agent.kernel.compaction.tool_exchange_max_chars` | 控制单条 tool exchange 注入 compact 输入时的最大字符数 |
| `agent.kernel.compaction.compacted_tool_exchange_max_chars` | 控制 compact / recovery 后 tool history region 的总字符预算 |
| `agent.kernel.compaction.retrieved_memories_keep` | 控制 retrieved memory item 数量 |
| `agent.kernel.compaction.retrieved_memory_max_chars` | 控制 retrieved memory 总长度 |
| `agent.kernel.compaction.compacted_retrieved_memory_max_chars` | 控制 compact / recovery 后 retrieved memory 总字符预算 |
| `agent.kernel.prompt.bootstrap.max_chars_per_file` | 控制 bootstrap 单文件截断 |

补充约束：

1. `RecentMessages` / `RecentToolHistory` / `RecentToolOutputs` 命中总字符预算时，必须优先保留最新内容（latest-first），不能优先保留较老内容。
2. `compacted_*_max_chars` 必须小于等于对应的常规 `*_max_chars`，避免 recovery 配额反向膨胀。

### 8.2 当前文档要求

这次重写后，对这些配置项的态度必须是：

1. 它们是当前实现的一部分，文档要如实反映。
2. 不再把“未来可能删除”写成“现在已经删除”。
3. 后续若要继续收敛配置，必须基于真实运行数据和 warning 观测，不靠拍脑袋删字段。
4. 与当前实现不再相关、仅做参数搬运、或无法形成稳定行为差异的无用配置，应在完成替代路径后及时清理，不得长期挂在 `config` 模块中制造伪开关。

### 8.3 当前首要配置问题

当前最需要优先审计的不是 history limit，而是 budget 解析与模型画像语义，尤其是：

- `reserved_output_tokens = use_model_default`
- 模型 `max_tokens > context_window`

这类配置组合会显著影响 budget 基线判断，属于一级治理问题。

---

## 9. Prompt 区域与压缩边界

### 9.1 当前与本方案对齐的区域边界

以下 prompt 区域已经是当前真实实现，后续压缩方案必须引用它们，而不是再使用旧名称：

#### Static Prefix

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

#### Dynamic Suffix

- `RequestedToolPreferenceHint`
- `PlanningInstruction`
- `RecentTaskContinuity`
- `ExpandedToolCatalog`
- `SessionSummary`
- `RecentMessages`
- `RetrievedMemories`

#### Request Context Injection

- `AppendedSystemContext`
- `PrependedUserContextMessage`

### 9.2 需要移除的旧引用

本方案后续不再允许出现以下旧引用：

- `RuntimeSessionContext`
- `ProjectContext`
- `CompactToolRouting`

原因：

1. `RuntimeSessionContext` 已被删除，其内容已拆入 `AppendedSystemContext`
2. `ProjectContext` 已被删除，bootstrap 文件已固定进入 static slots
3. `CompactToolRouting` 已并入 `ToolChoicePriority`

补充说明：

- `CompactToolRouting` 原先承担的不是“动态事实”，而是“压缩/收敛场景下的工具选择规则”
- 这类内容本质上属于稳定 policy，而不是 request-scoped dynamic context
- 因此并入 `ToolChoicePriority` 后，它的语义归属更准确，也更符合 static prefix 规则层定位

### 9.3 `ExpandedToolCatalog` 的地位

`ExpandedToolCatalog` 不是“可有可无的工具说明文本”。

在当前设计下，它负责：

- 默认仅暴露系统工具
- 通过 tool search 动态发现额外工具
- 将当前回合真实可见的工具集合注入给模型

因此：

1. 它必须保留在 `DynamicSuffix`
2. 它可以被 last-resort drop
3. 但不能从整体架构中删除

---

## 10. Implementation Gaps

以下能力尚未在当前实现中完全落地，但已经被本文明确列为后续收敛项。它们属于 gap 列表，而不是当前行为描述。

### 10.1 Gap A: Session-Level Compaction State Machine

当前未落地：

1. `Recovery` 之后的未完成任务恢复协议
2. backlog 的显式可观测事件与管理接口
3. 基于 backlog 的更强恢复策略，而不是仅做 session-local 串行队列

当前已落地前置能力：

1. builder diagnostics
2. prepare compaction
3. session reset API
4. `Normal / Compressing / Recovery / Blocked` session states
5. `Blocked` session gate
6. agent-level backlog gate / resume gate

### 10.2 Gap B: Preemptive Compaction Before Section Drop

当前未落地：

1. 将 `0.85 / 0.90 / 0.95` 进一步统一收敛为覆盖 blocked / recovery 的完整状态机
2. 将阈值治理从“prepare 阶段 preemptive compaction”继续扩展到更细粒度的 builder / load policy 联动
3. 对 `old tool output` 之外的 `assistant/tool exchange` 建立更细粒度 trimming

当前已落地行为：

1. `prepare` 已先构建保留全部 dynamic section 的 prompt
2. 超预算、达到治理阈值、或历史区被裁掉时，已先尝试 history compaction
3. 仅在 compact exhausted 后，才回退到允许 dynamic trim 的 section drop
4. compact 输入仍过长时，已先做 `old tool output` trimming，再做 `oldest history turn` trimming

因此当前实现与目标顺序之间仍存在一个确定 gap：

- 当前已实现 “compaction first, drop last”
- 仍未实现的是“更细粒度、更统一的阈值状态机”

### 10.3 Gap C: Codex-Style Context Trimming Inside Compaction

当前未落地：

1. 比“old tool output -> old assistant/tool exchange -> oldest history item”更细粒度的 compactable unit 定义与优先级
2. compact 失败后的更丰富递进式重试策略

当前已落地：

1. compaction input budget 已可限制 compact 输入规模
2. compact 输入过长时，已先裁 `OldToolOutput`，再裁 `OldAssistantToolExchange`，最后裁 `OldestHistoryItem`
3. `trimmed_context_unit_count` / `trimmed_context_unit_kind` 已进入 compaction 诊断

目标行为：

1. 当 compact 输入本身超出可接受预算时，不直接 drop prompt section
2. 先按更细粒度 compactable unit 逐级删除
3. 重新生成 compact 输入并重试
4. 只有当 compact retry exhausted 后，才允许 `DynamicSuffix` section drop

### 10.4 Gap D: Strongly Typed Compaction Output

当前已落地：

- 一版基础强类型 `SessionContinuity`
  - `recent_goal`
  - `VerifiedPathEvidence`
  - `ContinuityToolSummary`
  - `pending_tasks`
  - `ContinuityAnswerMode`
  - `last_failure`
  - `suggested_next_step`
- compact helper 的结构校验与 retry
  - `SessionMemorySummary.title` 非空
  - `MemoryPersistenceDecision.rationale` 非空
  - 校验重试耗尽后，在 prepare compaction 主路径中降级为 `RejectedMissingSummary`

旧文档里的 `VerifiedEvidence / ToolCallSummary` 一类结构，不属于当前实现。

### 10.5 Gap E: Runtime Threshold Encoding

当前未落地：

- 把 `0.85 / 0.90 / 0.95` 直接编码为统一运行时状态机控制常量

当前已落地：

1. builder 依据 `effective_prompt_budget`
2. prepare 依据“超预算或历史区已被裁掉”触发 compaction

### 10.6 Gap F: Budget Metadata Semantic Repair

当前未落地：

1. 模型画像中 `max_tokens` 语义收敛
2. 围绕 budget 解析结果的更强配置校验与观测

当前已落地：

1. `UseModelDefault` 的保守上限修复
2. `compaction_buffer_tokens` 相对 `context_window` 的防御性上限

这仍然是高优先级治理项，因为模型元数据语义不清仍会影响 budget 基线是否失真。

---

## 11. 演进约束

后续继续演进时，必须保持以下不变量：

1. `StaticPrefix` 不参与 last-resort drop
2. bootstrap files 保持固定 static slots，不得运行时漂移
3. `DynamicSuffix` 仍保持 system role
4. `AppendedSystemContext` 保持 system role
5. `PrependedUserContextMessage` 保持 user role
6. `ExpandedToolCatalog` 继续作为动态工具可见性注入区
7. 常规上下文收缩必须优先依赖 summary / compact，而不是把 `PromptDynamicSectionDropped` 当作主要治理路径
8. 达到压缩阈值后，默认先走 compact；只有 compact 无法将窗口拉回预算内时，才允许 drop dynamic sections
9. compact 失败重试时，优先裁旧 context unit，不优先裁 prompt section

### 11.1 不再接受的旧写法

后续所有 context-window 相关文档与实现说明中，不再接受以下写法：

1. 把“未来想删除的配置”写成“当前已经删除”
2. 使用已废弃区域名
3. 把 `AppendedSystemContext` 写成 user role
4. 把不存在的旧状态名或尚未实现的恢复协议写成当前既成事实

---

## 12. 推荐排障顺序

当线上再次出现如下 warning 组合时：

- `PromptBootstrapFileTruncated`
- `PromptDynamicSectionDropped`

推荐排查顺序固定为：

1. 先看 `PromptDiagnostics.window_usage`
   - `context_window`
   - `reserved_output_tokens`
   - `compaction_buffer_tokens`
   - `effective_prompt_budget`
2. 再看模型元数据是否存在：
   - `max_tokens > context_window`
3. 再看 bootstrap 文件大小与截断：
   - 是否 `AGENTS.md` 过大
   - 是否 `max_chars_per_file` 过小
4. 再看 dynamic 区实际膨胀来源：
   - `RecentMessages`
   - `SessionSummary`
   - `ExpandedToolCatalog`
   - `RetrievedMemories`
5. 最后才考虑是否需要进一步收紧 compact/load policy

不要一上来就把问题归因到：

- 权限模式
- tool role
- provider cache

这些通常不是第一根因。

---

## 13. 可观测性

当前上下文治理的可观测性能力包括：

- `PromptDiagnostics.window_usage`
- `PromptDiagnostics.dropped_sections`
- `PromptDiagnostics.bootstrap_files`
- `PromptWarningState` 去重

这部分不是压缩动作本身，而是压缩治理的观测层。

它必须继续保留，因为它是定位以下问题的唯一可靠手段：

1. 是预算基线被配置压坏
2. 还是 dynamic 区真实膨胀
3. 是 bootstrap 大文件造成的 static 压力
4. 还是 history/memory/tool catalog 造成的 dynamic 压力

严重度建议：

- `PromptDynamicSectionDropped` 通常应视为低到中等级退化信号
- 单次偶发 drop 不一定表示系统故障
- 但若同一 session 或同一模型配置持续重复出现 drop，应升级为治理问题，优先检查：
  1. `reserved_output_tokens`
  2. `context_window / max_tokens` 模型画像
  3. `prepare` 阶段的 history compaction 是否及时生效
  4. summary / recent history 配置是否过于激进

判定原则再补一条：

- 若系统已经具备 preemptive compaction，但仍在“首次超过压缩阈值”时直接发出 `PromptDynamicSectionDropped`，应优先视为治理顺序错误，而不是正常行为

---

## 14. 流程图

### 14.1 当前已实现路径

```text
user input
   │
   ▼
general_turn::prepare
   │
   ├── build PromptBuilder
   ├── PromptBuilder::assemble
   │      ├── build StaticPrefix
   │      ├── build DynamicSuffix
   │      ├── build AppendedSystemContext
   │      ├── build PrependedUserContextMessage
   │      ├── estimate PromptWindowUsage
   │      └── preserve all DynamicSuffix on first build
   │
   ├── if within budget and no history section dropped
   │      └── render PromptBlock -> ChatMessage -> continue
   │
   └── else
          ├── stage session state = Compressing
          ├── flush memory
          ├── rebuild with compacted load policy + PreserveAll
          ├── if compacted summary exists
          │      └── replace current assembly
          ├── if prompt still over budget
          │      └── rebuild with TrimToBudget fallback
          ├── if still over budget
          │      └── mark session state = Blocked and reject future turns
          └── else
                 └── record warnings/diagnostics and continue
```

### 14.2 下一阶段目标路径

```text
user input
   │
   ▼
Normal
   │
   ├── usage < 0.85
   │      └── continue
   │
   ├── 0.85 <= usage < 0.90
   │      └── pre-compress dynamic history
   │
   ├── 0.90 <= usage < 0.95
   │      ├── explicit history compaction + recovery
   │      ├── if compact input still too large
   │      │      -> trim old compactable context units
   │      │      -> retry compact
   │      └── if still over budget -> last-resort dynamic section drop
   │
   └── usage >= 0.95
          ├── try compaction first
          ├── if compact input still too large
          │      -> trim old compactable context units
          │      -> retry compact
          ├── if still over budget -> last-resort dynamic section drop
          └── if still unresolved -> session freeze / blocked / reset
```

---

## 15. 验收标准

本次文档重写后的验收标准如下：

1. 不再引用已删除的 prompt 区域。
2. 明确区分当前已实现行为与未来目标态。
3. 正确反映当前 PromptBuilder 装配链路和 message role contract。
4. 正确反映当前 builder trim、prepare compaction、warning dedupe 的真实行为。
5. 不再把“配置收敛目标”写成“当前已经删除配置”。

---

## 16. 总结

当前 Talos 的 context-window 治理已经不是“完全没有压缩”，而是一个**两层已实现、状态机未补齐**的系统：

1. builder 阶段负责 `DynamicSuffix` 预算裁剪
2. prepare 阶段负责 history compaction 与 prompt 重建
3. prompt 诊断与 warning 已具备较强可观测性

当前最需要修正的问题不是“再加更多阈值”，而是：

1. 修正被 `use_model_default + max_tokens > context_window` 放大的预算失真
2. 在 compact 路径内补上 Codex-style old context trimming，而不是默认先 drop prompt section
3. 如实治理 bootstrap 大文件与动态历史膨胀
4. 确保常规路径优先走 summary / context compaction，而不是达到阈值就直接 dynamic drop
5. 在此基础上，再补 session-level freeze / recovery 状态机

这份文档从现在开始只作为：

- **当前实现的真实治理基线**
- **下一阶段压缩状态机的演进约束**

不再混用旧区域名、旧角色语义和未落地目标态描述。
