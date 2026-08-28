# Dream Memory 切换为 OKF 文档

Status: implemented
Owner: agent
Created: 2026-08-27
Updated: 2026-08-28
Related: `refactor-039-memory-storage-retrieval`, `refactor-041-user-workspace-memory`, `feat-041-agent-inner-model`

## 1. 结论

Talos 将以下 Dream Memory 文件从自定义 Markdown block 格式切换为规范 OKF Concept Document：

```text
<effective-user-workspace>/memory/YYYY-MM-DD.md
```

每个日期文件是一个 `DreamMemory` Concept，并在 Talos managed region 中保存以 `memory_id` 为稳定 key 的强类型 Dream Memory Entry。

职责固定为：

```text
talos-knowledge-format  -> OKF/managed Markdown 的 parse、validate、mutation、render
talos-memory            -> Dream Memory Profile、Entry、批次幂等和业务转换
talos-store             -> 文件持久化、SQLite 派生索引及两者一致性
talos-service           -> scope、权限、CAS 和应用层编排
Maintenance/Kernel      -> 形成 MemoryWriteBatch，不接触 Markdown
```

“Knowledge Format 拥有完整 OKF 能力”指它拥有内容级 producer/consumer 语义，不表示它直接访问磁盘。路径解析、文件锁、Workspace revision、原子写入、SQLite 和 transport 不进入 `talos-knowledge-format`。

每个 Dream Memory 目录形成完整 OKF Bundle：日期 Concept 是事实源，`index.md` 是可重建导航，`log.md` 是按 revision 幂等追加的审计记录，SQLite 是可重建全文检索投影。不存在独立的 `log.yaml`；YAML 只作为 Markdown frontmatter。

## 2. 目标

1. 让 Dream Memory 的 Markdown 结构由统一 Knowledge Format 基础库生成和校验。
2. 为 Dream Memory 增加强类型业务文档，消除 Store 中手写 Markdown parser/renderer。
3. 统一 Compaction 与 Scheduler 的 Dream Memory formation 规则，并保持 `memory_search`、`memory_get` 和 SQLite 查询语义稳定。
4. 保持 `memory/YYYY-MM-DD.md` 为长期记忆唯一事实源，SQLite 继续作为可重建派生索引。
5. 保持批次幂等、source range 冲突、跨文件 `memory_id` 唯一性和手工编辑校验。
6. 复用现有 `talos-knowledge-format`、Dream Memory 持久化边界、`MemoryStore`、文件锁、原子写入和 revision 机制。
7. 对外提供简洁、强类型、无需猜测 Markdown/YAML 结构的业务 API。
8. 让列表读取实际消费 `index.md`，并把 `index.md`、`log.md` 与 SQLite 纳入同一 scope lock 和 stale-recovery 边界。

## 3. 非目标

1. 不改变 `ModelUsageKind::Dream` 或 processing cursor 的推进语义。
2. 不把文件系统、数据库或查询引擎放入 `talos-knowledge-format`。
3. 不把 OKF `index.md` 当作 SQLite 搜索索引。
4. 不把 OKF `log.md` 当作 Dream Memory 事实源、事务日志或 processing cursor。
5. 除补齐推荐的 `generated` producer metadata 外，不改变 User Workspace Memory 的 Entry、Tool、Reflection 或 Prompt 注入语义。
6. 不新增 scheduler、event bus、数据库表、通用 Repository 框架或新的存储 crate。
7. 不长期保留旧格式和 OKF 格式双写、双读或运行时 fallback。
8. 不实现跨日语义去重、自动过期、跨日移动或新的检索策略。

## 4. 当前实现事实

1. 当前 `MemoryDailyMaintenance` 读取未处理 Session tail，通过 `form_daily_session_memory` 形成 `MemoryWriteBatch`，并经现有 `DreamMemoryFormationStore` 写入；目标态统一重命名为 Dream Memory 术语。
2. `DreamMemoryFormationStore` 已接收强类型 `MemoryWriteBatch`，Maintenance 和 Kernel 不知道目标文件格式。
3. `MemoryStore::write_batch` 当前同时负责日期分区、稳定 ID 生成、旧 Markdown 解析、block 拼接、文件锁、原子写入和 SQLite 同步。
4. `memory/parser.rs` 通过 `## [HH:MM] title` 和裸 `key: value` 行解析 Dream Memory；该格式不是 OKF Concept Document。
5. `memory/YYYY-MM-DD.md` 是事实源；SQLite `memory_sources`/Memory records 是从文件同步或 rebuild 得到的派生状态。
6. `MemoryService::write_file_with_request` 接受用户提供的原始 Markdown，在文件锁内执行 revision 校验、source edit 校验、原子写入和重新索引。
7. `talos-knowledge-format` 已提供完整 OKF facade、`KnowledgeDocumentProfile`、managed Entry CRUD、规范渲染和 Prompt projection，并保持纯内存。
8. `OkfIndex` 已支持 parse/generate/render；`OkfLog` 已支持 parse/render。它们表达 OKF Bundle 导航和变更历史，不承担 Talos SQLite 查询索引职责。

