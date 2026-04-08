# Talos Prompt & Tools 改进方案（修订版）

> 基于对 Claude Code CLI 源码的深度调研，对 Talos 现有 prompt 和 tools 设计进行差距分析和改进设计。
> 本文档已根据评审意见修订，纠正了对现状的错误描述，并收敛了 4 个主要问题点。

---

## 0. Prompt 分层语义模型

> 本节是业务方向性约束，先于实施方案定义，指导后续所有 Prompt 相关的设计决策。

### 0.1 为什么 Static / Dynamic 二分是必要的，但不够用

"Static / Dynamic"是 Talos Prompt 的**顶层语义分层**，含义是：

- **Static Context**：内容不依赖当前这一轮 turn 的即时输入，在一个较长时间窗口内相对稳定。
- **Dynamic Context**：直接依赖当前 turn、当前 session 或当前检索结果，每次请求都可能变化。

这个二分**在语义层面是正确的**，因为它回答了"这段内容是否随每次请求变化"这个核心问题，是做任何进一步决策的前提。

但它**不够用**，原因有两个：

**原因一：Static 内部的"稳定范围"并不一致。**

System 层（角色定义、阶段说明）只由代码常量决定，可以跨所有 workspace、所有用户稳定复用。  
Tooling 层（工具列表）由运行时注册状态决定，工具集变化时内容就变化。  
Workspace 层（AGENTS.md、SOUL.md 等 bootstrap 文件）由当前工作区决定，换一个仓库就是另一套内容。

把这三种内容统一标记为"static"，如果后续直接用"是否 static"来决定是否缓存，就会缓存错误边界——比如把 Workspace 内容当作跨工作区可复用的全局缓存。

**原因二：三个独立决策维度被混用。**

"Static / Dynamic"只是分层语义，它不直接回答以下三个独立问题：

| 维度 | 它回答的问题 | 当前 Talos 对应概念 |
|------|------------|-------------------|
| **顶层分层** | 这段内容在多次请求间是否稳定？ | `system_sections` vs `context_sections` |
| **预算语义** | token 预算紧张时，这段内容的截断优先级？ | `BudgetTier`（Critical / Important / Supplemental） |
| **复用语义** | 如果要缓存，这段内容在什么范围内可以复用？ | 当前无显式表达（即将引入 `CacheScope`） |

这三个维度彼此独立。一段内容可以同时是：Static + Critical（高预算优先级）+ Workspace-scoped（只在同一工作区复用）。把任意两个维度混用都会导致设计或实现出现逻辑漏洞。

---

### 0.2 推荐的 Prompt 分层模型

#### 顶层：Static vs Dynamic（语义分层）

```
Prompt
├── Static Context      内容稳定，不依赖当前 turn 的即时输入
│   ├── System          角色定义、执行阶段说明、核心行为约束
│   ├── Planner         规划器模式指导（BUILT_IN / PLAN_REACT instruction）
│   ├── Tooling         可用工具列表及描述
│   └── Workspace       工作区 bootstrap 文件（AGENTS.md, SOUL.md, TOOLS.md 等）
└── Dynamic Context     内容随请求变化，依赖当前 turn / session / 检索结果
    ├── Session         会话历史（SessionSummary、RecentMessages）
    ├── Memory          检索到的长期记忆（RetrievedMemories）
    └── Turn Input      当前 user input 及本轮即时拼装的上下文片段
```

#### 各子类的定义与示例

**Static > System**
- 定义：由代码常量决定，与工作区、工具集、用户无关
- 示例：`You are {agent_name}, {agent_description}`；`Phase: execution`；行为约束文本
- 当前对应：`PromptSectionKind::Identity` 的角色/阶段部分

**Static > Planner**
- 定义：由 planner 模式决定，同一模式下内容稳定
- 示例：`BUILT_IN_TOOL_GUIDANCE`、`PLAN_REACT_INSTRUCTION`
- 当前对应：`PromptSectionKind::PlanningInstruction`

