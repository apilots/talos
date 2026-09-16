# 模型驱动的用户 Workspace 记忆

Status: archived
Owner: agent
Created: 2026-08-23
Updated: 2026-08-27
Related: `refactor-039-memory-storage-retrieval`, `feat-041-agent-inner-model`, `feat-021-user-scoped-agent-workspace`
Reference: [Open Knowledge Format v0.2](https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md)

## 1. 结论

Talos 使用模型驱动的更新机制替代周期性根记忆刷新，目标文件由 effective Agent Workspace 解析得到：

```text
<effective-agent-workspace>/users/<user-id>/MEMORY.md
```

默认配置下，`<effective-agent-workspace>` 为 `<talos-home>/workspace-<agent>`。产品概念统一为 **User Workspace Memory**。其作用域由 `(effective-agent-workspace, user)` 唯一确定：同一 effective Agent Workspace、同一用户的多个 Session 共享该文件；不同用户或不同 effective Agent Workspace 相互隔离。两个 Agent 若显式配置为同一个 effective workspace root，则按配置共享文件，不宣称额外的 Agent ID 隔离。

唯一写入所有权为：

```text
Session model        -> users/<user-id>/MEMORY.md
Compaction/Scheduler -> users/<user-id>/memory/YYYY-MM-DD.md
```

模型通过一个结构化 Tool 和一个共享 Writer 边界更新 User Workspace Memory。主模型没有调用该 Tool 时，成功完成的 Turn 可以执行一次有界的 Inner Model 记忆反思。实际文件内容从可缓存的静态 Prompt 前缀移动到必需的动态 Prompt 区域，稳定的记录策略继续保留在静态区域。

Talos 新增纯格式库 `talos-knowledge-format`：完整实现 OKF v0.2 producer/consumer facade，并在标准层之上提供 Talos 结构化 Markdown 的 managed region、Entry CRUD、规范渲染和 Prompt projection。User Workspace Memory 在 `talos-memory` 中提供强类型业务封装；路径解析、文件锁、Workspace revision 和原子写入继续属于 `talos-service`。该格式库是后续 Daily Memory OKF 化可以复用的基础能力，但本次不改造 Daily Memory。

本 Spec 只替代 `refactor-039-memory-storage-retrieval` 中关于 `MEMORY.md` 维护和 Prompt 位置的设计。Daily Memory 的存储、索引、search、get、compaction 和形成流程保持不变。

## 2. 目标

1. 让 Session 模型成为 `users/<user-id>/MEMORY.md` 唯一的自动化写入方。
2. 保持同一 effective Agent Workspace 和用户作用域内的跨 Session 共享。
3. 为 User Workspace Memory 提供确定性的 Entry `create`、`update`、`upsert`、`replace`、`remove`、`get`、`list` 和整文档 Replace 语义。
4. 在 `talos-knowledge-format` 中完整覆盖 OKF v0.2 的规范对象、producer/consumer 行为与 conformance，并在其上提供有大小上限的 Talos managed document API。
5. 每个新 Turn 重新加载当前记忆，同时不破坏稳定 Prompt 前缀缓存。
6. 复用现有 effective workspace resolver、`MemoryScope`、Tool Context、`InnerModelClient`、workspace revision、文件锁和原子文件写入。
7. 保持 User Workspace Memory 与 Daily Memory 的形成和存储相互独立。

## 3. 非目标

1. 不在 `<agent-workspace>/MEMORY.md` 增加 Agent 全局记忆。
2. 不合并 Agent 根目录和用户 Workspace 下的记忆文件。
3. 不迁移或解释旧自动维护 marker。
4. 不改变 `USER.md` 的所有权，也不增加 `USER.md` 自动更新。
5. 不改变 `memory/YYYY-MM-DD.md`、SQLite 索引、`memory_search` 或 `memory_get` 行为。
6. 不新增 Memory Agent、文件 watcher、scheduler、event bus、数据库表或通用 Knowledge Repository。
7. 不在本次改造 Daily Memory 的 Markdown、形成、存储或检索实现；只提供其后续可以复用的格式基础能力。
8. 不自行重写 OKF 标准实现；`talos-knowledge-format` 通过稳定 facade 使用 Rust OKF 实现，并由契约测试约束兼容行为。
9. 不把 effective Agent Workspace 整体声明为 OKF Bundle；User Workspace Memory 本次只使用完整 OKF facade 中的 Concept Document 能力。
10. 不在格式库中定义文件系统、数据库、serving、全文检索或 attestation runtime；这些本来就是 OKF v0.2 的非目标。

## 4. 当前实现事实

本设计基于以下现有实现：

