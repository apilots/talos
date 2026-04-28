# Prompt Cache Boundary And Context Window Governance Design

> Status: historical draft. This document was written before the current `PromptBuilder`
> alignment and context-window compaction work landed.
> The current implementation baseline is defined by:
> - `spec/prompt-builder-claude-code-alignment-design.md`
> - `spec/context-window-threshold-compaction-governance-design.md`
>
> Names such as `RuntimeSessionContext`, `ProjectContext`, `CompactToolRouting`,
> and `BootstrapProjectContext` are historical references only and do not describe
> the current prompt architecture.

## 1. 背景

Talos 当前 prompt 治理仍保留 `max_static_tokens` 与 `max_dynamic_tokens` 两个独立预算上限。这套机制在早期控制 prompt 体积时有效，但随着系统 prompt、bootstrap、memory、continuity、tool catalog 逐步增强，问题已经暴露得很明确：

1. 关键行为约束会被预算裁剪掉。
2. 预算裁剪更像“局部修补”，而不是面向模型上下文窗口的完整治理。
3. 预算被拆成 static/dynamic 两个小箱子后，系统开始围绕箱子做妥协，而不是围绕 cache 能力和 compaction 策略做设计。
4. 真正重要的指标没有被提升到一等公民：
   - prompt cache read / write tokens
   - 当前请求总上下文占用
   - compaction 触发阈值
   - compaction 成本

对照 `claude-code` 本地源码，Talos 下一阶段应当切换到另一条设计路线：

- 以模型支持的上下文窗口为总预算上界
- 以稳定 system prefix 的 cache 能力为第一原则
- 以动态 suffix + memory/history/summary 的渐进装配为第二原则
- 以 compaction 为上下文压力的主治理手段
- 不再使用 `max_static_tokens` / `max_dynamic_tokens` 这种固定分仓上限

本方案的目标不是“把 prompt 变得无限大”，而是让 prompt 治理方式从“小预算裁剪”升级为“上下文窗口治理”。

---

## 2. 对齐 `claude-code` 的关键观察

### 2.1 明确的 static / dynamic boundary

`claude-code` 通过 `SYSTEM_PROMPT_DYNAMIC_BOUNDARY` 明确切开：

- boundary 之前：稳定、可缓存的 system prefix
- boundary 之后：session/user/tool 相关的动态部分

参考：
- `src/constants/prompts.ts`
- `src/utils/api.ts`
- `src/services/api/claude.ts`

关键点不是“把静态 prompt 做得很短”，而是“把稳定内容做成可缓存前缀”。

### 2.2 section 级 memoization 与 cache break 显式建模

`claude-code` 的 `systemPromptSection()` 会缓存 section 结果，直到 `/clear` 或 `/compact` 才失效；只有显式声明 `DANGEROUS_uncachedSystemPromptSection()` 的 section 才允许破坏缓存。

这说明它把“cache stability”作为 prompt 设计的一等目标，而不是靠不断缩短 prompt 来减少问题。

### 2.3 上下文治理主要依赖 compaction

`claude-code` 不依赖一个小型的 static token 上限来裁剪系统提示，而是：

1. 根据模型 context window 计算可用窗口。
2. 预留 summary / output headroom。
3. 在接近阈值时触发 auto-compact。
4. compact 后继续当前 query。

参考：
- `src/services/compact/autoCompact.ts`
- `src/query.ts`

### 2.4 显式跟踪 cache 与 compaction token 成本

`claude-code` 会直接跟踪：

- `cache_creation_input_tokens`
- `cache_read_input_tokens`
- compaction input / output / total tokens

这让 prompt 治理变成可观测系统，而不是仅靠“感觉 prompt 被裁了”。

### 2.5 工具说明的稳态作用依赖 cacheable prefix + tool-local prompt

`claude-code` 的 `Grep` / `Read` / `Bash` 行为稳定，不是因为加了关键词规则，而是因为：

- 全局 system prompt 明确优先专用工具
- 每个工具自己的 prompt 明确职责边界
- 这类提示尽可能稳定，不被频繁裁掉

---

## 3. 问题定义

当前 Talos 仍存在以下结构性问题：