**Static > Tooling**
- 定义：由运行时工具注册状态决定，工具集不变时内容稳定
- 示例：`Available tools:` 列表及每个工具的 description + schema
- 当前对应：`PromptSectionKind::Identity` 内的 `Available tools:` 段（与 System 混合，尚未拆分）

**Static > Workspace**
- 定义：由当前工作区目录的文件内容决定，同一 workspace 内稳定
- 示例：`AGENTS.md`、`SOUL.md`、`TOOLS.md`、`IDENTITY.md`、`USER.md` 等 bootstrap 文件
- 当前对应：`PromptSectionKind::ProjectContext`

**Dynamic > Session**
- 定义：来自当前 session 的历史消息和摘要，每次请求可能追加
- 示例：`SessionSummary`、`RecentMessages`（最近 N 条对话）
- 当前对应：`PromptSectionKind::SessionSummary`、`PromptSectionKind::RecentMessages`

**Dynamic > Memory**
- 定义：通过语义检索得到的长期记忆片段，每次请求按相关性变化
- 示例：`RetrievedMemories`（memory_search 结果）
- 当前对应：`PromptSectionKind::RetrievedMemories`

**Dynamic > Turn Input**
- 定义：当前 turn 的用户输入及即时上下文，每次请求完全不同
- 示例：`user_input`、本轮即时注入的上下文片段
- 当前对应：`PromptAssembly::user_input`

---

### 0.3 三个独立维度的解耦表达

这三个维度独立赋值，不互相推导：

**维度 A：顶层分层（Topological Layer）**

```
Static > System
Static > Planner
Static > Tooling
Static > Workspace
Dynamic > Session
Dynamic > Memory
Dynamic > Turn Input
```

回答："这段内容的生命周期是什么？"

**维度 B：预算语义（Budget Priority）**

```
Critical      → 必须保留，超出预算时最后截断
Important     → 尽量保留，预算紧张时可截断
Supplemental  → 按需保留，优先丢弃
```

回答："token 预算不足时，这段内容的取舍优先级是什么？"  
与分层无直接关系：一段 Dynamic > Session 的内容可以是 Important 的；一段 Static > Workspace 的内容可以是 Supplemental 的。

**维度 C：复用语义（Reuse Scope）**

```
Static    → 内容只由系统常量决定，可跨所有 workspace / 用户复用
Workspace → 内容由当前工作区决定，同一 workspace 内可复用
None      → 每次请求动态生成，不可复用
```

回答："如果要做 Prompt Caching，这段内容在什么边界内可以命中缓存？"  
不能直接由分层推导：Static > Tooling 的复用范围取决于工具集是否变化；Static > Workspace 是 Workspace-scoped，不是 Static-scoped（跨工作区不能命中）。

---

### 0.4 现有 `PromptSectionKind` 的映射关系（现状分析）

| 现有 Kind | 顶层分层 | 复用语义（应为） | 预算（现有） |
|----------|---------|--------------|------------|
| `Identity`（整体） | **混合**：System + Tooling 未拆分 | **混合**：静态角色文本 `Static`；工具列表独立复用语义，默认 `None`（工具集受注册状态、配置、连接器、会话能力影响，不由 workspace 决定），仅在工具集稳定条件明确时才可提升 | Critical |
| `PlanningInstruction` | Static > Planner | Static（同模式不变） | Important |
| `ProjectContext` | Static > Workspace | Workspace（同一工作区内） | Critical |
| `SessionSummary` | Dynamic > Session | None | Important |
| `RecentMessages` | Dynamic > Session | None | Supplemental |
| `RetrievedMemories` | Dynamic > Memory | None | Supplemental |

**关键发现**：`Identity` 目前同时承载 System 和 Tooling 两类内容（见 `assembler.rs:build_identity_prompt`），两者的复用语义不同，是后续引入 `CacheScope` 时需要处理的主要边界问题。

---

### 0.5 对后续设计的约束