1. `resolve_effective_kernel_workspace` 将配置的 Agent Workspace 解析为 `<base>/users/<user-id>`，并用该有效目录替换 `kernel_config.workspace_dir`。
2. `PromptBuilder` 在每次 Prompt assembly 时从有效 `workspace_dir` 读取 Workspace bootstrap 文件。
3. `MEMORY.md` 当前由 `WorkspaceBootstrapFileKind::Memory` 表示，以 `BootstrapMemory` 放入静态前缀，并参与 bootstrap fingerprint。
4. `WorkspaceDocService` 已经为 `WorkspaceDocKind::Memory` 提供 actor scope 下的读取和带版本校验的整文件写入。
5. `MemoryScope { agent_id, user_id, workspace_root }` 承载 Tool 和 Runtime 使用的可信 Agent、用户及有效 Workspace 边界。
6. `ToolContext` 已经支持应用层注入的 Memory capability 和可信 `MemoryScope`。
7. `talos_fs::with_file_lock` 提供 sidecar 文件锁，`talos_fs::write_file` 使用安全的原子替换。
8. `AgentHarnessImpl` 已经持有 `InnerModelClient`，`ModelUsageKind::Memory` 已经标识 User Workspace Memory 辅助模型调用。
9. `MemoryDailyMaintenance` 当前会重写每个有效用户 Workspace 中 `MEMORY.md` 的旧自动维护区域；该路径与 Session 驱动更新冲突，必须删除。
10. 现有 `talos-memory::MemoryReader/DreamMemoryFormationStore`、`MemoryService` 和 `MemoryStore` 面向 `memory/YYYY-MM-DD.md`：Writer 写入形成批次并同步 SQLite，Reader 通过 Store 执行 `search/get`。它们不是 User Workspace Memory 的文档 API。
11. 现有 `WorkspaceDocService` 对 `MEMORY.md` 只提供整文件读取和带版本校验的写入，没有稳定 key Entry CRUD；新 Service 必须复用其 effective workspace、revision、文件锁和原子写入基础设施。
12. `WorkspaceBootstrapFileKind::Memory` 当前受 `bootstrap.enabled`、`bootstrap.include_memory` 和 `PromptMode` 控制，并由 Prompt Builder 自动创建旧版无 frontmatter 模板；该路径与 required dynamic Memory 和 Missing 状态冲突。
13. Tool 执行还受 `talos-tools-policy` allow-list 约束；`AlwaysLoad` 只保证 Tool 选择可见性，不授予执行权限。
14. 当前 streamed Agent Harness 会先发送 terminal event，再执行部分 completion 后置逻辑；Memory reflection 必须定义比现有非关键后置任务更严格的完成顺序。

早期目录结构可能仍在 Agent 根目录留下 `MEMORY.md`，但当前 effective workspace 解析不会加载或更新该文件。

## 5. 规范文件格式

默认模板和 Replace 模板保持现有内容结构，并使用 OKF v0.2 的 `status: stable` 与完整 `generated` producer metadata。该模板作为 User Workspace Memory Profile 的规范模板：

```markdown
---
type: UserWorkspaceMemory
title: User Workspace Memory
description: Durable cross-session memory scoped to one agent and one user.
tags:
  - talos
  - agent
  - user-memory
status: stable
generated:
  by: talos-agent/talos
  at: 2026-08-28T09:30:00Z
---

# User Workspace Memory

<!--
Keep only information that remains useful across Sessions.
Record conclusions instead of chronological activity.
Update or remove stale entries instead of accumulating history.
-->

<!-- talos:user-workspace-memory:start -->

## Interaction Guidance

<!-- Model-learned collaboration preferences and confirmed feedback. -->

## Key Knowledge

<!-- Durable facts, decisions, constraints, lessons, and references. -->

## Active Context

<!-- Cross-session goals, commitments, blockers, ownership, and next steps. -->

<!-- talos:user-workspace-memory:end -->
```

每个 User Workspace Memory Entry 包含一个稳定 key 和一个三级标题：

```markdown
<!-- talos:user-workspace-memory:entry key="memory-ownership" -->
### Memory ownership

- **Kind:** Decision
- **Summary:** Session model exclusively maintains User Workspace Memory.
- **Why:** Scheduled rewriting conflicts with concurrent Session updates.
- **Apply:** Daily maintenance only writes Daily Memory.
```

Active Context 中的 User Workspace Memory Entry 可以额外包含 `Status` 和 `Next` 字段。为空的可选字段不输出。key 在 User Workspace Memory region 内唯一，User Workspace Memory Entry 在 section 之间移动时保持 key 不变。

完整文件继续遵守 `MEMORY_FILE_MAX_CHARS`，当前上限为 8,000 characters。

## 6. 记录策略

只有当信息可能改变未来 Session 的行为时，模型才应记录：

- 用户明确要求记住、纠正或忘记某项信息；
- 稳定的协作偏好或经过确认的反馈；
- 跨 Session 有效的事实、决策、约束或承诺；
- 必须延续到其他 Session 的目标、负责人、阻塞或下一步；
- 从已完成任务或错误中总结出的可复用教训。

模型不得记录：

- 对话摘要、时间线流水账或常规执行步骤；
- 临时状态、短期计划或没有未来价值的已完成上下文；
- 可以直接从当前代码仓库文件重新获取的事实；
- 推测、推断出的敏感信息或秘密；
- `USER.md` 或 User Workspace Memory 中已经存在的重复信息。

当前结论变化时，必须更新或删除已有 User Workspace Memory Entry。一个当前 User Workspace Memory Entry 可以表达完整结论时，不得继续追加历史记录。

## 7. Knowledge Format 基础库

新增独立的纯格式 crate：

```text
crates/common/knowledge-format/
  Cargo.toml
  src/
    error.rs
    lib.rs
    knowledge_format_tests.rs
    okf/{mod,types,frontmatter,document,bundle,index,log,computation,conformance,diagnostics}.rs
    managed_document/{mod,profile,entry,document,parser,renderer}.rs
```

`talos-knowledge-format` 将 `okf-core` 作为私有运行时实现依赖，并将 `okf-validator` 作为官方 conformance 契约测试 oracle。`okf-validator 0.2.4` 只接受磁盘 `okf_core::Bundle`，因此不得为了在生产路径调用它而让纯格式库创建临时文件；官方 validator 只在测试中校验 Talos 内存 facade 的兼容结果。上层只依赖 Talos 强类型 facade，不直接依赖或接收上游 crate 类型。该 crate 不读取或写入文件，不知道 `MemoryScope`、Workspace revision、Session、模型、Tool、SQLite 或 transport；Bundle 由调用方提供的内存文件集合构建，上游涉及路径的 API 不向 Talos 调用方暴露。

### 7.1 完整 OKF v0.2 facade

“完整实现 OKF”指 Talos facade 覆盖 OKF v0.2 的全部规范对象和 producer/consumer 语义，而不是只解析 User Workspace Memory 使用到的字段：

