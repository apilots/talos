# Memory LLM Tree-Search 设计方案

## 1. 背景与问题

### 当前实现缺陷

| 层面 | 问题 |
|------|------|
| 检索 | `retrieval.rs` substring scoring，字面不匹配就 score=0 |
| 写入格式 | `## Durable Memory HH:MM`，标题零语义 |
| 写入去重 | 扫描所有文件做全文包含判断，重复确认、持续性偏好、跨 session 延续任务被吞掉 |
| 结构 | free-text summary，后续流程无法强类型处理 |

### 目标

1. 写入侧：结构化 JSON summary → 渲染为语义 md，标题携带核心点
2. 索引侧：每个 section 存足够 facet，而不只是标题
3. 检索侧：两跳语义检索（文件选候选 → section 精筛），而不是全量平铺一次喂给 LLM
4. 接口侧：`Memory::search_memory` 保留，内部换 LLM 实现，tool 层只做参数适配

---

## 2. 写入路径改造

### 2.1 LLM 返回结构化 JSON

Summary 生成 prompt 改为强制 JSON 输出：

```
Extract durable memory from this conversation as JSON.
Return ONLY valid JSON, no markdown fences, no explanation.

Schema:
{
  "title": "one-line core topic, max 12 words, no trailing punctuation",
  "facts": ["..."],
  "preferences": ["..."],
  "open_tasks": ["..."],
  "risks": ["..."]
}

Omit any field whose array would be empty. Keep total under 200 words.
```

**LLM 返回示例：**
```json
{
  "title": "User prefers ephemeral sessions; flush on close required",
  "facts": [
    "Session data must be flushed on close per USER.md.",
    "Assistant acknowledged and confirmed compliance."
  ],
  "preferences": [
    "Strict session hygiene; no residual data retention."
  ]
}
```

### 2.2 强类型结构体

```rust
// crates/cognition/memory/src/store.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMemorySummary {
    pub title: String,
    #[serde(default)]
    pub facts: Vec<String>,
    #[serde(default)]
    pub preferences: Vec<String>,
    #[serde(default)]
    pub open_tasks: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
}

pub struct WorkspaceMemoryRecord {
    pub timestamp: DateTime<Utc>,
    pub session_id: String,   // 新增，由调用方注入
    pub title: String,        // 语义标题，非 Option
    pub content: String,      // 仅 body，不含 heading 行
}
```

### 2.3 渲染协议（职责划分）

**`WorkspaceMemoryRecord.content` 只存 body**，不含 heading 行。

**`workspace_store.rs` 统一负责拼 heading**：

```rust
let payload = format!(
    "## [{}] {}\nsession: {}\n\n{}\n",
    record.timestamp.format("%H:%M"),
    record.title,
    record.session_id,
    record.content.trim(),
);
```

`render_summary` 只生成 body 部分（`**Facts:**` 以下），不含 `## [HH:MM]`：

```rust
fn render_summary_body(summary: &SessionMemorySummary) -> String {
    let mut out = String::new();
    let sections = [
        ("**Facts:**",       &summary.facts),
        ("**Preferences:**", &summary.preferences),
        ("**Open Tasks:**",  &summary.open_tasks),
        ("**Risks:**",       &summary.risks),
    ];
    for (heading, items) in &sections {
        if !items.is_empty() {
            out.push_str(heading);
            out.push('\n');
            for item in *items {
                out.push_str(&format!("- {}\n", item));
            }
            out.push('\n');
        }
    }
    out
}
```

**最终写入 `.md` 效果：**
```markdown
## [10:41] User prefers ephemeral sessions; flush on close required
session: f3a2b1c0-9d8e-4f7a-b6c5-1234567890ab

**Facts:**
- Session data must be flushed on close per USER.md.
- Assistant acknowledged and confirmed compliance.

**Preferences:**
- Strict session hygiene; no residual data retention.

```

### 2.4 写入去重策略修订

**背景**：MEMORY 是 session 级 flush、workspace 级共享，每次 flush 已经是对本 session 完整对话的 LLM 压缩摘要，内容质量有保证。去重的目的是防止同一 session 在异常路径下重复 flush 写入完全相同的块，而非抑制跨 session 的相似内容。