1. `max_static_tokens` / `max_dynamic_tokens` 迫使组装器优先围绕“预算箱子”裁剪，而不是围绕“哪些内容应该稳定缓存、哪些内容应该动态装配”做决策。
2. static 和 dynamic 虽然在语义上区分，但在治理上仍停留在“分别做总量限制”的阶段，缺少真正的 boundary + cache scope 建模。
3. 当 static budget 不够时，关键 system section 仍可能 drop / truncate。
4. dynamic budget 只解决了“不能无限塞”，没有解决“何时应该 compact、何时应该只做后缀裁剪”的决策问题。
5. 当前配置项把 prompt 治理错误地暴露成两个数字，用户和实现都会自然围绕这两个数字做局部调参。

此外，还有 4 个在实施前必须明确的现实边界：

1. 当前代码库中并不存在可直接切换的 prompt compaction 子系统，阶段 5 实际上是新建能力，不是简单改开关。
2. `context_window` / `max_output_tokens` 当前没有从 `LlmProvider` trait 直接暴露到 prompt 组装路径。
3. 现有 `RequestedToolPreference` 和 `IdentityMission` 都混入了 per-turn 动态内容，不能直接整体放进稳定 prefix。
4. `CacheScope` 虽然存在，但需要明确它在 renderer / provider / API request 的端到端翻译链路。

---

## 4. 设计目标

### 4.1 主要目标

1. 完全移除 `max_static_tokens` 与 `max_dynamic_tokens`。
2. 总上下文预算改为以模型支持的最大 context window 为唯一上界。
3. Prompt 组装切换为：
   - cacheable static prefix
   - dynamic suffix
   - ordered trim + compaction context governance
4. 关键 system prompt 不再因为局部预算箱子被裁掉。
5. prompt/cache/compaction 成本可观测。

### 4.2 非目标

1. 不追求“永远不裁剪任何内容”。
2. 不把 prompt 变成无限长；仍然要受模型窗口约束。
3. 不引入规则驱动意图分类或硬编码路由。
4. 不改变“模型驱动”的整体路线。

---

## 5. 核心决策

### 5.1 移除双预算字段

以下配置与代码概念应被删除，而不是保留兼容残留：

- `agent.kernel.prompt.max_static_tokens`
- `agent.kernel.prompt.max_dynamic_tokens`
- `PromptBudget.max_static_tokens`
- `PromptBudget.max_dynamic_tokens`
- 所有围绕这两个字段的默认值、校验、测试和透传逻辑

原因：

- 这两个字段已经把 prompt 治理简化成了错误抽象。
- 继续保留会诱导后续实现围绕小预算做 patch，而不是转向窗口治理。
- 项目当前阶段优先最佳方案，不做历史包袱兼容。

### 5.2 总预算基于模型最大上下文窗口

Talos 的 prompt 总预算上界应当来自当前 session 使用模型的能力描述：

`context_window = model.max_context_window`

但可用 prompt 上界不等于裸窗口本身，而是：

`effective_prompt_budget = context_window - reserved_output_tokens - compaction_buffer_tokens`

其中：

- `reserved_output_tokens`
  - 用于确保模型仍有足够空间生成 answer / summary / salvage response
  - 应从 provider 能力中读取，优先使用模型 max output tokens 的保守值
- `compaction_buffer_tokens`
  - 用于提前触发 compact，而不是等到真正撞窗
  - 应为强类型配置对象中的一个治理参数，不再拆成 static/dynamic 上限

结论：

- 整体 token 上限，应以模型最大上下文窗口为根。
- 但运行时真正允许装配的 prompt 上界，应是“模型窗口减去保留区”的有效窗口。
- 这与 `claude-code` 的 `effective context window` 思路一致。

### 5.2.1 Model capability 的来源

阶段 3 落地时，不应等待 `LlmProvider` 先完成大改再继续；推荐分两步：

1. **阶段 3 的主路径**
   - 从 `talos_config` 的模型元数据读取能力值
   - 当前代码里已经有模型级 `context_window` 元数据，适合作为第一实现来源
   - `max_output_tokens` 若模型元数据已有则直接读取；若暂时没有，则通过保守策略推导

2. **后续增强路径**
   - 在 `LlmProvider` 或 provider capability 抽象里增加显式能力查询接口
   - 让运行时 capability 可以覆盖静态配置映射

本方案因此明确：

- **阶段 3 先走 config-backed capability resolution**
- **provider capability introspection 作为后续增强，而不是阶段 3 的阻塞前提**

### 5.3 引入明确的 prompt boundary

Prompt 组装应明确分为两段：

1. `StaticPrefix`
2. `DynamicSuffix`