1. **`PromptSectionKind` 应逐步反映 0.2 节的分层模型**，至少保证 Static 内的关键边界（System vs Tooling）可被独立表达；Dynamic 子类本轮不需要细化，待多 agent 或 streaming 场景有明确需求时再扩展。
2. **顶层结构必须继续保留并复用现有 `system_sections`（Static）/ `context_sections`（Dynamic）作为主分层骨架**，新增子类只在各自层内细化表达，不得重建主模型、不得改变现有预算截断与装配边界、不得影响 general_turn 的组装路径。
3. **`CacheScope`（复用语义）必须独立于 `BudgetTier`（预算语义）**，不能合并或互相推导。
4. **`Identity` section 的拆分是 Caching 落地的前置依赖**：在 System 和 Tooling 未分离之前，无法对 Identity 安全地设置缓存边界。
5. **`Static > Tooling` 的复用语义独立处理，默认 `CacheScope::None`**：工具集受注册状态、配置、连接器和会话能力影响，不由 workspace 决定；仅在工具集稳定条件被明确验证时，才允许将其提升为可缓存。
6. **不得在 general_turn 主路径之外引入新的执行分支**，所有分层只影响 prompt 组装，不影响调用路径。
7. **动态层内部的子类（Session / Memory / Turn Input）在本轮不需要细化 Kind**，维持现有 variant 即可；在后续引入多 agent 或 streaming 场景时再扩展。

---

## 1. 背景与动机

Talos 当前 prompt 和 tools 系统已具备基本骨架，关键工作流引导（`BUILT_IN_TOOL_GUIDANCE`、`PLAN_REACT_INSTRUCTION`）和部分工具描述均有一定质量。本文档聚焦于**仍存在的真实差距**，不夸大现有问题。

核心改进目标：
1. **工具参数 Schema 缺字段级描述** — 影响参数填写准确率，改动成本低
2. **部分工具描述（edit/write/exec_command）质量偏低** — 缺 Usage 和关键约束
3. **planner 指导缺失工具优先级、并行调用和安全操作引导** — Claude Code 中这几块是减少误操作的关键
4. **Prompt Caching 未落地** — 架构影响面较大，需全链路改造

---

## 2. 现状基线（修正版）

以下是各模块的**实际状态**，纠正上个版本对现状的误描述。

### 2.1 工具描述现状

| 工具 | 现有描述质量 | 主要缺失 |
|------|------------|---------|
| `read_file` | **中等**：有工作流引导（"Prefer this after list_dir or grep_files"） | 参数无描述；缺少"读前必 read"约束 |
| `list_dir` | **中等**：有使用时机说明（"Use this first when exploring"） | 参数无描述 |
| `grep_files` | **中等**：说明了适用场景（entrypoints, symbols, routes） | 参数无描述，缺示例 pattern |
| `edit` | **弱**："Edits a file by finding and replacing text." | 无 Usage；无"先 read 再 edit"要求；无唯一匹配约束 |
| `write` | **弱**："Writes content to a file." | 无 Usage；无覆盖警告；无"先 read 再 write"要求 |
| `exec_command` | **弱**："Run a shell command and optionally keep the session open." | 参数多且无描述；缺专用工具优先提示 |
| `memory_search` / `memory_get` | 未核查，按同类估计较弱 | — |
| `web_search` / `web_fetch` | 未核查 | — |

**关键共性问题**：所有工具的 `parameters_schema` 中字段均无 `"description"` 属性。

### 2.2 Planner 指导现状

`BUILT_IN_TOOL_GUIDANCE` 和 `PLAN_REACT_INSTRUCTION`（`planner.rs:44-64`）已覆盖：
- ✅ 本地路径验证流程（list_dir / read_file 优先）
- ✅ 代码库分析步骤（list_dir → grep → read）
- ✅ 工具结果事实 vs 推断的分离

**尚未覆盖**：
- ❌ 专用工具优先于 exec_command（核心痛点）
- ❌ 并行工具调用指导
- ❌ 破坏性操作确认（git force-push, file deletion 等）
- ❌ Prompt injection 安全警告

### 2.3 Prompt Caching 现状