**当前问题**：跨文件全文 `contains` 判断，会误杀跨 session 的持续性偏好、延续任务等有效记忆。

**新策略**：仅在当日文件内做**块级 (session_id + title) 去重**——按 `## ` 边界解析当日文件，提取每个块的 session_id 和 title，命中则跳过写入：

```rust
async fn is_duplicate_in_today(
    store: &MemoryStore,
    record: &WorkspaceMemoryRecord,
) -> TalosResult<bool> {
    let file_path = store.workspace_memory_file_for(record.timestamp);
    if !file_path.exists() {
        return Ok(false);
    }
    let content = fs::read_to_string(&file_path).await?;
    // 按 "## [" 边界切块，每块独立检查 session_id + title
    for block in content.split("\n## [") {
        let has_session = block.contains(&format!("session: {}", record.session_id));
        let has_title = block.contains(&record.title);
        if has_session && has_title {
            return Ok(true);
        }
    }
    Ok(false)
}
```

跨 session 的相似内容**允许写入**，保证召回完整性。

---

## 3. 索引设计

### 3.1 每个 section 存足够 facet

`MemoryIndexSection` 不只存标题，增加结构化字段，为 LLM 第一跳提供足够信号：

```rust
pub struct MemoryIndexSection {
    pub title: String,          // ## 标题行的语义内容
    pub session_id: String,     // 该块对应的 session
    pub summary_preview: String, // facts 前两条拼成的预览（≤ 100 chars）
    pub start_line: usize,
    pub end_line: usize,
}

pub struct MemoryIndexFile {
    pub path: String,           // "memory/2026-04-04.md"
    pub date: String,           // "2026-04-04"
    pub sections: Vec<MemoryIndexSection>,
}
```

### 3.2 索引文件格式

`memory/2026-04-04.index.json`：

```json
{
  "path": "memory/2026-04-04.md",
  "date": "2026-04-04",
  "sections": [
    {
      "title": "User prefers ephemeral sessions; flush on close required",
      "session_id": "f3a2b1c0-9d8e-4f7a-b6c5-1234567890ab",
      "summary_preview": "Session data must be flushed on close. Assistant acknowledged compliance.",
      "start_line": 1,
      "end_line": 14
    },
    {
      "title": "Tool-call limit hit; session flush intent and privacy noted",
      "session_id": "a1b2c3d4-5e6f-7890-abcd-ef1234567890",
      "summary_preview": "Tool-call rounds exceeded. Session flush intent maintained.",
      "start_line": 16,
      "end_line": 30
    }
  ]
}
```

### 3.3 索引重建触发

**写入时触发（增量）**：`append_workspace_memory` 完成后重建当日索引文件。

**全量 rebuild（bootstrap/迁移/修复）**：

```rust
// crates/cognition/memory/src/retrieval.rs
pub async fn rebuild_all_indexes(store: &MemoryStore) -> TalosResult<()>
```

触发时机：
- 首次初始化 workspace（检测到 `.md` 存在但无对应 `.index.json`）
- 用户手工修改 `.md` 文件后
- 迁移历史数据时

`Memory` trait 暴露此方法供外部调用：

```rust
async fn rebuild_memory_indexes(&self) -> TalosResult<()>;
```

---

## 4. 两跳检索设计

### 4.1 整体流程

```
memory_search(query, max_results)
        │
        ▼
[Hop 1] 文件粗筛
  加载所有 .index.json 的文件级摘要
  （date + 各 section title + preview，不含完整 section 列表）
  → LLM 选出候选文件列表
        │
        ▼
[Hop 2] section 精筛
  仅加载候选文件的完整 section 列表
  （title + session_id + summary_preview + line range）
  → LLM 选出相关 section 引用 [{path, from, to}]
        │
        ▼
[Fetch] 拉取正文
  对每个 section 调用 get_memory(path, from, lines)
  组装为 MemorySearchResult 返回
```

### 4.2 Hop 1：文件粗筛