## 5. 规范文件格式

日期由文件名确定，不重复写入自定义 frontmatter extension。规范模板为：

```markdown
---
type: DreamMemory
title: Dream Memory 2026-08-27
description: Durable memories formed for 2026-08-27.
tags:
  - talos
  - dream-memory
status: stable
generated:
  by: talos-agent/talos
  at: 2026-08-27T09:30:00Z
---

# Dream Memory 2026-08-27

<!-- talos:dream-memory:start -->

## Memories

<!-- talos:dream-memory:entry key="mem_0123456789abcdef0123456789abcdef" -->
### Example memory

- **Occurred At:** 1787761800
- **Session:** session-id
- **Kind:** fact
- **Importance:** normal
- **Source From Seq:** 10
- **Source Until Seq:** 24

Durable memory body.

<!-- talos:dream-memory:end -->
```

不变量：

- frontmatter 必须与 Dream Memory Profile 一致；
- `generated.by` 必须标识拥有该 Workspace 的 Agent，`generated.at` 在首次创建后保持不变；
- 文件名日期、标题日期和业务 `DreamMemoryDate` 必须一致；
- managed region 和 `Memories` section 必须唯一、完整且顺序固定；
- Entry key 必须等于规范 `memory_id`，在整个用户 scope 内唯一；
- Entry 字段均使用强类型对象解析，不向业务调用方暴露自由 YAML/JSON；
- `occurred_at`、source range、Kind 和 Importance 必须合法；
- Entry body、title 和单批 candidate 数继续执行现有硬上限；
- render 后的内容必须能由同一 Profile 无损回读。

## 6. Knowledge Format 边界

`talos-knowledge-format` 继续只负责纯内容：

```text
OkfDocument::{parse, validate, render}
OkfBundle::{builder, validate, resolve_links, diff}
OkfIndex::{builder, parse, generate, render, render_body}
OkfLog::{builder, parse_strict, append_entry, contains_text, render}
KnowledgeDocument::{parse, create/update/upsert/replace/remove/list, render}
```

`OkfIndexBuilder` 生成带当前 `okf_version` 的 root index；`OkfLogBuilder` 和 `append_entry` 生成规范日期分组并提供幂等追加。格式库只处理内存对象，不读取路径、不持有锁，也不决定 Dream Memory 的 revision 文案。

本次不得在 Knowledge Format 中新增以下对象：

- `Path`/Workspace resolver；
- async Reader/Writer；
- file lock、revision 或 CAS；
- SQLite Record/Store；
- Dream Memory、Session 或 User scope。

## 7. Dream Memory 业务模块

在现有 `talos-memory` 增加内聚模块：

```text
crates/cognition/memory/src/dream_memory/
  mod.rs
  bundle.rs
  entry_codec.rs
  error.rs
  profile.rs
  types.rs
  document.rs
  dream_memory_tests.rs
```

公共业务对象为：

```rust
pub struct DreamMemoryDate;

pub struct DreamMemoryDocumentContext {
    pub agent_id: MemoryAgentId,
    pub user_id: MemoryUserId,
    pub date: DreamMemoryDate,
    pub timezone: MemoryTimezone,
}

pub struct DreamMemorySourceRange {
    pub from_seq: u64,
    pub until_seq: u64,
}

pub enum DreamMemoryProvenance {
    Generated {
        session_id: MemorySessionId,
        source_range: DreamMemorySourceRange,
    },
    UserAuthored,
}

pub struct DreamMemoryEntry {
    pub memory_id: MemoryId,
    pub title: String,
    pub body: String,
    pub occurred_at: i64,
    pub kind: MemoryKind,
    pub importance: MemoryImportance,
    pub provenance: DreamMemoryProvenance,
}

pub struct DreamMemoryDocument;

pub enum DreamMemoryAppendResult {
    Appended,
    AlreadyApplied,
}

pub struct DreamMemoryBundleConcept;
pub struct DreamMemoryConceptRevision;
pub enum DreamMemoryBundleEventKind { Creation, Update, Recovery }
pub struct DreamMemoryBundle;
```