- `PromptSection` 有 `BudgetTier`（Critical/Important/Supplemental），用于 token 预算截断
- `renderer.rs` 将 section 平铺为普通 `ChatMessage`（`role` + `content`）
- `ChatMessage`（`provider/src/lib.rs:260`）无 `cache_control` 字段
- Anthropic adapter 发出的请求未携带任何 `cache_control` 指令

---

## 3. 改进方案

### 3.1 工具参数 Schema 字段级描述（优先级：P0）

**目标**：为**本轮核心本地工具**（`read_file` / `list_dir` / `grep_files` / `edit` / `write` / `exec_command`）的 `parameters_schema` 中每个字段添加 `"description"` 属性。

**覆盖范围说明**：本轮仅覆盖上述 6 个核心工具。`memory_search` / `memory_get` / `web_search` / `web_fetch` / `http_request` / 协作工具等，在后续迭代中按同一标准补齐，不列入本轮实施范围。

字段描述帮助模型正确填写参数，尤其是：
- 路径类字段：明确"必须绝对路径"
- 可选参数：明确何时传、何时不传
- 枚举参数：说明各选项的含义

**改动示例（`read_file`）：**

```rust
fn parameters_schema(&self) -> Option<Value> {
    Some(serde_json::json!({
        "type": "object",
        "required": ["file_path"],
        "properties": {
            "file_path": {
                "type": "string",
                "description": "Absolute path to the file to read. Must start with /. Relative paths are not accepted."
            },
            "offset": {
                "type": "integer",
                "minimum": 1,
                "description": "Line number to start reading from (1-based). Omit to read from the beginning."
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "description": "Maximum number of lines to return. Omit to use the default limit."
            }
        }
    }))
}
```

**需要补全描述的工具参数清单：**

| 工具 | 需补充描述的字段 |
|------|---------------|
| `read_file` | `file_path`（绝对路径），`offset`（从第几行），`limit`（最多几行） |
| `list_dir` | `dir_path`（绝对路径），`depth`（展开层数），`offset/limit`（分页） |
| `grep_files` | `pattern`（正则），`include`（文件 glob 过滤），`path`（搜索范围），`limit`（结果数） |
| `edit` | `path`（绝对路径），`find`（建议使用足够长的唯一上下文以避免误改多处），`replace`（替换内容） |
| `write` | `path`（绝对路径，会覆盖），`content`（完整文件内容） |
| `exec_command` | `cmd`（命令字符串），`workdir`（工作目录），`sandbox_permissions`（各枚举值说明），`justification`（需要升权时的理由） |

### 3.2 工具描述文本增强——弱描述工具（优先级：P0）

**原则**：保留 `read_file` / `list_dir` / `grep_files` 现有描述风格，对 `edit` / `write` / `exec_command` 按照统一格式补强。

**统一格式：**
```
{一句话能力摘要，说明工具做什么}

Usage:
- {参数约束，如路径格式、字段要求}
- {使用前提，如需先 read}
- {输出格式说明}
- {限制或边界情况}

IMPORTANT: {关键约束，若有}
```

**`edit` 工具改进后：**

> **行为说明**：当前实现对所有匹配项全部替换（`str::replace` 语义）。本轮仅做 prompt 文案增强，不收紧为唯一匹配约束。描述中的引导策略是"建议使用足够长的唯一上下文"，而非"必须唯一"。若未来需要将行为收紧为唯一匹配，应单列为行为变更项并配套测试。

```
Edit a file by finding and replacing text. All occurrences of the find string will be replaced.

Usage:
- path must be an absolute path to an existing file
- find must match the target text exactly, including whitespace and indentation
- If the find string appears multiple times, all occurrences will be replaced; use a longer, unique context string (e.g., include surrounding lines) to target a specific location
- To verify current content before editing, use read_file first

IMPORTANT: Always read the file with read_file before editing. Do not edit a file you have not read in the current session.
```