- Bundle tree、Concept ID、`index.md`、`log.md` 和 reserved filename；
- Concept Document、UTF-8 Markdown body、完整 frontmatter 和未知扩展字段保留；
- `type`、`title`、`description`、`resource`、`tags`；
- provenance：`sources`、credibility signals、usage window、footnote attribution；
- trust/lifecycle：actor、`generated`、`verified`、trust tier、`status`、`stale_after`；
- Markdown cross-link、path-valued field、resolved/broken link graph 和 `references/` convention；
- Attested Computation：runtime、parameters、inline/file computation、executor、receipt fields 和 attester contract；
- conformance、recommended-field diagnostics、permissive consumption、OKF version 和 v0.1 consumer fallback；
- 规范 parse/render、Index/Log parse/render/generate 和 bundle-level validation。

Attestation 执行、receipt/verdict wire protocol、文件存储和查询基础设施不属于 OKF v0.2 格式实现。格式库只建模并校验 Attested Computation contract，不运行 executor 或 attester。

Talos 标准 facade 至少公开以下稳定类型族：

```rust
pub struct OkfDocument;
pub struct OkfFrontmatter;
pub struct OkfDocumentBuilder;
pub struct OkfExtensionFields;
pub struct OkfVersion;
pub struct OkfDate;
pub struct OkfDateTime;
pub enum OkfDateTimeValue;
pub struct OkfBundle;
pub struct OkfBundleBuilder;
pub struct OkfBundleFile;
pub struct OkfConcept;
pub struct OkfConceptId;
pub struct OkfSource;
pub struct OkfUsageWindow;
pub struct OkfAttribution;
pub enum OkfResourceKind;
pub struct OkfActor;
pub struct OkfGenerated;
pub struct OkfVerification;
pub enum OkfTrustTier;
pub enum OkfStatus;
pub struct OkfLink;
pub enum OkfLinkKind;
pub struct OkfResolvedLink;
pub struct OkfResolvedSource;
pub struct OkfIndex;
pub struct OkfIndexEntry;
pub struct OkfLog;
pub struct OkfLogDay;
pub struct OkfLogEntry;
pub struct OkfAttestedComputation;
pub struct OkfInlineComputation;
pub struct OkfParameter;
pub struct OkfExecutor;
pub struct OkfAttester;
pub enum OkfComputationSource;
pub struct OkfValidationReport;
pub struct OkfDiagnostic;
pub enum OkfDiagnosticSeverity;
```

公共标准 API 为：

```text
OkfDocument::{builder, parse}
document.{validate, missing_recommended, render}
document.{links, sources, attributions}
document.{generated, verifications, trust_tier, status, is_stale_at}
document.attested_computation()
OkfFrontmatter::builder().{generated, verifications, sources, usage_window}
OkfFrontmatter::builder().{stale_after, computation_runtime, computation_parameters}
OkfFrontmatter::builder().{computation_resource, executor, attester}
OkfDocument::builder().inline_computation(OkfInlineComputation)
OkfBundle::builder().file(OkfBundleFile { path, content }).build()
bundle.{validate, concepts, parse_errors}
bundle.{concepts_of_type, attested_computations, tags, okf_version, stale_at}
bundle.{resolve_links, resolve_sources, backlinks, derived_from, derives, diff}
OkfIndex::{parse, generate}
index.render()
OkfLog::parse(content)
log.render()
```

Facade 必须遵守 OKF permissive conformance：只有 Concept 的非空 `type` 和保留文件结构违反才是 Error；存在时各标准 family 的形状问题只产生 Warning/Info，不得因为缺少可选字段、未知 type、未知扩展字段、断链或缺少 Index 而拒绝消费。未知 frontmatter 字段和原始有效值必须 round-trip 保留，但作为 opaque extension 只公开稳定字段名，不公开任意 YAML/JSON Value，也不提供通用 extension 写入口；所有标准 family 只能通过强类型对象和 Builder 读写。单个 `verified` mapping 必须规范化为一项列表，v0.1 `timestamp` 和 `# Citations` 只作为 consumer fallback，不自动改写源文档。

### 7.2 Talos Managed Knowledge Document

Talos managed document 是完整 OKF facade 之上的扩展，用来表达稳定 key Entry CRUD，不属于 OKF 标准本身。公共业务无关类型为：

```rust
pub struct KnowledgeDocumentProfile;

pub struct KnowledgeDocument;

pub struct KnowledgeEntry {
    pub key: EntryKey,
    pub section: SectionKey,
    pub title: String,
    pub body: MarkdownFragment,
}

pub struct KnowledgeEntryUpdate {
    pub section: Option<SectionKey>,
    pub title: Option<String>,
    pub body: Option<MarkdownFragment>,
}

pub struct KnowledgeEntryQuery {
    pub section: Option<SectionKey>,
    pub key_prefix: Option<String>,
    pub limit: Option<usize>,
}
```

`KnowledgeDocumentProfile` 定义一类文档的完整默认 `OkfFrontmatter`、region namespace、section 和硬限制，不额外定义第二个 Meta 对象。其字段不公开，只能通过 Builder 构造并通过只读 getter 访问；`KnowledgeDocumentBuilder::build()` 还会再次校验 Profile，不依赖调用方维持不变量。User Workspace Memory Profile 使用 `type = UserWorkspaceMemory` 和 `status = stable`；其他可选 OKF family 可以缺省，但完整标准类型始终可用。

所有输入对象提供 Builder：

```rust
KnowledgeDocumentProfile::builder()
KnowledgeEntry::builder()
KnowledgeEntryUpdate::builder()
KnowledgeEntryQuery::builder()
```

对外以 `KnowledgeDocument` 为统一文档入口，Builder 是链式 facade：