`MemoryId` 和 `DreamMemoryDate` 均为经过校验的强类型对象，不使用裸字符串表达稳定标识和日期。`DreamMemoryAppendResult` 只表达 Store 实际消费的幂等状态，不暴露无人使用的结果字段。

公共 API 为：

```text
DreamMemoryDocument::new(context, generated_at)
DreamMemoryDocument::parse(context, markdown)
document.append_batch(batch)
document.create_entry(entry)
document.entries()
document.render()
DreamMemoryBundle::reconcile(concepts, current_log, event_kind, logged_on)
DreamMemoryBundle::parse_index_dates(markdown)
bundle.render_index()
bundle.render_log() -> Option<String>
```

`create_entry` 服务一次性 legacy converter 对已解析 Entry 的规范化写入；通用 Markdown CRUD 继续由 `talos-knowledge-format` 提供。Dream Memory 不为未来可能的业务编辑提前复制 Query、replace 或 remove API，whole-file 编辑统一经过 Store 的强类型校验边界。

`DreamMemoryDocumentContext` 固定文档所属的 Agent、user、日期和时区；日期分区校验不得读取进程全局时区。首次创建使用形成批次时间写入完整 `generated`，后续 parse/mutation 保留原值。`DreamMemoryProvenance` 通过 `Generated | UserAuthored` 排除不完整组合：新形成的 `MemoryWriteBatch` 必须产生完整 provenance；一次性转换允许把无法恢复 provenance 的 legacy block 表达为 `UserAuthored`，但不得伪造 Session 或 seq。

`DreamMemoryDocument` 内部封装 `KnowledgeDocument`，负责：

- Dream Memory Profile；
- `MemoryWriteBatch` 到稳定 `DreamMemoryEntry` 的转换；
- 当前稳定 `memory_id` 生成算法；
- 同一 Session source range 的幂等和重叠检查；
- Entry 与 managed Markdown body 的双向转换；
- 文件日期和 Entry `occurred_at` 的时区归属校验；
- 业务硬限制。

它不执行文件 I/O 或 SQLite 操作。

## 8. 形成策略与同日 Consolidation

Compaction 与 Scheduler 必须使用同一份 Dream Memory system prompt。两条路径只通过强类型 `mode` 区分响应：

```text
compaction  -> Session summary + MemoryCandidate[]
maintenance -> MemoryCandidate[]
```

统一规则借鉴成熟 Agent 的记忆筛选原则，但转换为 Talos 语义：只记录可能改变未来 Session 行为、且无法从当前权威源可靠恢复的事实、偏好、决策、承诺和教训；排除代码结构、路径、Git 历史、调试流水、临时进度、推测、秘密和重复结论。User Workspace Memory 保存当前规范结论，Dream Memory 保存带日期和来源的历史证据，两者不得互相代写。

每次形成在模型调用前读取 `occurred_at` 所属日期的现有 Dream Memory 强类型快照，并作为有界上下文传给模型。模型应在当前 transcript 内归并重复信号，并抑制当天已经表达同一结论的候选；补充信息只输出真正新增部分，矛盾信息作为新的日期证据保留。形成过程不读取、比较或改写其他日期文件。

模型输出仍是强类型结构，空候选优于不确定候选。模型不获得 Markdown、文件路径或 mutation API，也不负责删除或改写已有 Entry。

## 9. Store 与 Service 边界

现有 Dream Memory 持久化 port 增加形成所需的同日快照和 revision 语义：

```text
DreamMemoryFormationStore::read_day(scope, DreamMemoryDate) -> DreamMemoryDaySnapshot
DreamMemoryFormationStore::write(scope, DreamMemoryWriteRequest)
```

`DreamMemoryDaySnapshot` 只包含强类型日期、revision 和 `DreamMemoryEntry`；`DreamMemoryWriteRequest` 绑定快照日期、base revision 与 `MemoryWriteBatch`。形成方不读取 Markdown，也不猜测 revision 结构。

`MemoryStore::write_batch` 调整为持久化编排：