**`write` 工具改进后：**
```
Write content to a file, creating it if it does not exist or overwriting it entirely if it does.

Usage:
- path must be an absolute path
- content replaces the entire file — partial updates should use edit instead
- To preserve existing content, read the file first and incorporate the existing content into the new write

IMPORTANT: This tool overwrites the entire file. Use edit for targeted changes to existing files.
```

**`exec_command` 工具改进后：**
```
Run a shell command in the workspace. Use only when no dedicated file or search tool can accomplish the task.

Usage:
- Prefer read_file over exec with cat/head/tail
- Prefer list_dir over exec with ls/find
- Prefer grep_files over exec with grep/rg
- Prefer edit or write over exec with sed/awk
- workdir defaults to the session workspace root if not specified
- For commands that require elevated permissions, set sandbox_permissions to "require_escalated" and provide a justification

IMPORTANT: Do not use exec_command as a substitute for dedicated tools. Reserve it for operations that no other tool supports.
```

### 3.3 Planner 指导补强（优先级：P1）

**目标**：在 `BUILT_IN_TOOL_GUIDANCE` 和 `PLAN_REACT_INSTRUCTION` 各自增补三个缺失的引导块。

**注意**：这两段指导通过 `PlanningInstruction` section 注入（不是 identity prompt），需避免与 `build_identity_prompt` 中的 `phase_instruction` 重复。

**Block A：专用工具优先（Tool Priority）**

追加到 `BUILT_IN_TOOL_GUIDANCE` 和 `PLAN_REACT_INSTRUCTION` 末尾：

```
Do not use exec_command when a dedicated tool exists for the operation:
- Reading files: use read_file, not exec_command with cat or head
- Listing directories: use list_dir, not exec_command with ls or find
- Searching content: use grep_files, not exec_command with grep or rg
- Editing files: use edit, not exec_command with sed or awk
- Writing files: use write, not exec_command with echo or heredoc
Use exec_command only for operations that no dedicated tool supports.
```

**Block B：并行工具调用（Parallel Execution）**

```
When multiple tool calls are needed and their inputs do not depend on each other,
call them in parallel in a single response to reduce round-trips.
When a later call depends on the output of an earlier one, call them sequentially.
```

**Block C：破坏性操作确认（Destructive Action Safety）**

```
Before taking actions that are hard to reverse, confirm with the user:
- Deleting files or directories
- Force-pushing or hard-resetting git history
- Running commands that affect external services or shared state
Never use --no-verify, --force, or similar bypass flags unless the user explicitly requests it.
```

**放置位置**：`planner.rs` 中 `BUILT_IN_TOOL_GUIDANCE` 和 `PLAN_REACT_INSTRUCTION` 两个常量末尾各自追加，不新增 section 类型，避免与 identity 内容重叠。

### 3.4 Prompt Caching 架构改造（优先级：P2，独立架构项）

> 前置约束：本节的实施依赖 0 节分层模型中明确的**复用语义（CacheScope）**。CacheScope 是维度 C，与 BudgetTier（维度 B）独立，不能混用。

#### 3.4.0 CacheScope 是跨 Provider 的统一语义，不是 Anthropic 专属能力

`CacheScope` 表达的是 Talos Prompt 层的**复用语义**，回答"这段内容在什么边界内可以复用"。它是 Talos 内部的统一抽象，不等价于任一厂商 API 的具体字段。

是否能将 `CacheScope` 真正映射为请求级缓存控制，取决于具体 provider 的能力：

- **支持原生 prompt cache 的 provider**：可将 `CacheScope` 映射为厂商请求协议中的缓存控制字段。
- **不支持原生 prompt cache 的 provider**：必须安全忽略 `CacheScope`，不得报错、不得伪造缓存命中结果。
- **未来可能支持但当前未接入的 provider**：应在能力声明中显式标记为未支持，待协议确认后再单独实现。

因此：

1. `CacheScope` 是 Talos Prompt 层的统一语义，Prompt 层无需感知 provider 差异。
2. Provider 是否支持原生缓存，必须通过**显式 capability 声明**，不能隐含假设。
3. Anthropic 是首个落地实现，不代表所有 provider 都支持缓存。
4. `cache_creation_input_tokens` / `cache_read_input_tokens` 仅由支持该能力的 provider 填充；不支持时保持 `None`。