传给 LLM 的文件摘要文本（每个文件只列 date + 前几条 section title）：

```
2026-04-03 (3 sections)
  - Session flush requested; no state persistence
  - Tool-call limit hit; privacy preference noted
  - Ephemeral session confirmed per USER.md

2026-04-04 (4 sections)
  - Strict session hygiene required; flush acknowledged
  - Session flush protocol confirmed; audit gap flagged
  - No state persistence; USER.md alignment
  - Session flush confirmed; no pending tasks
```

Prompt：
```
Query: "<user query>"
Which dates/files likely contain relevant information?
Return a JSON array of file paths: ["memory/YYYY-MM-DD.md", ...]
If all files are irrelevant, return [].
```

### 4.3 Hop 2：section 精筛

仅对 Hop 1 返回的候选文件展开完整 section 列表：

```
memory/2026-04-04.md:
  L1-14:  "Strict session hygiene required; flush acknowledged"
          session: f3a2b1c0 | "Session data must be flushed. Assistant confirmed."
  L16-30: "Session flush protocol confirmed; audit gap flagged"
          session: a1b2c3d4 | "Flush protocol acknowledged. Audit gap identified."
```

Prompt：
```
Query: "<user query>"
Which sections are most relevant? Return at most <N>:
[{"path": "memory/...", "from": <line>, "to": <line>}]
```

### 4.4 接口层

**保留 `Memory::search_memory`**，LLM 调用在 memory retriever 内部封装，tool 层只做参数适配：

```rust
// trait.rs（保留，改内部实现）
async fn search_memory(&self, query: &MemorySearchQuery) -> TalosResult<Vec<MemorySearchResult>>;

// retriever.rs（新增，封装两跳逻辑）
pub(crate) async fn semantic_search(
    store: &MemoryStore,
    llm: Arc<dyn LlmProvider>,
    query: &MemorySearchQuery,
) -> TalosResult<Vec<MemorySearchResult>>
```

**`MemoryStore` 新增 LLM 字段**：

```rust
pub struct MemoryStore {
    pub(crate) workspace_dir: PathBuf,
    pub(crate) llm: Arc<dyn LlmProvider>,   // 新增
    // ...其余字段不变
}
```

`MemoryStore::from_config` 同步更新，接受 `Arc<dyn LlmProvider>`。

---

## 5. 类型变更

### 删除字段

| 类型 | 删除字段 |
|------|----------|
| `MemorySearchResult` | `score: f32`, `citation: Option<String>` |
| `MemorySearchQuery` | `min_score: Option<f32>` |
| `RetrievedMemoryItem` | `score: f32`, `citation: Option<String>` |

### `MemorySearchResult` 更新后

```rust
pub struct MemorySearchResult {
    pub path: String,
    pub snippet: String,
    pub start_line: usize,
    pub end_line: usize,
}
```

### `MemorySearchQuery` 更新后

```rust
pub struct MemorySearchQuery {
    pub query: String,
    pub max_results: usize,
}
```

---

## 6. Memory Trait 变更

```rust
#[async_trait]
pub trait Memory: Send + Sync {
    // 保留，内部实现换成 LLM 两跳检索
    async fn search_memory(&self, query: &MemorySearchQuery) -> TalosResult<Vec<MemorySearchResult>>;

    // 保留不变
    async fn get_memory(&self, path: &str, from: Option<usize>, lines: Option<usize>) -> TalosResult<MemoryGetResult>;

    // 新增：全量重建索引
    async fn rebuild_memory_indexes(&self) -> TalosResult<()>;

    // 删除（内部化）：不再暴露 memory_index()

    // ...其余 session 相关方法不变
}
```

---

## 7. 依赖注入路径（wiring 闭环）

### 问题

`MemorySearchTool` 当前在 `execute()` 内部按 `workspace_dir` 临时 `MemoryStore::new(...)` 构造 store。新方案要求 `MemoryStore` 持有 `Arc<dyn LlmProvider>`，临时构造拿不到 LLM，两跳检索无法执行。

### 方案：`ToolContext` 携带共享 `Arc<MemoryStore>`