#### StaticPrefix

特征：

- 尽量稳定
- 尽量 cacheable
- 不含每轮都变化的信息
- 只在 `/clear`、`/compact`、workspace bootstrap 变化或模型/工具集合变化时失效

包含：

- `IdentityCore`
- `EvidenceToolingPolicy`
- `ToolChoicePriority`
- `RequestedToolPreferenceTemplate`
- `FailureRecoveryPolicy`
- `CompactToolRouting`
- 稳定版 `TaskProtocolLibrary`
- 稳定版工具 catalog 前缀

#### DynamicSuffix

特征：

- 每轮可能变化
- 不要求长期 cache 稳定
- 优先承载 session / workspace / memory / continuity 内容

包含：

- `RuntimeSessionContext`
- `AGENTS.md` 内容
- `USER.md` / `SOUL.md` / `MEMORY.md`
- `RequestedToolPreferenceHint`
- continuity
- session summary
- retrieved memories
- recent messages
- expanded tool catalog
- planning instruction

### 5.3.1 `RequestedToolPreference` 的拆分

当前实现会读取当轮 `user_input` 来判断用户是否显式点名工具。这意味着它**不是稳定 prefix section**。

因此后续实现必须拆成两部分：

1. `RequestedToolPreferenceTemplate`
   - 只保留稳定的通用 policy 文案
   - 例如“当用户明确点名某个工具时，应优先尝试该工具”
   - 进入 `StaticPrefix`

2. `RequestedToolPreferenceHint`
   - 由当轮输入解析出的轻量提示
   - 例如“本轮用户显式点名了 grep_files”
   - 进入 `DynamicSuffix`

禁止把含有 per-turn user input 派生结果的完整 section 直接放进 `StaticPrefix`。

### 5.3.2 `IdentityMission` 的拆分

当前 `IdentityMission` 混入了：

- phase label
- planner mode
- session directory
- connection mode
- project_root

这些都是 per-turn 或 per-session runtime context，不适合与稳定 identity 一起缓存。

因此后续实现必须拆成：

1. `IdentityCore`
   - Talos 的长期身份、使命、基础行为风格
   - 进入 `StaticPrefix`

2. `RuntimeSessionContext`
   - phase / planner / directory / connection / project_root
   - 进入 `DynamicSuffix`

### 5.4 StaticPrefix 优先缓存，而不是优先压缩

Talos 要对齐 `claude-code` 的核心决策：

- 稳定 prefix 的主要治理手段是 cache，而不是压缩
- dynamic suffix 的主要治理手段是裁剪 + compact

因此：

- 不再为 static prefix 设置单独 token cap
- static prefix 只在“总有效窗口即将被撑爆”且 compact 尚未发生时，才允许做最后手段的结构化 trimming
- 即使 trimming，也应遵守 section 优先级，不允许随意 drop critical behavior policy

### 5.5 Context pressure 由有序裁剪与 compaction 协同处理

当 prompt 组装接近 `effective_prompt_budget` 时，处理顺序应为：

1. 先裁剪 dynamic suffix 中最低价值内容
2. 若仍超预算，或本轮已开始裁掉 history section，则触发 compact / summary / history reduction
3. compact 后重新组装 prompt，并优先让后续轮次消费 compact 结果
4. 仍超预算才进入最后手段的 structure-aware trimming

而不是像现在这样，先把 static / dynamic section 按两个预算箱子硬切。

### 5.5.1 Compaction 的实现边界

当前 Talos 代码库中并不存在完整的 prompt/session compaction 子系统，因此本方案里的阶段 5 明确视为**新子系统建设**，不是切换现有路径。

阶段 5 的实现边界定义为：

1. **触发位置**
   - 在 kernel 组装 prompt 并估算 token 后判断是否需要 compact
   - compact 属于 kernel 的上下文治理动作

2. **输入范围**
   - 主要面向 session history / recent messages / summaries
   - 不直接 compact 稳定 static prefix

3. **实现方式**
   - 由 kernel 发起一次 summary / compaction request
   - 生成结构化 compact result
   - 将历史对话压缩为 compact boundary 之后的摘要段
   - 重写后续 prompt 所读取的 session history 视图

4. **非目标**
   - 阶段 5 不要求一次性做成 `claude-code` 全量的 microcompact / snip / projection 体系
   - 先实现单一路径的 session history compaction

换句话说，阶段 5 的最小可行实现是：