**Provider capability 声明设计（`talos-llm-provider`）：**

```rust
/// Declares whether a provider supports native prompt caching at the request level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptCacheSupport {
    /// Provider does not support prompt caching. CacheScope must be silently ignored.
    Unsupported,
    /// Provider supports request-level prompt caching (e.g., Anthropic cache_control).
    RequestLevel,
}
```

在 `LlmProvider` trait 增加默认方法（默认返回 `Unsupported`，各 provider 按需 override）：

```rust
fn prompt_cache_support(&self) -> PromptCacheSupport {
    PromptCacheSupport::Unsupported
}
```

各 provider 声明：

| Provider | `prompt_cache_support()` |
|----------|------------------------|
| `AnthropicProvider` | `RequestLevel` |
| `OpenAIProvider` | `Unsupported`（保持默认） |
| `OllamaProvider` | `Unsupported`（保持默认） |
| `BailianProvider` | `Unsupported`（官方协议未确认前） |

**Bailian 处理原则**：在 Bailian 官方协议确认支持 prompt cache 之前，显式视为 `Unsupported`。Prompt 层仍可保留 `CacheScope` 标注；Bailian adapter 不消费 `cache_control` 字段，不填写缓存命中统计字段。如后续确认支持，单开一个小阶段接入，不影响其他 provider。

**usage 字段扩展（`provider/src/lib.rs`）**：

```rust
pub struct Usage {
    // 现有字段不变...
    /// Input tokens used to create (write) a new prompt cache entry. None if provider does not support caching.
    pub cache_creation_input_tokens: Option<u32>,
    /// Input tokens served from an existing prompt cache hit. None if provider does not support caching.
    pub cache_read_input_tokens: Option<u32>,
}
```

#### 3.4.1 CacheScope 的语义来源

`CacheScope` 对应 0.3 节的**复用语义维度**，回答"在什么范围内可以命中缓存"：

| CacheScope | 含义（复用语义） | 对应分层子类 |
|-----------|--------------|------------|
| `Static` | 只由系统常量决定，可跨所有 workspace / 用户复用 | Static > System、Static > Planner |
| `Workspace` | 由当前工作区文件内容决定，同一 workspace 内可复用 | Static > Workspace |
| `None` | 每次请求动态生成，不可复用 | Static > Tooling（工具集可变时）、所有 Dynamic 子类 |

**关键约束（来自 0.4 节发现）**：当前 `Identity` section 混合了 Static > System（角色文本）和 Static > Tooling（工具列表）。工具列表随注册状态变化，不能整体标记为 `CacheScope::Static`。这意味着：

- **Caching P2 的实施有一个前置依赖**：`Identity` section 需要拆分为独立的 System section 和 Tooling section，才能对两者分别设置正确的 CacheScope。
- 在拆分完成之前，只能对 `ProjectContext`（Static > Workspace）安全地设置 `CacheScope::Workspace`。

#### 3.4.2 完整改造链路

```
PromptSection { cache_scope: CacheScope }
    ↓ renderer.rs
ChatMessage { cache_control: Option<CacheControl> }   ← 需新增字段（Talos 统一语义层）
    ↓ provider request model
ChatRequest { messages: Vec<ChatMessage> }             ← 已有，自动携带
    ↓ provider.prompt_cache_support()                  ← 能力检查（新增）
        RequestLevel → 消费 cache_control，序列化到厂商请求
        Unsupported  → 忽略 cache_control，不传递给厂商 API
    ↓ anthropic adapter（首个落地实现）
AnthropicRequest: system[].cache_control / messages[].cache_control  ← 需改 adapter
```

**Step 1：`PromptSection` 增加 `cache_scope` 字段（`types.rs`）**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheScope {
    /// Content is determined by system constants only; safe to reuse across all requests.
    Static,
    /// Content is determined by workspace state; safe to reuse within the same workspace.
    Workspace,
    /// Content is dynamic per request; must not be cached.
    None,
}

