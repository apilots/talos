# Long-term Memory 存储与检索简化设计

Status: archived
Owner: agent
Created: 2026-07-17
Updated: 2026-07-21

## 1. 结论

本设计只保留五个核心事实：

1. 用户个人空间下的 `MEMORY.md` 是每轮完整加载的少量核心记忆，硬上限为 8,000 chars。
2. 用户个人空间下的 `memory/YYYY-MM-DD.md` 是长期记忆的唯一事实源。
3. Talos data dir 下的 `memory-index.sqlite` 是可丢弃、可从 daily Markdown 重建的 FTS5 索引。
4. `MemoryService` 对外提供确定性的 search/get；Tool、Gateway、SDK 和 UI 复用同一组 DTO。
5. 记忆形成复用现有 compaction、automation、Session artifact 和 timeline delta，不新增 Agent Search 框架、任务框架或 compaction 协议。

```text
user workspace/MEMORY.md ------------------------> prompt bootstrap

user workspace/memory/YYYY-MM-DD.md
             canonical
                 |
                 v
data dir/memory-index.sqlite ---> search ---> get(memory_id)
             derived
```

不引入向量库、图数据库、Memory JSONL、每用户 SQLite、运行时文件 watcher、隐藏 LLM 检索或第二套 Session 状态。

## 2. 要解决的问题

旧实现参考 PageIndex，在每次 search 时扫描和 hash Markdown，再串行调用 LLM 选择文件和 section。该方案适合大型稳定文档，不适合持续追加、按条目读取的 daily memory。

同时，旧 `Memory` 模块混入了 Session turn、transcript、summary、prompt context、close flush 和长期记忆检索，导致职责、写入时机和数据真相不清晰。

本次目标：

- search/get 不调用 LLM，不扫描全部 Markdown。
- 已有 Markdown 不丢失；SQLite 损坏后可以重建。
- Memory API 使用稳定 ID、强类型 Request/Response 和明确 user scope。
- compaction 只轻量提取重点；session close 不再刷长期记忆。
- `MEMORY.md` 与 daily memory 进入上下文的方式明确且有硬上限。
- TUI、Web 和 Desktop 使用同一 Session timeline delta 展示 compaction，并阻止同 Session 的新 input/command。

非目标：

- embedding、vector search、knowledge graph、LLM rerank。
- Memory 专用 agent loop 或 search agent。
- 每用户 cron、分布式 lease、任务管理 UI。
- 新 SessionBusy/Compacting 协议或全局 Session 锁。
- 为已删除 API 保留 alias、fallback、双写或反向测试。

## 3. 模块边界

| 模块 | 唯一职责 | 不应承载 |
| --- | --- | --- |
| `talos-memory` | `MemoryReader`、`DreamMemoryFormationStore`、`MemoryScope`、`MemoryCandidate`、`MemoryWriteBatch` | SQL、filesystem、LLM、Session 状态、transport |
| `talos-store::memory` | daily Markdown 解析/写入、SQLite schema、sync/rebuild、record search/get | actor scope、Agent 判断、公开 DTO |
| `talos-service::memory` | 注入 user/workspace scope，提供文件 API 与 search/get，编排 daily maintenance | Prompt 构建、Session event、Memory DB Record 外泄 |
| `talos-store::session` | turn/transcript/summary artifact、真实 seq、memory processing cursor | daily Markdown、FTS5、长期记忆 API |
| `talos-agent-kernel` | Session prompt、现有 Tool Loop、compaction/daily 候选形成、cursor 编排 | Memory SQL、Markdown 路径、transport |
| `talos-tools` | 将 `memory_search`/`memory_get` 连接到 `MemoryReader` | 排序、scope 解析、文件扫描、专用 agent loop |
| `talos-automation` | 定时调用 daily maintenance | 记忆提取、文件写入、第二套任务状态机 |
| `talos-base` | 创建 Store/Service 并注入现有调用方 | Memory/Session 业务逻辑 |

依赖方向固定为：