```text
resolve YYYY-MM-DD.md
  -> acquire existing effective-workspace memory mutation lock
  -> read latest content
  -> compare expected day revision
  -> DreamMemoryDocument::parse or new
  -> append_batch
  -> render canonical OKF
  -> validate cross-file memory_id uniqueness
  -> atomic write
  -> regenerate index.md and append the unseen Concept revision to log.md
  -> synchronize derived SQLite index
```

scope mutation lock 从可信 effective workspace 派生，并复用 `talos_fs::with_file_lock`；它串行化同一 Workspace 内的 append、raw edit、conversion 和 rebuild，不能只锁目标日期文件。索引身份固定为 `(agent_id, user_id, effective_workspace)`，所有 rebuild、search、get 和 write 必须使用完整三元 scope。这样跨日期 `memory_id` 唯一性和 source range 冲突检查与最终写入处于同一临界区，不新增 manager 或事务框架。

Store 不再保留独立的 Markdown heading parser、metadata line parser 或 `render_record`。SQLite Record 由 `DreamMemoryEntry` 显式转换，不直接解析 Markdown 字符串。

`MemoryService::write_file_with_request` 继续接受人可编辑的 Markdown wire content，但写入前必须通过 `DreamMemoryDocument::parse(date, content)`：

```text
resolve trusted scope/date
  -> acquire effective-workspace memory mutation lock
  -> read latest raw content
  -> verify expected revision
  -> parse and validate canonical DreamMemoryDocument
  -> validate cross-file memory_id/source-range constraints
  -> atomic write
  -> synchronize SQLite index
```

应用层对外 DTO shape 保持不变。Raw Markdown 是文件 API 的 transport 内容，不是内部业务对象。

手工编辑继续保持现有 provenance 保护语义：

- whole-file API 中已存在 Entry 可以修改 title、body、kind 和 importance，也可以删除；
- 已存在 generated Entry 的 `memory_id`、`occurred_at`、session 和 source range 不可修改；
- whole-file API 中 UserAuthored Entry 的 key 变化按“删除旧 Entry + 创建新 Entry”处理；
- 手工新增 Entry 只允许无 provenance 的用户记录；带 provenance 的新 Entry 必须经 `DreamMemoryFormationStore` 形成；
- cross-file `memory_id` 唯一性和 source range 冲突仍在 scope lock 内检查。

Service 只完成 actor scope、权限和 Workspace revision 校验，并把已观察到的精确旧内容交给 `MemoryStore::replace_source`。Store 在 effective-workspace lock 内重新核对旧内容，再完成 parse、校验、文件提交和索引同步；Service 不直接持有锁或写 Dream Memory 文件。

Markdown 写入成功后它即成为事实源。Store 在提交文件前创建 `<effective-user-workspace>/.dream-memory-index-stale`，只有 `index.md`、`log.md` 和 SQLite 全部同步成功后才删除；若任一步失败，该 marker 保留。后续 list、`memory_search`、`memory_get` 或显式 rebuild 在同一 scope lock 下从日期 Concept 恢复 Bundle 与 SQLite。不得回滚或覆盖已经成功提交的规范 Markdown，也不得让派生材料被当作事实源。

## 10. SQLite、Index 与 Log

三种能力必须保持独立：

1. `memory/YYYY-MM-DD.md`：唯一事实源，使用 OKF Concept + managed Entry。
2. SQLite Memory Index：可重建派生状态，服务 `memory_search`/`memory_get`。
3. `memory/index.md`：由全部有效日期 Concept 确定性重建；每个日期条目使用最多三个 Entry 标题形成有界的一行内容摘要，文件列表 API 从该导航读取日期，再读取目标 Concept 形成摘要。
4. `memory/log.md`：按实际变更发生日期记录 Concept revision 首次进入 Bundle 的 `Creation | Update | Recovery` 事件，并复用同一内容摘要；同一 revision 只记录一次，历史 revision 保留。

`index.md` 不参与全文检索，`log.md` 不参与恢复业务 Entry。日期 Concept 提交后若进程中断，stale recovery 根据当前 Concept fingerprint 补 `Recovery` 事件、重建 index 和 SQLite；损坏的派生 `log.md` 在 Recovery 中由当前日期 Concept revision 重建，不得永久阻断 list/search/get/write。因此不新增自由 JSON journal 或第二套事务框架。