```rust
KnowledgeDocument::builder(profile)
    .create_document()
    .create_entry(entry)
    .build()

KnowledgeDocument::builder(profile)
    .from_markdown(content)
    .update_entry(key, update)
    .upsert_entry(entry)
    .replace_entry(entry)
    .remove_entry(key)
    .build()

KnowledgeDocument::builder(profile)
    .replace_document(replacement)
    .build()

document.get_entry(key)
document.list_entries(query)
document.render()
document.prompt_view()
```

Builder 内部可以使用私有 Command 表达操作顺序，但不公开 `KnowledgeDocumentPatch`、`Mutation` 或 `Edit` 领域对象。`build()` 在内存中按顺序执行所有命令并统一校验，任一命令失败时不产生部分结果。

通用语义固定为：

- `create_entry`：key 必须不存在，否则返回 `AlreadyExists`；
- `update_entry`：对已有 Entry 做字段级更新，key 不存在返回 `NotFound`；
- `upsert_entry`：key 不存在时创建，存在时整体覆盖，section 改变时移动 Entry；
- `replace_entry`：使用 `entry.key` 整体替换已有 Entry，key 必须存在，不接受第二个 key 来源；
- `remove_entry`：从 managed region 删除 Entry，key 不存在视为幂等成功；
- `replace_document`：把完整 Markdown 重新解析为新文档并执行全部 Profile 校验；
- `get_entry` 和 `list_entries`：只查询已解析文档，不执行全文或语义检索；
- `render`：生成规范 OKF Markdown；
- `prompt_view`：移除 frontmatter、Talos marker、模板 comment 和多余空行。

格式层不同时提供语义重复的 `remove_entry` 和 `delete_entry`。`delete_document` 表示持久化资源生命周期，超出纯文档库职责。

结构安全约束固定为：

- Entry key 使用小写 ASCII `a-z0-9_-`，长度 1 到 64，且首字符必须是字母或数字；
- title 是长度不超过 160 characters 的单行文本，不得包含控制字符或 HTML comment；
- body 不得包含 Profile region marker、Entry marker或同 namespace 的保留 comment；
- Entry body 位于三级标题下，只允许四级及以下的子标题，避免破坏 section/Entry 边界；
- `list_entries` 按 Profile section 顺序，再按文件中的 Entry 顺序稳定返回；
- 所有 Builder 输入和最终 render 结果都执行上述校验及 Profile 大小上限。

外部调用示例：

```rust
let profile = KnowledgeDocumentProfile::builder()
    .frontmatter(user_workspace_memory_frontmatter)
    .region_namespace("talos:user-workspace-memory")
    .section("interaction-guidance", "Interaction Guidance")
    .section("key-knowledge", "Key Knowledge")
    .section("active-context", "Active Context")
    .max_chars(8_000)
    .build()?;

let entry = KnowledgeEntry::builder()
    .key("memory-ownership")
    .section("key-knowledge")
    .title("Memory ownership")
    .body(markdown)
    .build()?;

let updated = KnowledgeDocument::builder(&profile)
    .from_markdown(current_content)
    .upsert_entry(entry)
    .remove_entry("obsolete-entry")
    .build()?;

let file_content = updated.render()?;
let prompt_content = updated.prompt_view()?;
```

## 8. User Workspace Memory 领域 API

现有 `talos-memory` crate 增加业务模块：

```text
crates/cognition/memory/src/workspace_memory/
  mod.rs
  profile.rs
  types.rs
  document.rs
  ports.rs
  workspace_memory_tests.rs
```

该模块依赖 `talos-knowledge-format`，定义第 5 节模板对应的 Profile、强类型 Entry、业务校验和 Reader/Writer port。它复用现有 `MemoryScope`，不重复定义 user/workspace scope，不执行文件系统访问、模型调用、Session 编排或 transport 处理。

稳定的业务对象为：

```rust
pub enum UserWorkspaceMemorySection {
    InteractionGuidance,
    KeyKnowledge,
    ActiveContext,
}

pub struct UserWorkspaceMemoryProducer {
    agent_id: MemoryAgentId,
    generated_at: i64,
}

pub struct UserWorkspaceMemoryEntry {
    pub key: String,
    pub section: UserWorkspaceMemorySection,
    pub title: String,
    pub kind: MemoryKind,
    pub summary: String,
    pub why: Option<String>,
    pub apply: Option<String>,
    pub status: Option<String>,
    pub next: Option<String>,
}

pub struct UserWorkspaceMemoryEntryUpdate {
    pub section: Option<UserWorkspaceMemorySection>,
    pub title: Option<String>,
    pub kind: Option<MemoryKind>,
    pub summary: Option<String>,
    pub why: OptionalField<String>,
    pub apply: OptionalField<String>,
    pub status: OptionalField<String>,
    pub next: OptionalField<String>,
}

pub enum OptionalField<T> {
    Unchanged,
    Set(T),
    Clear,
}

pub struct UserWorkspaceMemoryEntryQuery {
    pub section: Option<UserWorkspaceMemorySection>,
    pub key_prefix: Option<String>,
    pub limit: Option<usize>,
}

pub enum UserWorkspaceMemoryUpdate {
    CreateEntry { entry: UserWorkspaceMemoryEntry },
    UpdateEntry {
        key: String,
        update: UserWorkspaceMemoryEntryUpdate,
    },
    UpsertEntry { entry: UserWorkspaceMemoryEntry },
    ReplaceEntry { entry: UserWorkspaceMemoryEntry },
    RemoveEntry { key: String },
    ReplaceDocument {
        content: String,
        base_version: ExpectedFileVersion,
    },
}

pub enum UserWorkspaceMemorySnapshot {
    Missing {
        template: UserWorkspaceMemoryDocument,
    },
    Valid {
        raw_content: String,
        updated_at: Option<i64>,
        version_token: String,
        document: UserWorkspaceMemoryDocument,
    },
    Invalid {
        raw_content: String,
        updated_at: Option<i64>,
        version_token: String,
        diagnostic: String,
    },
}
```