pub struct PromptSection {
    // 现有字段不变
    pub cache_scope: CacheScope,  // 新增，默认 None
}
```

**Step 2：`ChatMessage` 增加 `cache_control` 字段（`provider/src/lib.rs`）**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CacheControl {
    #[serde(rename = "type")]
    pub kind: CacheControlKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CacheControlKind {
    #[serde(rename = "ephemeral")]
    Ephemeral,
}

pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    // ... 现有字段 ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,  // 新增
}
```

**Step 3：`renderer.rs` 将 `cache_scope` 映射到 `cache_control`**

```rust
fn cache_control_from_scope(scope: &CacheScope) -> Option<CacheControl> {
    match scope {
        CacheScope::Static | CacheScope::Workspace => {
            Some(CacheControl { kind: CacheControlKind::Ephemeral })
        }
        CacheScope::None => None,
    }
}
```

**Step 4：Anthropic adapter 序列化 `cache_control`（`anthropic/src/lib.rs`）**

Anthropic API 的 `cache_control` 字段挂在 `system` 数组的每个 block 上和 `messages[].content[]` block 上。需确认 adapter 当前如何序列化 `system` 字段（单字符串 vs 结构化数组），并根据实际情况选择改造方式。

**改造前提条件**：先确认 Anthropic adapter 目前的 `system` 序列化方式，再决定是改为 `system: Vec<SystemBlock>` 还是通过其他方式注入缓存标记。

#### 3.4.3 Section 的 cache_scope 分配（拆分 Identity 后）

```
System（Identity 拆分后） → CacheScope::Static
Tooling（Identity 拆分后）→ CacheScope::None（工具集可变）或 Static（工具集稳定时）
PlanningInstruction       → CacheScope::Static（同 planner 模式不变时）
ProjectContext            → CacheScope::Workspace
SessionSummary            → CacheScope::None
RecentMessages            → CacheScope::None
RetrievedMemories         → CacheScope::None
```

### 3.5 工具描述与实现拆分（可选，非本轮优先）

原方案将此列为独立阶段，评审意见认为收益被高估。**修订结论：降级为可选重构，前提条件满足时再做。**

**前提条件**（满足任意一条才启动）：
1. 工具描述需要根据运行时配置（如最大行数、工作区路径）动态生成，原位字符串常量维护明显失控
2. 多个工具共享描述模板逻辑，提取公共函数的收益超过拆分成本

**现状判断**：`edit`、`write`、`exec_command` 的描述改进均为纯文本，直接原位修改 `description()` 返回值即可，不需要拆文件或改 trait。

---

## 4. 实施路径

### 阶段 1：参数 Schema + 弱描述工具（P0，低风险，高收益）

**改动范围：**
- `crates/action/tools/src/tools/read_file.rs`：补全 3 个字段 description
- `crates/action/tools/src/tools/list_dir.rs`：补全 4 个字段 description
- `crates/action/tools/src/tools/grep_files.rs`：补全 4 个字段 description + description 文本微调
- `crates/action/tools/src/tools/edit.rs`：description 文本重写 + 补全字段 description
- `crates/action/tools/src/tools/write.rs`：description 文本重写 + 补全字段 description
- `crates/action/tools/src/tools/unified_exec.rs`：description 文本重写 + 补全所有字段 description

**验证方式：**
1. `cargo test -p talos-tools`
2. 构建 prompt 后打印 tool summary，肉眼确认格式和内容
3. 运行 e2e（`tests/integration/run_e2e.sh`），观察工具调用错误是否减少

### 阶段 2：Planner 指导补强（P1，中风险，中收益）

**改动范围：**
- `crates/orchestration/agent-kernel/src/planner.rs`：
  - `BUILT_IN_TOOL_GUIDANCE` 末尾追加 Block A + B + C
  - `PLAN_REACT_INSTRUCTION` 末尾追加 Block A + B + C