摘要只从已校验的 Entry 标题确定性生成：最多展示三个标题，单个标题有字符上限，剩余数量使用 `and N more` 表达。Index 与 Log 不各自生成摘要，不新增模型调用或第二份可漂移状态。

空 scope 仍生成带 `okf_version` 的空 `index.md`，使列表读取拥有稳定入口；没有 Concept revision 时不生成空 `log.md`。`DreamMemoryBundle::render_log` 使用 `Option<String>` 明确表达该状态，Store 不重复判断日志是否应存在。

## 11. 切换策略

本次采用目标态切换，不长期保留 legacy parser：

1. 发布前使用一次性转换工具完整读取一个用户 scope 的 legacy `YYYY-MM-DD.md`；
2. 转换器复用当前 legacy parser，并保留已有 `memory_id`、时间、kind、importance 和完整的 Session/source range；无完整 generated metadata 的普通 heading block 转换为 `UserAuthored` 用户记录；legacy 中只有 Session、没有 source range 的 partial provenance 不进入规范格式；旧 Runtime 视为零 Entry 的空文件或无二级标题文件转换为合法的空 Dream Memory Concept；
3. 在 sibling staging 目录生成全部 OKF 文件，使用对应 `DreamMemoryDocumentContext` 校验，并记录源文件 revision manifest；
4. 获取 user-scope memory mutation lock 后重新核对全部 source revision；不一致则放弃 staging，原目录不变；
5. 通过目录级 rename 将原 `memory/` 切换为 backup、staging 切换为新 `memory/`；第二次 rename 失败时立即恢复 backup；若进程在两次 rename 之间崩溃，下次启动先从 backup 恢复并重新转换，禁止暴露 mixed legacy/OKF scope；
6. 切换后从 OKF 文件 rebuild SQLite；成功后删除 backup，失败则保留规范 Markdown 和 backup，并将 scope 标记为需要 rebuild；
7. Runtime 只加载规范 OKF Dream Memory，不保留 legacy 自动 fallback 或双写；
8. 转换工具和 rollout 只服务一次性切换，不进入长期公共 API。converter 模块和入口必须带英文移除说明及 `TODO(dream-memory-okf): Remove this one-time legacy converter in the next release.`，且正常 Runtime 读写不得依赖 converter。

如果产品确认当前环境无需保留既有 Dream Memory，可以删除一次性转换工具，但必须在发布说明中明确这是数据格式 breaking change。

## 12. 调整范围

| 模块 | 调整 |
| --- | --- |
| `talos-knowledge-format` | 仅补 Dream Memory 实际依赖且当前缺失的纯格式 API；不增加 I/O |
| `talos-memory` | 增加 `dream_memory/` 强类型业务文档、Bundle 和转换 |
| `talos-store::memory` | 删除自定义 parser/renderer，统一维护日期 Concept、index、log 和 SQLite |
| `talos-service::memory` | Raw file write 通过 Dream document 校验；API shape 不变；隔离一次性 converter |
| Dream formation | Compaction 与 Scheduler 使用同一 Prompt 和同日快照；Scheduler 按 Session `agent_id` 使用已加载的 Agent Kernel 配置；`MemoryDailyMaintenance`、`form_daily_session_memory` 及对应模块/测试统一为 Dream Memory 术语 |
| Automation config | `memory_daily_maintenance` 直接重命名为 `dream_memory_maintenance`，不保留配置 alias |
| List/Search/Get | List 消费 `index.md`；Search/Get SQL 行为保持稳定，Record 来自强类型 Entry |
| E2E | 更新现有 Dream Memory case，校验完整 Bundle 和 rebuild/list/search/get |

不需要修改 Agent Tool、User Workspace Memory、Gateway DTO、SDK 或前端。日期分区文件名和 Memory file API 的 `date` 语义保持不变。

## 13. 验证策略

### 13.1 Knowledge Format

- 复用现有官方 OKF conformance oracle，不复制另一套标准测试。
- 仅当 Dream Memory 使用缺失 API 时，补一个对应的 Index/Log 或 managed document producer 场景。

### 13.2 Dream Memory Domain

- 一个完整 round-trip 场景覆盖模板、append batch、parse 和完整 Entry 对象。
- 在同一场景覆盖 batch 幂等、重叠 source range、非法日期/字段和稳定 ID。
- 使用完整对象相等验证 Entry，不逐字段复制断言。

### 13.3 Store/Service