复用已有的 `CollaborationRuntime` 注入先例——`ToolContext` 已经有 `collaboration_runtime: Option<Arc<CollaborationRuntime>>`，按同一模式新增 `memory_store` 字段：

```rust
// context.rs
pub struct ToolContext {
    // ...已有字段...
    collaboration_runtime: Option<Arc<CollaborationRuntime>>,
    memory_store: Option<Arc<MemoryStore>>,   // 新增
}

impl ToolContext {
    pub fn with_memory_store(mut self, store: Arc<MemoryStore>) -> Self {
        self.memory_store = Some(store);
        self
    }

    pub fn memory_store(&self) -> Option<Arc<MemoryStore>> {
        self.memory_store.clone()
    }
}
```

### Kernel 侧注入（flow.rs）

Kernel 构建 context 时把已有的 `memory_store` 注入进去：

```rust
// ToolRegistry::build_context 或 flow.rs 的 tool context 构建处
let context = registry
    .build_context(session_id, user_id, Some(kernel_config.clone()))
    .with_memory_store(memory_store.clone());   // memory_store 已在 flow 中持有
```

### Tool 侧使用

`MemorySearchTool::execute` 不再自行构造 store，从 context 取：

```rust
async fn execute(&self, input: Value, context: ToolContext) -> TalosResult<ToolOutput> {
    let Some(store) = context.memory_store() else {
        return Ok(disabled_output("memory store unavailable"));
    };
    // store 已持有 LLM，直接调用两跳检索
    let results = store.search_memory(&query).await?;
    ...
}
```

### 注册签名不变

```rust
// lib.rs（不变）
pub fn register_core_local_tools(registry: &ToolRegistry) {
    registry.register(Arc::new(MemorySearchTool));
    registry.register(Arc::new(MemoryGetTool));
}
```

`MemoryStore` 在 kernel 初始化时构建并注入 LLM：

```rust
// flow.rs / runner.rs
let memory_store = Arc::new(MemoryStore::from_config(&kernel_config.memory, llm_provider.clone()));
```

---

## 8. 完整改动文件清单

| 文件 | 改动类型 |
|------|----------|
| `crates/cognition/memory/src/store.rs` | 删除 score/citation；新增 SessionMemorySummary、MemoryIndexSection/File；WorkspaceMemoryRecord 新增 session_id，title 去 Option；MemoryStore 新增 llm 字段 |
| `crates/cognition/memory/src/trait.rs` | search_memory 保留；新增 rebuild_memory_indexes；删除 memory_index（内部化） |
| `crates/cognition/memory/src/retrieval.rs` | 删除 keyword 逻辑；新增 semantic_search（两跳）、load_index_files、rebuild_file_index、rebuild_all_indexes |
| `crates/cognition/memory/src/workspace_store.rs` | 写入格式：heading 统一由此处负责；去重改为当日 session_id+title 级别 |
| `crates/cognition/memory/src/memory_impl.rs` | search_memory 委托 semantic_search；实现 rebuild_memory_indexes |
| `crates/cognition/memory/src/lib.rs` | 导出更新 |
| `crates/action/tools/src/context.rs` | 新增 memory_store 字段 + with_memory_store / memory_store 方法 |
| `crates/action/tools/src/tools/memory.rs` | 从 context.memory_store() 取 store，不再自行构造；删除 min_score 参数 |
| `crates/action/tools/src/lib.rs` | register_core_local_tools 签名不变 |
| `crates/orchestration/agent-kernel/src/flow.rs` | MemoryStore::from_config 传入 llm_provider |
| `crates/orchestration/agent-kernel/src/tool_execution.rs` | build_context 后链式 .with_memory_store |
| `crates/orchestration/agent/src/agent.rs` | 两处 build_context 后链式 .with_memory_store |
| `crates/orchestration/agent-kernel/src/general_turn.rs` | summary prompt 改 JSON；解析 SessionMemorySummary；render_summary_body；写入改用新 WorkspaceMemoryRecord |
| `crates/common/prompt/src/lib.rs` | 更新 RetrievedMemoryItem 使用处（去 score/citation） |
| `crates/infra/config/src/lib.rs` | 删除 search_min_score |