**注意事项：**
- 检查新增内容是否与 `build_identity_prompt` 中 `phase_instruction` 重叠
- 检查 `PLAN_REACT_INSTRUCTION` 格式要求段（`/*PLANNING*/` 等）是否仍在末尾

**验证方式：**
1. `cargo test -p talos-agent-kernel`（planner 单测）
2. e2e 测试，重点验证：exec_command 是否还被错误调用来读文件

### 阶段 3：Prompt Caching（P2，架构项，单独排期）

**前置依赖（启动前必须完成）：**
1. `Identity` section 拆分为独立的 System section 和 Tooling section（见 0.4 节），否则无法对 Tooling 和 System 分别设置正确的 CacheScope
2. 确认 Anthropic adapter 当前 `system` 字段序列化方式（`anthropic/src/lib.rs`，单字符串 vs 结构化数组）
3. 确认 `ChatRequest` 序列化格式是否需要 `system: Vec<SystemBlock>` 而非 `String`

**实施原则**：`CacheScope` 是 Prompt 层统一复用语义；原生缓存是否生效由 provider capability（`PromptCacheSupport`）决定。本阶段先完成 Anthropic 首个落地实现，同时为其他 provider 提供显式 `Unsupported` 能力声明，避免将 Anthropic 行为误当作通用行为。

**改动范围（需按顺序执行）：**
1. `crates/common/prompt/src/types.rs`：拆分 `PromptSectionKind::Identity` → `System` + `Tooling`；新增 `CacheScope` 枚举；`PromptSection` 增加 `cache_scope` 字段
2. `crates/common/prompt/src/assembler.rs`：拆分 `build_identity_prompt` 的 System 部分和 Tooling 部分到各自 section
3. `crates/common/llm/provider/src/lib.rs`：新增 `PromptCacheSupport` 枚举；`LlmProvider` trait 增加默认方法 `prompt_cache_support() -> PromptCacheSupport`；`ChatMessage` 增加 `cache_control` 字段；`Usage` 增加 `cache_creation_input_tokens` / `cache_read_input_tokens: Option<u32>` 字段
4. `crates/common/prompt/src/renderer.rs`：映射 `CacheScope` → `cache_control`
5. `crates/common/llm/anthropic/src/lib.rs`：override `prompt_cache_support()` 返回 `RequestLevel`；adapter 消费 `cache_control` 并序列化到 Anthropic 请求；解析 response usage 中的缓存字段

**验证方式：**
1. 抓取 Anthropic API 实际请求包，确认 `cache_control` 正确携带
2. 对比 `usage.cache_read_input_tokens` 的计费差异确认缓存命中
3. 验证其他 provider（Bailian 等）在 `Unsupported` 声明下，`cache_control` 字段不被序列化到其请求中

---

## 5. 本轮不做的事

| 事项 | 原因 |
|------|------|
| `Session-Specific Guidance` 新 section | `build_identity_prompt` 已注入 tool summary 和 environment，planner 已注入工具使用指导；新增 section 会导致三处重复，prompt 变长但信息密度不提升 |
| 工具描述与实现拆分（`prompt.rs`）| 当前改动均为纯文本，原位修改足够；拆文件在改动完成后无附加收益 |
| 修改 `Tool` trait 增加 `prompt()` 方法 | 动态 prompt 生成当前无明确需求；如未来需要再评估 |

---

## 6. 关键文件路径

| 改进目标 | 文件路径 |
|---------|---------|
| 工具描述 + Schema | `crates/action/tools/src/tools/{read_file,list_dir,grep_files,edit,write,unified_exec}.rs` |
| Planner 指导常量 | `crates/orchestration/agent-kernel/src/planner.rs` |
| PromptSection 类型、CacheScope | `crates/common/prompt/src/types.rs` |
| Prompt 渲染器 | `crates/common/prompt/src/renderer.rs` |
| Provider capability 声明、ChatMessage、UsageStats | `crates/common/llm/provider/src/lib.rs` |
| ChatMessage 定义 | `crates/common/llm/provider/src/lib.rs` |
| Anthropic 适配器 | `crates/common/llm/anthropic/src/lib.rs` |