```text
talos-base
  +--> SessionStore / SessionArtifactStore --> Agent / Service
  `--> MemoryStore --> MemoryService
                         +--> MemoryReader --> Tools
                         +--> search/get --> Gateway / SDK / UI
                         +--> DreamMemoryFormationStore --> Agent Kernel
                         `--> daily maintenance --> Automation
```

`MemoryRecord` 只属于 store 边界，不进入协议、SDK 或 UI。Kernel 只依赖窄端口和 Session port，不依赖 concrete `MemoryStore` 或 `MemoryService`。

### 3.1 从旧 Memory 迁出的能力

- turn、transcript、summary、seq、reset：迁到 `talos-store::session`。
- Session prompt context：迁到 `talos-agent-kernel` 与 `talos-prompt`。
- daily Markdown 与 SQLite：迁到 `talos-store::memory`。
- actor scope、文件 API、search/get：归 `MemoryService`。
- 定时调用：归现有 automation scheduler。
- PageIndex tree search、两次检索 LLM、session-close flush、`MemoryFlushManager`、旧 Memory 大接口：删除。

迁移使用 move-and-delete，不保留 forwarding adapter、旧签名或新旧双写。

## 4. 数据真相

### 4.1 `MEMORY.md`

- 路径由用户 workspace 配置解析，不写入公共 storage connection，也不存到全局共享 workspace。
- 保存每轮都需要知道的少量用户事实、偏好、长期决策和未完成事项。
- 在 prompt bootstrap 中优先、完整加载，不参与 FTS search，避免重复注入。
- 文件总长度最多 8,000 chars；Workspace Doc API 和自动更新在写入前校验，Prompt build 发现已有超限文件时明确失败，不静默截断。
- 用户内容保留；自动内容只修改一对 managed markers 内的内容：

```markdown
<!-- talos:managed-memory:start -->
- Durable managed memory.
<!-- talos:managed-memory:end -->
```

marker 缺失时可在文件末尾创建；单边、重复或嵌套 marker 明确失败。

### 4.2 Daily Markdown

`memory/YYYY-MM-DD.md` 是长期记忆唯一事实源。自动写入条目固定为：

```markdown
## [09:30] User preferences
memory_id: mem_0123456789abcdef0123456789abcdef
occurred_at: 1784251800
session: session-id
kind: preference
importance: high
source_from_seq: 12
source_until_seq: 18

- The user prefers concise architecture proposals.
```

固定枚举：

```rust
pub enum MemoryKind {
    Preference,
    Fact,
    Decision,
    Task,
    Episode,
}

pub enum MemoryImportance {
    Low,
    Normal,
    High,
}
```

`kind` 和 `importance` 由 compaction/daily 的结构化 LLM 输出产生，不在入库时二次猜测。Store 只校验枚举和值上限。

升级前的 Markdown 原文保留并可索引：

- 没有 v1 reserved metadata 的旧 heading 按 legacy block 解析，并生成确定性 ID。
- 只要出现任一 v1 reserved field，就必须满足完整 v1 schema；部分 metadata 明确失败，不降级为 legacy。
- 新自动记忆只写 v1 格式，不批量重写旧文件。

### 4.3 SQLite 派生索引

单个 `memory-index.sqlite` 保存全部 user scope，所有查询必须带 `user_id`。核心表只有：

- `memory_entries`：结构化 record。
- `memory_entries_fts`：FTS5 external-content 表。
- `memory_sources`：`user_id + source_path + fingerprint`，用于变更 source 对账。

SQLite 不保存 Markdown 副本，也不是长期记忆事实源。

同步规则：

- 通过 daily file API 写入：Markdown 成功后立即在同一调用中 sync 对应 source。
- 自动 candidate write：先在文件锁内更新 Markdown，再 sync 对应 source。
- Talos 启动：枚举已知用户 workspace，按 source fingerprint 对账；新增、修改和删除的 source 更新索引。
- 用户绕过 API 直接编辑文件：不新增运行时 watcher；下一次 Talos 启动时识别变更。

损坏恢复保持简单：启动时执行 SQLite quick check；确认损坏后只删除派生 DB/WAL/SHM，创建空索引，并在服务对外可用前从 daily Markdown 重建。重建中断时下次启动继续按 Markdown 对账。无需临时 DB manager、`.corrupt-*` 归档或运行时恢复状态机。