`OptionalField<T>` 明确区分可选字段的“不修改”“设置”和“清空”。`UserWorkspaceMemoryEntry`、`UserWorkspaceMemoryEntryUpdate` 和 `UserWorkspaceMemoryEntryQuery` 均提供 Builder。`UserWorkspaceMemoryProducer` 提供首次创建所需的强类型 Agent identity 和时间；`UserWorkspaceMemoryDocument::template(producer)` 写入完整 `generated`，后续 parse 和 Entry mutation 保留首次值。`UserWorkspaceMemoryDocument` 封装通用 `KnowledgeDocument`，负责强类型 Entry 与 Markdown body 的双向转换，并提供 `template`、`parse`、`get_entry`、`list_entries`、`render` 和 `prompt_view`。

`get_entry` 和 `list_entries` 在 Invalid 状态返回 `InvalidDocument`，不会对 raw content 做启发式解析。

应用层 port 提供完整、可发现的业务语义：

```rust
UserWorkspaceMemoryReader::read(scope) -> UserWorkspaceMemorySnapshot
UserWorkspaceMemoryReader::get_entry(scope, key) -> Option<UserWorkspaceMemoryEntry>
UserWorkspaceMemoryReader::list_entries(scope, query) -> Vec<UserWorkspaceMemoryEntry>

UserWorkspaceMemoryWriter::create_entry(scope, entry) -> UserWorkspaceMemorySnapshot
UserWorkspaceMemoryWriter::update_entry(scope, key, update) -> UserWorkspaceMemorySnapshot
UserWorkspaceMemoryWriter::upsert_entry(scope, entry) -> UserWorkspaceMemorySnapshot
UserWorkspaceMemoryWriter::replace_entry(scope, entry) -> UserWorkspaceMemorySnapshot
UserWorkspaceMemoryWriter::remove_entry(scope, key) -> UserWorkspaceMemorySnapshot
UserWorkspaceMemoryWriter::replace_document(scope, content, base_version)
    -> UserWorkspaceMemorySnapshot
```

文件不存在时，`read` 使用可信 `MemoryScope.agent_id` 和当前时间构建 `Missing { template }`，模板仅驻留内存，读取不得创建 `MEMORY.md`；第一次 Entry mutation 使用同一 producer 语义持久化。规范文件返回 `Valid` 及原始内容、revision 和解析后的 Document。缺失或不完整的 `generated` 与其他格式损坏一样返回 `Invalid`，并只能通过带 CAS 的 `replace_document` 修复。互斥 enum 从类型层禁止 Missing 携带 revision、Valid 缺少 Document 等非法组合。

Workspace revision 始终根据磁盘原始内容计算，不进入 `talos-knowledge-format` 或 `UserWorkspaceMemoryDocument`。Workspace Doc 的 Memory read 继续返回 raw content、mtime 和 raw revision；整文件 write 从“任意 Markdown”收紧为“通过 User Workspace Memory Profile 校验的规范 OKF Replace”，API shape 不变但写入行为属于显式 breaking change。

业务方调用示例：

```rust
let entry = UserWorkspaceMemoryEntry::builder()
    .key("preferred-output-style")
    .section(UserWorkspaceMemorySection::InteractionGuidance)
    .title("Preferred output style")
    .kind(MemoryKind::Preference)
    .summary("Prefer concise Chinese design explanations.")
    .build()?;

let snapshot = writer.upsert_entry(&scope, entry).await?;
let entries = reader
    .list_entries(
        &scope,
        UserWorkspaceMemoryEntryQuery::builder()
            .section(UserWorkspaceMemorySection::InteractionGuidance)
            .build()?,
    )
    .await?;
```

本次不为 User Workspace Memory 建立 SQLite 或语义搜索。它受 8,000-character 上限约束并完整进入动态 Prompt；按 key 或 section 的结构化查询由 Reader 提供。现有 `MemoryReader::search/get` 继续只服务 Daily Memory 长期检索。

## 9. 文档更新与并发语义

### 9.1 Entry 操作

- 所有 Entry 操作只修改 `talos:user-workspace-memory:start/end` region。
- 文件缺失时，Entry 操作以规范空模板为基础创建文件。
- 文件存在但不符合 User Workspace Memory Profile、缺少新 marker、包含旧 `talos:managed-memory` marker 或 marker 损坏时，Entry 操作返回 `InvalidDocument`，不得追加 region、迁移 marker 或重写外部内容；只能使用带 CAS 的整文档 Replace 修复。
- `create`、`update`、`upsert`、`replace` 和 `remove` 遵守第 7 节定义的通用语义。
- `remove_entry` 永远不删除 region 之外的内容；“忘记某项记忆”使用该操作。
- 写入前拒绝重复 key、非法 key、非法字段和超限结果。

### 9.2 整文档 Replace

- 使用规范内容替换整个文件，不提供直接删除 `MEMORY.md` 的业务 API。
- `base_version` 复用现有 `ExpectedFileVersion`：文件不存在时使用 `MustNotExist`，已存在时使用 `Match { version_token }`。
- 内容必须是规范 OKF Concept Document，包含且只包含一对顺序正确的 `talos:user-workspace-memory:start/end` marker，并允许零个或多个 User Workspace Memory Entry；存在的 Entry 必须全部合法。
- 版本过期、marker 损坏、内容非法或结果超限时拒绝写入。

### 9.3 并发

Entry 操作获取文件锁后读取最新文件，并在最新 Document 上执行。无关 key 的并发更新都应保留；同一 key 的并发 `upsert` 采用最后完成的写入生效；依赖 key 当前存在状态的 `create`、`update` 和 `replace_entry` 按锁内最新状态返回成功或冲突。

整文档 Replace 通过现有 Workspace revision token 实现 compare-and-swap。版本过期返回 `Conflict`，且不修改文件。模型不得提供 `user_id`、`workspace_root` 或绝对路径。