---

## 9. 实现注意：ToolContext 构建路径全量补齐

`ToolContext` 新增 `memory_store` 字段后，若某条调用链没有注入，`memory_search` 会静默走 `disabled_output`，不报错、不崩溃，极难发现。

### 路径分类

**A. 生产路径（必须注入）**

`ToolRegistry` 只负责 tool 注册和基础 context 构建，不持有 `memory_store`。调用方在 `build_context()` 之后显式链式注入：

```rust
// tool_execution.rs / agent.rs 等调用处
let context = registry
    .build_context(session_id, user_id, Some(kernel_config.clone()))
    .with_memory_store(memory_store.clone());
```

需要更新的三个生产调用点：
- `tool_execution.rs:60` — `registry.build_context(...)`
- `agent.rs:281` — `self.tool_registry.build_context(...)`
- `agent.rs:702` — `self.tool_registry.build_context(...)`

这三处的调用方均已持有 `Arc<MemoryStore>`（kernel 初始化时构建），直接链式追加即可。`ToolRegistry` 本身不变。

**B. memory tool 测试（必须注入，否则测试验证的是 disabled 路径）**

`tools/memory.rs` 的所有测试用 `ToolContext::new_with_kernel_config` 直接构造 context，需改为同时注入 mock MemoryStore：

```rust
// 测试辅助函数
fn test_context_with_memory(workspace_dir: &Path, llm: Arc<dyn LlmProvider>) -> ToolContext {
    let store = Arc::new(MemoryStore::from_config_with_llm(workspace_dir, llm));
    ToolContext::new_with_kernel_config("session-1", "user-1", Some(test_kernel_config(workspace_dir)))
        .with_memory_store(store)
}
```

**C. 无关测试（不需要改）**

不测 memory tool 的测试夹具无需注入，`memory_store` 为 `None` 不影响它们的行为：
- `registry.rs` 中的 echo/policy/truncation 测试
- `runtimes/apply_patch.rs`、`exec.rs`、`fs_search.rs`

### 验收标准

实现完成后，确认以下均不走 `disabled_output`：
- `memory_search` 在 e2e 流程中有实际命中返回
- `memory_search` 在 tool 单测中断言结果非空（而非 `disabled: true`）

### 单测（`retrieval.rs`）
- `rebuild_file_index` 正确解析 `## [HH:MM] title` + `session:` 行，生成 `.index.json`
- `load_index_files` 正确加载多个 `.index.json`
- `rebuild_all_indexes` 处理无 `.index.json` 的已有 `.md` 文件
- 两跳 hop1/hop2 用 mock LLM 覆盖各路径

### 单测（`workspace_store.rs`）
- 写入后 `.md` 文件格式正确（heading 不重复）
- 同 session_id + title 当日重复写入被去重
- 不同 session_id 相似内容允许写入

### 单测（`general_turn.rs`）
- LLM 返回合法 JSON → `SessionMemorySummary` 正确反序列化
- `render_summary_body` 空数组 section 不输出 heading
- LLM 返回非法 JSON → 返回错误，不写 MEMORY

### 集成测试
- `run_e2e.sh` 回归
- 验证语义查询命中（构造含语义内容的 memory 文件 + 索引）
- 验证 `rebuild_all_indexes` 后历史文件可被搜索命中

---

## 10. 权衡说明

| 维度 | 说明 |
|------|------|
| 两跳 vs 一跳 | 两跳在文件数增多时避免单次长上下文膨胀；当前只有 2 个文件时 hop1 几乎无开销 |
| LLM 在 MemoryStore | 检索能力内聚在 memory 模块，tool 层薄，其他调用方可复用 |
| 全量 rebuild | 初始化/历史文件/手工修订场景有明确恢复路径，不依赖写入路径完整性 |
| 去重策略放宽 | 允许跨 session 写入相似内容，保证召回完整性，代价是文件稍大 |
| 索引 facet 增加 | summary_preview + session_id 给 LLM 第一跳足够信号，不只靠标题 |