## 5. 确定性 API

公共 DTO 只有一套，Service、Gateway、SDK、Tool 和 UI 复用：

```rust
pub struct MemorySearchRequest {
    pub query: String,
    pub limit: Option<u32>,
    pub kinds: Option<Vec<MemoryKind>>,
    pub since_at: Option<i64>,
    pub until_at: Option<i64>,
}

pub struct MemorySearchResponse {
    pub data: Vec<MemorySearchHit>,
}

pub struct MemoryGetRequest {
    pub memory_id: String,
}

pub struct MemoryGetResponse {
    pub memory: MemoryInfo,
}
```

`MemorySearchHit` 返回稳定 `memory_id`、title、preview、kind、importance、occurred_at、rank 和 source；`MemoryInfo` 在此基础上返回完整 body。`MemorySourceInfo` 只暴露 date、可选 session_id 和 source seq range，不暴露 SQLite row、绝对路径或 Markdown 行号。

内部端口固定为：

```rust
MemoryReader::search(scope, request) -> MemorySearchResponse
MemoryReader::get(scope, request) -> MemoryGetResponse
DreamMemoryFormationStore::write(scope, batch) -> ()
```

`MemoryScope { agent_id, user_id, workspace_root }` 由 Runtime/Service 注入，模型和公共请求不能覆盖。

Transport 只做 envelope 差异：

- App Server：`memory/search`、`memory/get`。
- HTTP：`POST /v1/memory/search`、`GET /v1/memory/entries/{memory_id}`。
- Rust/TypeScript SDK 与 Web/Desktop 使用相同 DTO。

## 6. 写入流程

### 6.1 Session artifact 与候选形成

```text
Turn execution
  -> append persisted Session artifact and obtain real seq
  -> emit existing timeline terminal with artifact_seq
  -> compaction or daily maintenance reads a bounded contiguous artifact window
  -> one structured LLM response returns up to 8 MemoryCandidate values
  -> DreamMemoryFormationStore writes one idempotent MemoryWriteBatch
  -> Kernel advances memory_processed_until_seq with existing SessionStore CAS
```

约束：

- artifact 是 turn seq 的事实源；daily maintenance 使用 artifact `latest_seq` 判断是否有 tail，不以 Session DB 的 `latest_turn_seq` 镜像作为正确性前提。
- `MemoryWriteBatch` 的稳定 ID 由 user、session、source range 和 candidate ordinal 生成；同一 batch 重试为 no-op，相同 ID 不同内容返回 conflict。
- 写入顺序固定为 Markdown 后 SQLite。SQLite 失败时 Markdown 仍保留；同 range retry 或下次启动补齐索引后再推进 cursor。
- 同一 session source range 已存在于 Markdown 时，以 canonical Markdown 为准重新同步索引；不再比较重试时重新生成的 candidate 内容。
- 空 candidates 合法：不写文件，仍可推进已处理 cursor，避免重复调用 LLM。
- `DreamMemoryFormationStore` 不持有 SessionStore；Kernel 在 writer 成功后单独推进 cursor，避免 Memory 反向依赖 Session。
- 不使用进程级全局 Memory mutex。相同 daily 文件由既有 file lock 串行；SQLite 由 transaction 保证 source sync。

### 6.2 Compaction、daily 与 close

- Compaction 在同一次 summary LLM 输出中附带少量 candidates，不新增第二次 LLM。
- 只有 compaction coverage 与 memory cursor 连续时才直接提交 candidates；cursor 已由 daily 推进时不强行合并重叠 range，剩余 tail 交给 daily。
- Daily maintenance 是一个全局 automation job，不为每用户创建 job；每次按配置限制 Session 数量和每个 Session 的 artifact window。
- Daily 对有 artifact tail 的 Session 形成 candidates，并从当前 bounded user set 的 High memory 更新 `MEMORY.md` managed section。
- Session close 不调用 LLM、不写 daily memory；未处理 tail 留给 daily job。
- Session clear 保留 seq 单调性，并把 memory cursor 对齐到 clear 前的 artifact latest；长期 Markdown/SQLite 不属于 Session clear 的删除范围。