## 10. Service 与 Tool 边界

编排层 Service 实现位于：

```text
crates/orchestration/service/src/user_workspace_memory/
  mod.rs
  service.rs
  service_tests.rs
```

`UserWorkspaceMemoryService` 实现第 8 节 Reader 和 Writer port。它根据可信 effective workspace root 定位 `MEMORY.md`，保留 raw read 和 parse diagnostic，在 `talos_fs::with_file_lock` 内完成 read-modify-write，调用 `UserWorkspaceMemoryDocument` 执行业务转换和格式校验，校验 8,000-character 上限，并使用 `talos_fs::write_file` 写入。

`WorkspaceDocService` 将 `WorkspaceDocKind::Memory` 的读取和整文件写入委托给同一个 Service。其他 Workspace Doc 类型保持当前路径。公开 Workspace Doc API shape 不变。

模型可见 Tool 为：

```text
public name: UserWorkspaceMemoryUpdate
internal name: user_workspace_memory_update
visibility: AlwaysLoad
```

Tool 输入使用 operation-tagged 模型 DTO：`create_entry`、`update_entry`、`upsert_entry`、`replace_entry`、`remove_entry` 或 `replace_document`。除 `update_entry` 外，其字段直接复用领域对象；`update_entry` 为避免模型混用多种三态表示，统一采用“变更字段直接赋值 + `clear_fields` 显式清空可选字段”的扁平形式，并在 Tool 边界转换为 `UserWorkspaceMemoryEntryUpdate`。该 DTO 只解决模型 schema 易用性，不进入 Service、领域或 transport 公共 API。

Tool 只把转换后的 Update 分派给 Writer 的对应方法。它不暴露通用 `KnowledgeDocument` 或 Builder，并通过 `ToolContext` 获得 `UserWorkspaceMemoryWriter` 和现有可信 `MemoryScope`；文件解析、文档渲染、revision 和 scope 解析都不属于 Tool 职责。

该 Tool 必须使用 `AlwaysLoad`，因为主动形成记忆不能依赖用户输入的关键词路由；同时必须加入 `talos-tools-policy` 默认 allow-list 和对应策略测试。Tool 可见性和执行授权是两个独立条件。

## 11. Session 驱动的更新流程

主路径是普通模型 Tool call：

```text
User/assistant exchange
  -> primary model identifies durable information
  -> UserWorkspaceMemoryUpdate
  -> shared writer applies the typed operation
  -> subsequent model round receives the Tool result
```

主 Turn 成功后，仅当 Turn 输出中不存在成功的 `UserWorkspaceMemoryUpdate` Tool result 时，`AgentHarnessImpl` 执行一次有界 Memory reflection：

```text
successful Turn result
  -> bounded input: user text + final assistant text + current snapshot
  -> InnerModelClient with ModelUsageKind::Memory
  -> Noop or one UserWorkspaceMemoryUpdate value
  -> shared writer
```

Reflection 位于 Agent 层，因为 `AgentHarnessImpl` 已经持有 `InnerModelClient`；该逻辑不进入 `talos-agent-kernel` 或 `talos-service`。

以下场景不执行 Reflection：

- Turn 失败或取消；
- `WaitingUserAction` outcome；
- Session compaction；
- Daily Memory maintenance；
- Subagent 执行。

Reflection 输出为 `Noop` 或一个 `upsert_entry` / `remove_entry`。执行顺序固定为：主 Turn 成功结果确定后执行 reflection 和 Memory 写入；完成或 best-effort 失败后，才发送对外 `TurnCompleted` terminal event，并允许本 Session dispatch 下一任务。Writer、解析或 Inner Model 失败只记录 warning，terminal event 仍携带原成功结果。这样，用户观察到 Turn 完成时，同 scope 的其他 Session 下一 Turn 已能读取该次 Memory 结果。

## 12. Prompt 架构

### 12.1 静态策略区域

稳定的记录规则使用新的必需静态区域：

```text
kind: UserWorkspaceMemoryPolicy
stable_name: user_workspace_memory_policy
title: # User Workspace Memory Policy
placement: StaticPrefixRequired
cache_scope: StaticPrefixEligible
priority: High
required: true
```

渲染后的策略为：

```text
User Workspace Memory is scoped to the current agent and user and is shared
across their Sessions.

Use UserWorkspaceMemoryUpdate for explicit remember, correct, or forget requests
and for durable preferences, decisions, constraints, commitments, or lessons.
Do not record logs, transient status, recoverable repository facts, speculation,
secrets, or duplicates. Update by stable key and remove stale entries.
```

### 12.2 动态 Memory 区域

当前文件内容使用新的必需动态区域：

```text
kind: UserWorkspaceMemory
stable_name: user_workspace_memory
title: # User Workspace Memory
placement: DynamicSuffixRequired
cache_scope: DynamicOnly
priority: MediumHigh
required: true
envelope: XmlEnvelopeRequired
```

该区域位于 `PlanningInstruction` 之后、`RecentTaskContinuity` 之前。

`talos-prompt` 只定义 prompt-owned 的窄输入，不依赖 `talos-memory`：

```rust
pub struct UserWorkspaceMemoryPromptContext {
    pub file_state: UserWorkspaceMemoryPromptFileState,
    pub version_token: Option<String>,
    pub content: String,
}

pub enum UserWorkspaceMemoryPromptFileState {
    Missing,
    Valid,
    Invalid,
}
```

Agent/Agent Kernel 组装层通过 `UserWorkspaceMemoryReader` 取得 Snapshot，并转换为该 Prompt Context：Missing 使用内存空模板的 `prompt_view()`；Valid 使用解析文档的 `prompt_view()`；Invalid 使用受 8,000-character 上限约束的 raw content，保留修复上下文。`talos-prompt` 只负责区域编排和 XML envelope，不解析 Memory 业务文档。