- 改造现有 `MemoryStore` 测试，不复制测试 Harness。
- 验证 append 后日期 Concept、index、log 和 SQLite 一致，重复批次不重复追加 log revision。
- 验证手工 write 的 CAS、非法 OKF 拒绝和写入失败不改变旧文件/索引。
- 验证 rebuild 从 OKF 文件恢复同一 Search/Get 结果。
- 验证两个日期文件并发写入时，跨文件 `memory_id` 和 source range 不变量由 user-scope lock 保证。
- 验证 Markdown 已提交但派生材料同步失败时，后续 rebuild 恢复 Bundle 与 SQLite。
- 验证形成方可读取同日强类型快照，且 stale revision 写入返回冲突并保持文件、索引和 cursor 不变。

### 13.4 E2E

- 优化已有 Dream Memory E2E，不新增相似场景。
- 通过现有文件 API 写入规范 Dream Memory，检查 `generated`、`index.md`、`log.md`，并由生产 parser 回读。
- 删除派生 SQLite、重启执行 startup rebuild 后，验证 `memory_search` 和 `memory_get` 返回同一稳定 `memory_id`。
- Dream formation 使用现有 Service/Kernel 集成测试驱动真实 `MemoryWriteBatch` 链路并用生产 parser 回读；不为等待 cron 和外部模型输出新增不确定的 E2E。
- 在现有 Kernel 场景验证 Compaction 与 Scheduler 使用同一 system prompt、各自的强类型 mode，并包含有界同日上下文。

### 13.5 一次性转换

- 复用现有 legacy fixture，覆盖 generated block 和无 provenance 的普通 heading block。
- 验证转换前后 Entry 数量、稳定 `memory_id`、规范字段和完整 generated provenance 一致；只有 Session 的 partial provenance 按已声明 breaking change 转为 `UserAuthored`。
- 验证任一解析失败、revision 变化或目录切换失败时，不会暴露 mixed legacy/OKF scope。

## 14. Breaking Change

1. `memory/YYYY-MM-DD.md` 从自定义 block 变为规范 OKF Concept Document。
2. 手工文件写入只接受规范 Dream Memory OKF，不再接受任意旧 Markdown。
3. Runtime 不再读取 legacy Dream Memory；既有数据必须在切换前完成一次性转换或明确放弃。
4. `DreamMemoryFormationStore::write` 使用绑定 day revision 的强类型 request；Memory file API 与 Search/Get DTO 不变，SQLite 派生索引增加 `agent_id`、`user_id` 和 `workspace_root` 三元作用域并通过独立 migration 重建。
5. legacy 普通 block 中只有 Session、没有 source range 的 partial provenance 不进入规范格式；Entry 及稳定 ID 保留，但 Search/Get 不再返回该不完整 Session attribution。
6. Automation 配置键从 `memory_daily_maintenance` 直接重命名为 `dream_memory_maintenance`；配置使用 `deny_unknown_fields`，旧键会明确加载失败，不提供 alias 或 fallback。
7. Rust 内部公开名称 `MemoryDailyMaintenance`、`form_daily_session_memory` 及对应模块统一重命名为 Dream Memory 术语；旧名称不保留 alias。
8. `OkfFrontmatterBuilder::legacy_timestamp` producer API 被删除；OKF v0.1 timestamp 仅保留 permissive consumer/conformance 读取能力。

## 15. 风险与不变量

1. Markdown 始终是事实源，SQLite 始终可重建。
2. Format/Domain 校验成功之前不得修改文件；文件写入成功之前不得提交新的索引状态。
3. 一次写入必须在同一个 effective-workspace memory mutation lock 内基于最新内容执行 parse、mutation、跨文件校验、render 和 write；索引查询必须额外匹配完整三元 scope。
4. 不得同时保留 Store 自定义 parser 与 `DreamMemoryDocument` 两条生产路径。
5. 不得为了复用而把 User Workspace Memory 和 Dream Memory 合并为同一个业务对象；两者只共享 Knowledge Format 基础能力。
6. 不得自动生成无人消费的 `index.md`/`log.md`，也不得用它们替代 SQLite。
7. 不得暴露任意 YAML/JSON Value、未校验 Markdown fragment 或裸日期/ID 给业务调用方。
8. Compaction 与 Scheduler 不得复制形成 Prompt；同日上下文必须有硬上限，跨日文件不得进入形成输入。
9. stale day revision 必须显式冲突，形成写入失败时不得推进 processing cursor。