## 7. Search、Get 与 Agentic Search

```text
Tool or UI
  -> MemoryService injects user scope and validates request
  -> MemoryStore executes bounded SQLite query
  -> Service converts MemoryRecord to shared DTO
  -> Tool or transport returns the same response shape
```

Search 规则：

- query 1–512 chars，limit 1–20。
- 3 chars 及以上使用 FTS5 trigram + BM25；title 权重 2.0，body 权重 1.0。
- importance 对 BM25 score 使用固定系数：High 1.2、Normal 1.0、Low 0.8。
- 小于 3 chars 使用有 limit 的 `instr` 查询，不回退扫描 Markdown。
- 相同相关度按 `occurred_at DESC, memory_id ASC` 稳定排序。
- search 只返回 bounded preview；get 按 `user_id + memory_id` 返回完整 `MemoryInfo`。

Agentic Search 不新增框架：现有 Tool Loop 可以根据结果调用一次或多次 `memory_search`，再对候选调用 `memory_get`，最后基于完整 tool result 回答。搜索策略属于 Agent 推理，不进入 MemoryService，也不改变确定性 API。

## 8. 参与 Context Build

Context 来源分为两类：

1. `MEMORY.md`：静态核心记忆，每轮作为 bootstrap system section 优先完整加载。
2. Daily memory：动态按需检索，不自动全量注入。

`memory_search` 和 `memory_get` 都是普通 tool call：

- search preview 只作为当前 tool result 参与下一次模型调用，不进入长期 Prompt section。
- get 的完整 `MemoryGetResponse` 先作为普通 tool result 参与下一次模型调用。
- 普通 working loop 不同时复制一份 retrieved-memory system message，避免重复内容。
- 如果 working-context trim 后真实移除了该 MemoryGet tool exchange，Kernel 从被移除的强类型 result 恢复同一 `MemoryInfo`，再通过既有 `PromptSectionKind::RetrievedMemories` 注入一次。
- `RetrievedMemories` 与 working-context 使用同一 item/char hard cap，并计入 prompt budget；不定义第二套 retrieved-memory DTO。

Prompt 顺序保持稳定：

```text
system/static:
  identity and policy
  MEMORY.md
  other bounded bootstrap files

system/dynamic:
  compact summary
  recent Session context

messages:
  current user/tool/assistant working loop

system/recovery:
  retrieved memories only after their tool exchange was trimmed
```

`system/recovery` 仅在发生真实 trim 后追加到当次模型输入末尾；它仍是受 hard cap 和 prompt budget 约束的 system message，不重写历史。

## 9. Compaction delta 与跨端行为

不新增 `Compacting` 或 `SessionBusy` 协议。复用现有 timeline item/delta：

- kind：`ContextCompaction`
- trigger：自动阈值或 `UserRequested`
- stage：`Preparing`、`Summarizing`、`RebuildingPrompt`、`Trimming`、`Completed`、`Failed`
- outcome：`Applied`、`NoChanges`、既有失败/trim outcome

显式 `/compact` 的 `request_id`、`turn_id`、`message_id` 使用同一 operation suffix 生成，保持可关联但类型前缀明确。Compaction terminal 使用真实事件序列，不生成假的 artifact seq。

服务端使用既有 Session 级 active reservation：

- 同 Session 已有 turn/compaction 时，新 input 或 command 返回现有 `ActiveTurnRunning`。
- 不进入 Session 内部排队，不影响其他 Session 并发。
- abort handle 在首个可见 compaction delta 前建立，立即 Stop 不返回 RunNotFound。

客户端只维护一条 Session timeline：

- Web、Desktop 和 TUI 都从该链路显示 `Context compacting...`。
- running compaction 时新的 input 和业务 command 被阻止，草稿保留并显示提示；现有 Stop/Esc 仍可取消当前 compaction。
- 断线后使用最新 event cursor replay/reconnect；replay 必须恢复仍 Running 的 compaction item。
- 服务端 conflict 是最终保护，客户端 busy 不是第二个事实源。