- `kernel-triggered summary compaction for session history`

而不是：

- “先假设已经有 compact 机制，再把它接上”

---

## 6. 新的 prompt 组装与治理流程

### 6.1 组装流程

```text
┌──────────────────────────────────────────────────────────┐
│ Resolve model capability                                │
│ - context_window                                        │
│ - max_output_tokens                                     │
└──────────────────────────────────────────────────────────┘
                          ↓
┌──────────────────────────────────────────────────────────┐
│ Compute effective window                                │
│ effective = context_window                              │
│            - reserved_output_tokens                     │
│            - compaction_buffer_tokens                   │
└──────────────────────────────────────────────────────────┘
                          ↓
┌──────────────────────────────────────────────────────────┐
│ Build StaticPrefix                                      │
│ - cacheable system sections                             │
│ - stable tool routing + behavior policy                 │
└──────────────────────────────────────────────────────────┘
                          ↓
┌──────────────────────────────────────────────────────────┐
│ Build DynamicSuffix                                     │
│ - bootstrap files                                       │
│ - continuity                                            │
│ - summaries / memories / recent messages                │
│ - expanded tool catalog                                 │
└──────────────────────────────────────────────────────────┘
                          ↓
┌──────────────────────────────────────────────────────────┐
│ Estimate total tokens                                   │
└──────────────────────────────────────────────────────────┘
                          ↓
       within budget? ─────────────── yes ─────────────→ send request
                          │
                          no
                          ↓
┌──────────────────────────────────────────────────────────┐
│ Trim low-value DynamicSuffix sections                   │
│ - expanded tool catalog                                 │
│ - retrieved memories                                    │
│ - recent messages                                       │
│ - session summary                                       │
│ - requested tool preference hint                        │
│ - recent task continuity                                │
│ - bootstrap project context                             │
└──────────────────────────────────────────────────────────┘
                          ↓
   within budget and no history trimmed? ─ yes ───────→ send request
                          │
                          no
                          ↓
┌──────────────────────────────────────────────────────────┐
│ Trigger compaction / history reduction                  │
│ Rebuild prompt                                          │
└──────────────────────────────────────────────────────────┘
                          ↓
       within budget? ─────────────── yes ─────────────→ send request
                          │
                          no
                          ↓
┌──────────────────────────────────────────────────────────┐
│ Last-resort structure-aware trimming                    │
│ (never arbitrary drop of critical policy)               │
└──────────────────────────────────────────────────────────┘
```

### 6.2 Section 优先级规则

#### StaticPrefix

- `CriticalStatic`
  - 永不因局部预算箱子被裁
  - 仅在总窗口危机且 compact 后仍超限时才允许最小 trimming
- `ImportantStatic`
  - 可做少量 tail trim
- `SupplementalStatic`
  - 尽量保留，但优先级低于 dynamic high-value continuity

#### DynamicSuffix

按裁剪优先顺序从低到高：

1. `ExpandedToolCatalog`
2. `RetrievedMemories`
3. `RecentToolOutputs`
4. `RecentToolHistory`
5. `RecentMessages`
6. `SessionSummary`
7. `RequestedToolPreferenceHint`
8. `RecentTaskContinuity`

说明：

- 这里的 pre-compaction trim 允许在预算危机时先 drop 历史 section，避免单轮请求直接卡死在 over-budget 状态
- 一旦 `RecentToolOutputs` / `RecentToolHistory` / `RecentMessages` / `SessionSummary` 被裁掉，kernel 应继续触发 compaction，将这次 history reduction 固化为后续轮次可复用的 summary
- continuity 和 bootstrap 项目规则比详细 tool catalog 更重要，因此在 history 之后再被裁

---

## 7. 配置与强类型设计

### 7.1 移除旧配置

从配置中删除：

- `PromptConfig.max_static_tokens`
- `PromptConfig.max_dynamic_tokens`

同时删除所有重复预算结构中的同名字段，包括：

- `PromptBuildConfig.budget.max_static_tokens`
- `PromptBuildConfig.budget.max_dynamic_tokens`
- `AgentKernelPromptConfig.max_static_tokens`
- `AgentKernelPromptConfig.max_dynamic_tokens`

删除后，`prompt` 域只保留与下列内容有关的治理参数：

- compaction buffer
- output reserve strategy
- bootstrap include 开关

历史/summary/retrieved memories 的保留策略不再继续挂在 `prompt` 域，统一进入 `agent.kernel.compaction`。