动态 payload 为：

```text
file_state: <missing | valid | invalid>
version_token: <workspace-revision-or-none>

<projected or bounded raw MEMORY.md content>
```

`prompt_view()` 移除 YAML frontmatter、Talos marker comment、模板 comment 和多余空行，同时保留用户可见标题及 User Workspace Memory Entry 内容。payload 包含 version token，使模型能够发起带版本校验的 Replace。

`UserWorkspaceMemory` 是 required region，不加入 `DYNAMIC_SUFFIX_DROP_ORDER`。现有 8,000-character 文件上限是其成本硬边界。

删除 `BootstrapMemory`、`WorkspaceBootstrapFileKind::Memory`、旧 Memory default template 和 `should_auto_create` 中的 Memory 分支。`MEMORY.md` 不再经过 bootstrap 文件发现、自动创建或静态 section 构建。删除 active config 中的 `bootstrap.include_memory` 和已无消费者的 `memory.root_memory_filename`；required dynamic Memory 不受 `bootstrap.enabled`、`PromptMode` 或 include 开关控制，文件名固定为 `MEMORY.md`。配置删除属于显式 breaking change，不保留 ignored field 或 fallback。

因此，Memory 文件更新只改变 dynamic Prompt Context，不改变 `static_prefix_fingerprint` 或 `bootstrap_fingerprint`。Missing 状态不会产生磁盘写入；只有 User Workspace Memory Writer 可以创建或更新文件。

每次新的 Prompt assembly 都读取最新 effective user workspace 文件。已经运行中的模型请求不会被重写；其他 Session 在下一 Turn 读取到更新。

## 13. Daily Maintenance 调整

`MemoryDailyMaintenance` 只保留 Daily Memory 职责：

- 枚举有上限的 Session tail；
- 跳过 active Session；
- 使用 `ModelUsageKind::Dream` 形成 Daily Memory candidate；
- 通过现有 `DreamMemoryFormationStore` 写入 `memory/YYYY-MM-DD.md`；
- 推进现有 processing cursor。

删除以下 root-memory 行为：

- `refresh_managed_memory`；
- 仅供 root refresh 使用的 `recent_high` 聚合；
- 旧 root-memory Prompt 和 marker helper；
- 按用户执行的 root-memory refresh loop；
- `updated_users` 和 `failed_users` report field。

完成本次改造后，scheduler 和 compaction 路径都不得写入 `users/<user-id>/MEMORY.md`。

## 14. 影响范围

| 区域 | 必要调整 |
| --- | --- |
| `talos-knowledge-format` | 新增完整 OKF v0.2 producer/consumer facade，以及纯 managed document、Profile、Builder、Entry CRUD 和 projection |
| `talos-memory` | 增加 `workspace_memory/` 强类型业务对象、Profile、Document facade、策略和 port |
| `talos-prompt` | 增加 prompt-owned Memory Context 和动态 region；删除 Memory bootstrap variant、旧模板和自动创建路径；不依赖 `talos-memory` |
| `talos-config` | 删除 `bootstrap.include_memory` 和已无消费者的 `memory.root_memory_filename` active config 字段 |
| `talos-service` | 增加 `UserWorkspaceMemoryService`、raw/parse snapshot 和修复路径；委托 Workspace Doc Memory 访问 |
| `talos-tools` | 增加 update Tool；通过 Tool Registry 和 Tool Context 注入 Writer |
| `talos-tools-policy` | 将 `UserWorkspaceMemoryUpdate` 加入默认 allow-list 并增加策略测试 |
| `talos-agent-kernel` | 把业务 Snapshot 转换为 prompt-owned Memory Context，并保证 required dynamic 注入 |
| `talos-agent` | 使用现有 `InnerModelClient` 增加 terminal event 之前的成功 Turn reflection |
| `talos-base` | 创建一个 Service，并注入 Tool、Agent Harness 和 Workspace Doc Service |
| `MemoryDailyMaintenance` | 删除 User Workspace Memory refresh 和相关 report field |
| Prompt tests | 验证动态内容和稳定前缀缓存行为 |
| Agent integration tests | 验证直接 Tool 更新、reflection 和跨 Session 重新加载 |
| App Server E2E | 通过真实模型验证 Tool、reflection、跨 Session 读取及最终 Markdown |

Gateway、HTTP、JSON-RPC、SDK 和 Workspace Doc DTO shape 不变。本次存在明确 breaking change：删除 `bootstrap.include_memory` 和 `memory.root_memory_filename` 配置字段；Workspace Doc 对 `Memory` 的整文件写入从任意 Markdown 收紧为规范 User Workspace Memory OKF Replace；Rust 组装方必须为 `AgentHarnessParams` 提供 User Workspace Memory Reader/Writer，`MemoryDailyMaintenanceReport` 不再包含 root-memory user counters。实现和发布说明必须显式列出，不提供 compatibility fallback。

## 15. 验证策略

### 15.1 Knowledge Format 测试

- 使用 OKF v0.2 Appendix A 合法场景和一个 malformed-family 场景与官方 `okf-validator` 对照 conformance、diagnostic 和 fixable 结果，同时验证完整标准 family 的强类型 producer/consumer round-trip。
- 使用一个 Bundle 场景覆盖 Concept、链接、tag、version、Index/Log 和 v0.1 fallback；未知扩展只验证 opaque 保留，不测试任意 Value API。
- 使用一个完整 CRUD 场景覆盖 Builder 语义；针对 Review 暴露的 Profile 漂移、Section 结构、游离内容和跨 Section 顺序分别保留最小回归断言。

### 15.2 User Workspace Memory 领域测试