Compaction summary 解析失败允许唯一例外：使用已有 deterministic trim recovery，避免超限 Session 永久不可用。该路径不能保存无效 summary、写 candidates 或推进 memory cursor，并必须返回明确 trim outcome。其他 provider、存储或内部错误不使用 fallback。

## 10. 错误与并发语义

- scope 缺失、非法 query、超限 `MEMORY.md`、损坏 metadata、重复 ID：明确失败。
- search/get 不触发 rebuild，不扫描 Markdown，也不吞掉 index 错误。
- candidate conflict 不覆盖 canonical Markdown，不推进 cursor。
- daily 单个 Session/用户失败不阻止其他 bounded item；失败 cursor 留待下一次 automation。
- 文件并发依赖现有 path lock、原子文件写和 version token；SQLite source sync 使用 transaction。
- Session cursor 只允许单调 CAS；旧 Session record 更新不得回退已推进 cursor。

## 11. 配置

只保留当前行为需要的配置：

- `memory.memory_dirname`
- memory file list limits
- `automation.dream_memory_maintenance.{cron,batch_size,max_batches}`
- 现有 compaction source/context hard caps

删除旧 PageIndex、close flush、tool enable、search source switch 和 session-memory store 配置。SQLite 路径从 Talos data dir 派生，不复用或改写 `storage.connection_string`；Markdown 始终位于用户个人 workspace。

## 12. Breaking Change

- 旧 path/from/lines `memory_get` 改为稳定 `memory_id`。
- `MemorySearchResponse`、`MemoryGetResponse`、Memory Tool schema 和 SDK 类型统一为共享 DTO。
- 旧 `Memory` 大接口、PageIndex 类型、flush manager 和 close-flush E2E 删除，不保留 alias。
- App Server/HTTP schema、生成的 TypeScript 类型和真实调用方必须同一变更更新。

## 13. 验证

必须验证真实行为：

- 既有/legacy Markdown rebuild 后可 search/get，源文件原文不变。
- corrupt SQLite 启动后从 Markdown 重建；SQLite 删除不造成 Memory 数据丢失。
- 10K entries 下 search/get 走 SQLite bounded query；不增加不稳定 wall-clock 断言。
- user scope 隔离、importance 排序、短 query、稳定 tie-break。
- candidate retry、Markdown 成功/SQLite 失败后的重建、cursor CAS。
- `MEMORY.md` 限额内完整加载，超限写入和加载都明确失败。
- Agent 真实 Tool Loop 覆盖 search -> get -> 下一轮；普通轮不重复注入，trim 后 section 恢复且受 hard cap。
- Web 新 Session 首条消息、idle reconnect、busy 草稿保留。
- TUI 真实 `/compact` 订阅 Session timeline；replay 后仍显示 busy 并阻止 input/command。
- Desktop replay 后旧 subscription ID 仍能取消当前真实订阅。
- 完整 E2E 及使用 `conf/talos-dev.yaml` 启动 Server/Web 的手工用户路径。

## 14. 核心设计理由

1. Markdown 做事实源：用户可读、可编辑、可备份；索引损坏不等于记忆丢失。
2. SQLite FTS5 做检索：成熟、内嵌、无需新服务，直接解决 PageIndex 的 LLM 与全文件扫描开销。
3. 单库加 user scope：比每用户 DB 简单，事务、migration 和连接池只有一套，同时保持查询隔离。
4. Search/Get 分离：search 返回小 preview，只有 get 才进入完整内容，减少上下文噪声。
5. Importance 只作固定权重：可解释、可测试，不引入隐藏 rerank 或第二次 LLM。
6. `MEMORY.md` 与 daily 分层：每轮只加载少量核心事实，大量历史按需搜索。
7. 复用 Tool Loop 和 timeline：Agentic Search 与 compaction 状态都由已有基础设施表达，不发明新框架。
8. cursor 归 Session/Kernel：`DreamMemoryFormationStore` 只提交 canonical + index，避免 MemoryService 依赖 Session 生命周期。
9. 简单恢复优先：SQLite 是派生数据，删除重建已满足安全目标，不需要复杂换库协议。