### 7.2 新强类型对象

建议新增：

```rust
pub struct PromptWindowPolicy {
    pub reserved_output_tokens: ReservedOutputPolicy,
    pub compaction_buffer_tokens: usize,
}

pub enum ReservedOutputPolicy {
    UseModelDefault,
    Fixed(usize),
    Ratio { numerator: u32, denominator: u32 },
}

pub struct PromptWindowUsage {
    pub context_window: usize,
    pub effective_prompt_budget: usize,
    pub static_prefix_tokens: usize,
    pub dynamic_suffix_tokens: usize,
    pub cache_read_input_tokens: Option<u32>,
    pub cache_creation_input_tokens: Option<u32>,
}
```

要求：

- 不允许退回松散 metadata json
- 不允许把这些参数在调用链上拆成多个离散标量长期透传
- 优先透传 `PromptWindowPolicy` 或其父配置对象

---

## 8. Cache 设计

### 8.1 Talos 的 cache scope 应显式化

现有 `CacheScope` 已有：

- `Static`
- `Workspace`
- `None`

建议将其真正用于 API 构建层，而不是仅停留在 section 标注：

- `StaticPrefix` → cacheable prefix
- `Workspace` → 对 workspace 级稳定内容可考虑中期缓存
- `None` → 每轮重算

并明确要求形成完整链路：

`PromptSection.cache_scope -> renderer cache_control -> provider request block -> usage metrics`

如果只停留在 section 标注层，而没有进入最终 API 请求，则本方案中的 cache boundary 设计视为未完成。

### 8.2 Section cache 生命周期

对齐 `claude-code`：

以下事件需要清理或重建 section cache：

- `/clear`
- compact 完成
- workspace bootstrap 文件变化
- tool registry 变化
- model 切换
- session continuity schema 版本变化

### 8.3 Tooling 不再整体视作一个可随意裁剪的大块

工具信息应拆成：

1. `CompactToolRouting`
   - cacheable
   - 稳定
   - 极短
2. `ExpandedToolCatalog`
   - dynamic
   - 可裁
   - 可 names-only / summary-only / full-desc 三档退化

---

## 9. Compaction 策略

### 9.1 触发阈值

`should_compact` 应根据：

- 当前 assembled prompt tokens
- 近期消息 tokens
- 预计输出保留
- 模型上下文窗口

来判断，而不是等到 provider 明确报 prompt too long。

### 9.2 先 compact 再重组

在高上下文压力下，应优先：

1. compact memory / history
2. 重新组装 prompt
3. 再看是否还需要 trimming

而不是“先把 system prompt 裁碎”。

### 9.3 compact 结果需要显式进入指标体系

每次 compact 应记录：

- pre-compact tokens
- post-compact tokens
- compact input tokens
- compact output tokens
- compact cache read tokens
- compact cache creation tokens

这与 `claude-code` 的事件记录方式保持一致。

---

## 10. Metrics 与可观测性

Talos 需要新增或强化以下指标：

- `prompt_context_window_tokens`
- `prompt_effective_budget_tokens`
- `prompt_static_prefix_tokens`
- `prompt_dynamic_suffix_tokens`
- `prompt_total_tokens`
- `prompt_cache_read_input_tokens`
- `prompt_cache_creation_input_tokens`
- `prompt_compaction_count`
- `prompt_compaction_input_tokens`
- `prompt_compaction_output_tokens`
- `prompt_sections_trimmed`
- `prompt_sections_dropped`
- `prompt_rebuild_after_compaction_count`

目的：

- 让 prompt 治理从“调两个数字”变成“看一套真实指标”
- 让后续优化能量化比较，而不是只看主观 case

---

## 11. 模块影响面

### 11.1 `talos_config`

需要：

- 删除 `max_static_tokens`
- 删除 `max_dynamic_tokens`
- 新增 `PromptWindowPolicy` 对应配置
- 新增 `AgentKernelCompactionConfig`，统一承载：
  - `model`
  - `recent_messages_keep`
  - `recent_messages_max_chars`
  - `compacted_recent_messages_keep`
  - `compacted_recent_messages_max_chars`
  - `recent_summaries_keep`
  - `summary_max_chars`
  - `compacted_summary_max_chars`
  - `source_recent_messages_limit`
  - `source_recent_tool_results_limit`
  - `tool_output_keep_last`
  - `tool_output_max_chars`
  - `compacted_tool_output_max_chars`
  - `source_recent_tool_exchanges_limit`
  - `tool_exchange_keep_last`
  - `tool_exchange_max_chars`
  - `compacted_tool_exchange_max_chars`
  - `retrieved_memories_keep`
  - `retrieved_memory_max_chars`
  - `compacted_retrieved_memory_max_chars`