- 渲染精确的默认模板。
- 确认默认模板使用 OKF `status: stable`，允许零 Entry 并通过完整 OKF/Profile 校验。
- 解析一对合法 User Workspace Memory marker，并拒绝重复、倒序或缺少一侧的 marker。
- 验证强类型 Entry 与通用 Knowledge Entry 的双向转换。
- 验证 `OptionalField` 的 Unchanged、Set 和 Clear。
- 非规范文件、旧 marker 和损坏 marker 只返回 raw snapshot/diagnostic；Entry CRUD 返回 `InvalidDocument`。
- 拒绝重复 key、非法字段和超过 8,000 characters 的输出。
- 构建移除 frontmatter 和 comment、同时保留 Memory 内容的 Prompt view。

### 15.3 Service 测试

- 确认文件解析到 effective user workspace。
- 确认不同 effective workspace/user 隔离；显式共用 workspace root 时按配置共享。
- 确认 Missing read 返回内存模板但不创建文件，首次 Writer 操作才创建规范文件。
- 确认 Invalid snapshot 保留 raw content、mtime、raw revision 和 diagnostic，并可通过匹配 revision 的 Replace 修复。
- 确认并发 Entry 操作保留无关 User Workspace Memory Entry。
- 验证 create/update/upsert/replace/remove 的存在性与冲突语义。
- 拒绝 version 过期的 Replace。
- 证明写入复用共享文件锁，并在校验失败时保持旧文件不变。
- 证明 Workspace Doc Memory 的读写使用同一个 Service。
- 确认 Workspace Doc raw read 在文档损坏时仍可用，任意 Markdown write 被新 Profile 校验拒绝。

### 15.4 Prompt 测试

- 确认 User Workspace Memory 不再出现在 static prefix，而是出现在 dynamic suffix。
- 确认 Prompt assembly 不自动创建 `MEMORY.md`，也不读取旧 Memory bootstrap template。
- 确认 required dynamic region 不受 bootstrap enabled、PromptMode 或 include 开关控制。
- 确认 Missing、Valid、Invalid 分别投影空模板、prompt view 和有界 raw content。
- 确认 `talos-prompt` 只接收 `UserWorkspaceMemoryPromptContext`，不依赖 `talos-memory`。
- 确认修改 `MEMORY.md` 会改变动态内容，但不会改变 `static_prefix_fingerprint` 或 `bootstrap_fingerprint`。
- 确认该 region 为 required，不能被动态裁剪删除。
- 确认使用 effective user workspace 文件，而不是 Agent 根目录文件。

### 15.5 Agent 集成测试

- 确认 Tool 始终可见，并从 Tool Context 获得可信 scope。
- 确认 Tool 已被默认 Tool Policy 授权，移除授权时调用被明确拒绝。
- 复用现有反思测试验证仅 successful `EndTurn` 且没有成功 Memory Tool result 时触发，并只接受闭合的 `Noop | UpsertEntry | RemoveEntry` 决策。
- App Server Memory E2E 负责证明直接 Tool、成功 Tool 后无额外语义重复 Entry、reflection、terminal 后可见性和跨 Session 重新加载，不为相同链路复制第二套 Harness/Mock。

### 15.6 Maintenance 测试

- 确认 Daily Maintenance 继续写入 Daily Memory 并推进 cursor。
- 确认 Daily Maintenance 永远不会创建或修改 `users/<user-id>/MEMORY.md`。
- 确认 Maintenance report 不再暴露 root-memory user counter。

### 15.7 App Server E2E

- 通过真实模型显式记住稳定 key，并断言 timeline 中存在成功的 `UserWorkspaceMemoryUpdate` Tool result；读取 `MEMORY.md` 并使用生产 `UserWorkspaceMemoryDocument::parse` 校验 OKF frontmatter、唯一 marker、规范 section 和 Entry 字段。
- 新 Session 从动态 Memory 准确读取该值；成功 Tool 写入后整个文档不存在额外语义重复 Entry；同 key 更新后只保留一个 Entry、旧结论消失，后续 Session 只读取新结论。
- 显式忘记后 Entry 被删除，文件仍是可解析的规范空模板。
- 在主模型不调用 Memory Tool 的成功 Turn 中验证 reflection 写入，并在 terminal event 后立即读取；新 Session 可见，最后清理对应 Entry。
- Workspace Doc E2E 覆盖缺失文件、规范模板首次创建、raw revision CAS 冲突和格式校验。

## 16. 风险与不变量

1. 只有主模型没有更新 Memory 时，Turn-end reflection 才增加一次有界 Inner Model 调用。该调用是 best-effort，不改变主 Turn 结果。
2. User Workspace Memory 是必需的动态 Prompt item。现有 8,000-character 上限是成本硬边界，所有写入路径都必须强制执行。
3. Entry 更新在最新的已加锁 Document 上执行，以保留无关并发更新。整文档 Replace 不得绕过 Workspace revision 校验。
4. 只识别一种新 marker 格式：`talos:user-workspace-memory:*`。
5. Agent 根目录 `MEMORY.md` 始终位于 Runtime 路径之外，不得作为 fallback 加载。
6. Daily Memory 和 User Workspace Memory 不得共享自动化写入路径。
7. `talos-knowledge-format` 始终是无 I/O 的纯文档库；路径、锁、revision、原子写入和检索索引不得下沉到该 crate。
8. `talos-knowledge-format` 必须覆盖完整 OKF v0.2 facade；User Workspace Memory 只使用 Concept Document 能力，不得反向缩小公共标准层。
9. `okf-core` 和测试期 `okf-validator` 只有在通过 Talos Rust 工具链、许可证和 OKF 契约测试后才可锁定；上层 API 不得依赖其具体类型，也不得暴露任意 YAML/JSON Value。
10. 非规范现有 `MEMORY.md` 不自动迁移、不自动追加新 region；只有 raw read 和带 raw revision CAS 的整文档 Replace 可以修复。
11. `talos-prompt` 不得依赖 `talos-memory`，Prompt assembly 不得创建或写入 User Workspace Memory。