- 更新默认配置、loader、validator、tests

最近动态区 (`RecentMessages` / `RecentToolHistory` / `RecentToolOutputs`) 命中总字符预算时，装配顺序应采用 `latest-first` 保留策略，优先保留最新上下文，而不是优先保留旧内容。

### 11.2 `talos_prompt`

需要：

- 重构 `PromptBudget` 为窗口治理模型，或直接删除并替换为 `PromptWindowPolicy`
- 引入 `StaticPrefix` / `DynamicSuffix` 显式边界
- 将 `IdentityMission` 拆分为 `IdentityCore` + `RuntimeSessionContext`
- 将 `RequestedToolPreference` 拆分为 `RequestedToolPreferenceTemplate` + `RequestedToolPreferenceHint`
- 调整 section trimming 顺序
- 强化 cache scope 输出

### 11.3 `agent-kernel`

需要：

- 在 prepare 阶段拿到当前模型 context capability
- 参与 ordered-trim-and-compaction 决策
- 输出新的 prompt governance 指标与 warning

### 11.4 provider / gateway 层

需要：

- 支持构建 cacheable prefix blocks
- 新增基于 `CacheScope` 的 `cache_control` 注入逻辑
- 支持上报 cache read / write token usage
- 明确 system prompt block 的 cache scope 行为

---

## 12. 迁移策略

### 12.1 迁移原则

- 不保留 `max_static_tokens` / `max_dynamic_tokens` 兼容字段
- 不做 fallback 兼容旧配置
- 配置与代码同步迁移

### 12.2 迁移步骤

1. 删除配置字段与默认值
2. 删除 `PromptBudget` 的双预算结构
3. 落地 `PromptWindowPolicy`
4. 落地 `StaticPrefix / DynamicSuffix` 组装边界
5. 接入 context window capability
6. 接入 ordered trim + compaction 路径
7. 接入 metrics

---

## 13. 验收标准

### 13.1 结构验收

1. 代码库中不再存在：
   - `max_static_tokens`
   - `max_dynamic_tokens`
2. Prompt 组装流程中存在显式的 `StaticPrefix` / `DynamicSuffix` 边界。
3. 关键 static policy section 不再因局部预算被先裁掉。

### 13.2 行为验收

1. 真实 SSE case 下，`RequestedToolPreference`、`CompactToolRouting` 不再因为局部静态预算缺失而消失。
2. 在长 session 下，context pressure 先由 ordered dynamic trimming 吸收尖峰，再由 compaction 固化 history reduction，避免每轮重复 over-budget。
3. 显式要求 `grep_files` 的代码分析 case 中，首轮服从率稳定，不依赖规则代码。

### 13.3 指标验收

1. 日志或 telemetry 中可看到：
   - 总上下文窗口
   - 有效预算
   - prefix/suffix token 占用
   - cache read / creation tokens
   - compaction token 成本
2. 不再只能通过 `PromptDynamicSectionDropped` 这种粗粒度 warning 猜问题。

---

## 14. 实施建议顺序

1. 阶段 2：删除旧预算字段与重复结构
2. 阶段 3：引入模型窗口驱动的 `effective_prompt_budget`
3. 阶段 4：落地 `StaticPrefix / DynamicSuffix` 与 `CacheScope` 端到端接入
4. 阶段 5：定义并实现第一版 session history compaction 子系统边界
5. 阶段 6：切换到 ordered trim + compaction 的上下文压力治理
6. 阶段 7：补齐 cache / compaction / context 指标
7. 阶段 8：通过真实 HTTP SSE case 与全量 e2e 做回归验证

---

## 15. 一句话结论

Talos 下一阶段不应继续围绕 `max_static_tokens` / `max_dynamic_tokens` 这种小预算做调参，而应对齐 `claude-code`：

- 用模型最大上下文窗口作为根预算
- 用稳定前缀 cache 保护 system prompt
- 用 dynamic suffix 承载每轮变化内容
- 用 compaction 作为上下文压力的主治理手段
- 用 cache / compaction / context 指标替代“section 被裁了多少”这类局部观察
