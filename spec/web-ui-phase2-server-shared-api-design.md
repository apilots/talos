# Web UI 第二阶段服务端共享 API 改造方案

## 1. 背景

第一阶段已经完成：

- Web UI 能复用现有共享 API 跑通会话、消息流、审批、历史
- TS SDK 与当前 Rust SDK / TUI 主链路已经对齐
- 页面与交互已经具备可验收形态

但当前仍有几类页面能力是假的，停留在浏览器本地状态或 placeholder 数据：

- `Project`
- `Memory`
- `Session delete`
- `Settings` 中的 workspace markdown 编辑

第二阶段目标是把这些能力逐步接回服务端真源，同时保持：

1. Web UI 与 TUI 继续走同一条共享 API 主链路
2. 优先复用现有 `session-store / coordinator / prompt memory` 路径
3. 不为了 Web UI 平行发明一套后端语义

## 2. 用户补充约束

本方案以下结论直接以用户补充约束为准：

### 2.1 Project 语义

- `Project` 是由 API 主动创建的实体
- `Session` 在任意连接模式下都可以不属于任何 `Project`
- 如果在某个 `Project` 下创建 `Session`，则 `Session` 绑定该 `project_id`
- 同一用户的 `Session` 仍然统一列表
- UI 只是支持按 `Project` 维度做展示、筛选、分组
- `Project` 自身需要保存默认工作路径
- `Project` 需要新数据库表支持完整 CRUD
- `project_root` 不再作为共享 `Session` 契约的一部分

### 2.2 Session current directory 语义

- `Session` 直接增加 `current_directory` 字段
- `current_directory` 由客户端在 create / patch session 时显式传入
- `current_directory` 可以为空
- `current_directory` 表示客户端当前工作目录，不等于 `project_id`
- `project_id` 负责服务端项目绑定
- `current_directory` 负责客户端目录上下文
- `embedded` 模式下，本地 TUI 启动时自动上报 `current_directory`
- `embedded` 模式下，服务端与客户端同机，`current_directory` 可参与真实目录解析
- `remote` 模式下，客户端不应传 `current_directory`，该字段应保持为空
- `remote` 模式下，服务端与客户端不同机，不存在客户端目录直连服务端文件系统的语义

### 2.3 Session delete 语义

- 需要一个新的真实 `delete session` API
- 不能继续把 `close` 当成删除来凑

### 2.4 Memory 语义

- `Memory` 的真实对象不是 UI 自定义结构体
- 真源是 `~/.talos/workspace-talos/memory` 目录下的 Markdown 文件
- 主要能力是：
  - 搜索 memory
  - 检索某一天 memory 文件
  - 修改某一天 memory 文件
- 目录里同时存在按天的 `.md` 文件和它们的索引文件
- 保存必须考虑和后台 memory flush 的并发冲突

### 2.5 Settings 中的 Markdown 文件编辑

- `~/.talos/workspace-talos/*.md` 里有若干顶层 Markdown 文件需要在 Web UI 中可编辑
- Settings 里增加一个专门 tab
- 左侧按文件名列出
- 点击后右侧显示内容，可编辑并保存

## 3. 目标与非目标

### 3.1 本阶段目标

- 提供真实 `Project` CRUD 与 `session-project` 关联能力
- 让 `Project.directory` 成为绑定 session 的单一执行根路径真源
- 从共享 `Session` API 中移除 `project_root`
- 在共享 `Session` 契约中增加 `current_directory`
- 提供真实 `Session delete` 能力
- 提供真实 `Memory markdown` 搜索、列表、读取、保存能力
- 提供真实 `Workspace markdown docs` 列表、读取、保存能力
- 保持 workspace memory、bootstrap docs、prompt builder 真源不被破坏

### 3.2 非目标

- 本批不做 replayable session stream
- 本批不做 admin 数据面
- 本批不改当前消息流、审批、turns 主链路
- 本批不做 Electron 本地模式专属能力
- 本批不做任意路径文件编辑

## 4. 范围与优先级

| 优先级 | 能力 | 本阶段结论 |
|---|---|---|
| P0 | Project APIs | 立即实现 |
| P0 | Session create/patch with `project_id` | 立即实现 |
| P0 | Session Delete API | 立即实现 |
| P0 | Memory Markdown APIs | 立即实现 |
| P1 | Workspace Markdown Docs APIs | 与 P0 同期或后一批 |
| P2 | Project-scoped `AGENTS.md` editor | 延期 |
| P2 | Replayable Session Stream | 延期 |
| P2 | Admin APIs | 延期 |

## 5. 设计原则

### 5.1 Session 仍然是执行主轴

`Session` 不因为 `Project` 的引入而改变主语义。

- 统一列表仍然是 `Session` 列表
- `Project` 只是一个可选的组织与执行根路径实体
- 一个 `Session` 可以：
  - 关联某个 `Project`
  - 或者不关联任何 `Project`

### 5.2 Project 与 current_directory 分别承担不同职责

本阶段目标不是把旧的 `project_root` 挪个位置继续保留，而是消除双真源。

最终共享语义改为：

- `Session` 仅保存可选 `project_id`
- `Session` 额外保存可选 `current_directory`
- `Project.directory` 是绑定 session 的服务端执行根路径真源
- 未绑定 `Project` 的 session，执行时再结合 `connection_mode + current_directory` 解析有效根路径
- 共享 `Session` DTO 不再暴露 `project_root`
- 共享 `SessionPatch` 不再接收 `project_root`

这意味着：

- 创建 session 时传 `project_id`，只建立绑定关系
- 创建或更新 session 时传 `current_directory`，只更新客户端目录上下文
- 执行时按 `session.project_id / session.current_directory / connection_mode` 解析有效根路径
- `Project.directory` 在 project 创建后不可修改

这里的 workspace default root 明确定义为：

- `config.agent.kernel.memory.workspace_dir`
- 即当前 memory / bootstrap / workspace services 共同使用的 workspace 根目录配置

本阶段不新增单独的“查询 default root”接口。  
前端若需要展示“未绑定 project”的来源，直接使用固定文案 `Workspace Default`；真正执行根路径由服务端按配置与连接模式解析。

这是有意设计。  
如果未来需要“创建时冻结服务端目录快照”的能力，应引入显式的新类型，而不是把匿名 `project_root` 字段重新塞回 `Session`。

### 5.3 Memory 必须直接尊重 Markdown 真相，并处理并发写

这批不能走 `MemoryEntry` 数据库表路线。  
真实真源仍然是：

- `~/.talos/workspace-talos/memory/*.md`
- 以及对应 `*.index.json`

同时必须处理一个关键现实：

- 用户可能正在 Web UI 里编辑某一天的 memory 文件
- 服务端也可能在 session close / flush 时往同一天文件追加 block

因此 memory save 不能做 blind overwrite，必须有 optimistic concurrency。

### 5.4 Workspace docs 与 project docs 必须按作用域拆开

`workspace` 顶层 docs 和 `project` 作用域 docs 不是一回事。

- `SOUL.md`
- `IDENTITY.md`
- `USER.md`
- `HEARTBEAT.md`
- `MEMORY.md`

属于 workspace docs。

`AGENTS.md` 的真实作用域是 project root。  
它应跟随 project 绑定解析，不应伪装成 workspace Settings tab 里的顶层文档。

### 5.5 共享 API 契约必须单一真源

这份 spec 不是局部 Web patch，而是 shared API 方案。  
因此必须同步约束：

- Rust DTO
- TS SDK types
- gateway request/response schema
- Web UI 前端 domain model

不能只改一份局部 spec，放任协议漂移。

## 6. 总体架构

```text
Web UI / TUI
    |
    v
shared SDK
    |
    v
gateway/http
    |
    v
coordinator
    |
    +-------------------------------+------------------------------+
    |                               |                              |
    v                               v                              v
session-store                 workspace doc adapter           memory file adapter
    |                               |                              |
    +-------------+                 v                              v
    |             |            talos-prompt/bootstrap         talos-memory
    v             v            workspace docs                 memory/*.md + index
session metadata  project records
project_id        directory
```

说明：

- `Project` 与 `Session delete` 更接近 `session-store`
- `Memory markdown` 与 `workspace markdown docs` 只在 HTTP 协议层同属“文件编辑 API”
- 实现层不引入新的统一 `workspace file services` 子系统
- `workspace docs` 继续复用 `talos-prompt/bootstrap` 路径
- `memory markdown` 继续复用 `talos-memory` 的索引、检索、写入路径
- 不新建平行 memory 子系统
- `current_directory` 是共享协议中的持久字段
- effective root 仍然只是 coordinator 在执行前解析出的派生值

### 6.1 分层职责钉死

为避免实现阶段职责下沉错层，这里明确约束：

- `gateway/http`
  - 只负责协议适配、鉴权、DTO decode/encode、错误码映射
  - 不负责解析 effective root
  - 不负责直接操作 memory/doc 真源文件
- `coordinator`
  - 负责 session -> project / current_directory / connection_mode -> effective root 的编排解析
  - 负责决定当前请求走 project adapter、memory adapter、doc adapter 中的哪条路径
  - 负责 active session delete 等运行态约束判断
- `session-store`
  - 只负责持久化 `Session.project_id`、`Session.current_directory` 与 `ProjectRecord`
  - 不负责把 `project_id` 解析成 effective root
  - 不负责读取 bootstrap docs 或 memory 文件
- `workspace doc adapter`
  - 是复用 `talos-prompt/bootstrap` 的适配层，不是新子系统
  - 只负责 `SOUL/IDENTITY/USER/HEARTBEAT/MEMORY` 这组 workspace docs 的文件读写和版本控制
- `memory file adapter`
  - 是复用 `talos-memory` 的适配层，不是新子系统
  - 只负责 memory day files 的 list/search/read/save 与索引一致性

这部分必须落实到实现拆分上，避免再次出现“为了方便把编排逻辑塞进 store”或“为了统一文件 API 新建一个横跨 prompt/memory 的大服务”。

## 7. 详细方案

## 7.1 Project APIs 与 Session 绑定模型

### 7.1.1 问题定义

当前 Web UI 的项目列表是浏览器本地注册表，刷新后丢失，且与服务端没有统一真源。  
同时现有执行路径语义耦合在 `Session.context.project_root` 上，不利于引入独立 `Project` 概念，也不利于把 `project_id` 保持为真正可选字段。

### 7.1.2 设计结论

- 新增服务端 `ProjectRecord`
- `ProjectRecord` 由 API 显式创建
- `Session` 增加可选 `project_id`
- `Session` 增加可选 `current_directory`
- `Session` 从 shared API 中移除 `project_root`
- `Project.directory` 成为绑定 session 的执行根路径真源
- 未绑定 project 的 session 再按 `connection_mode + current_directory` 解析有效根路径
- 会话列表仍统一返回所有 `Session`
- Web UI 按 `project_id` 做筛选与分组
- `project_id` 对 embedded / remote session 都是可选字段
- `Session` 可以绑定零个或一个 `Project`

### 7.1.3 类型定义

```rust
struct ProjectRecord {
    project_id: String,
    name: String,
    directory: String,
    created_at: i64,
    updated_at: i64,
}

enum OptionalProjectIdPatch {
    Set { project_id: String },
    Clear,
}

struct SessionAgentConf {
    model: Option<String>,
}

struct SessionConf {
    agent: Option<SessionAgentConf>,
}

enum OptionalCurrentDirectoryPatch {
    Set { path: String },
    Clear,
}

struct SessionPatch {
    conf: Option<SessionConf>,
    project_id: Option<OptionalProjectIdPatch>,
    current_directory: Option<OptionalCurrentDirectoryPatch>,
}

struct ProjectPatch {
    name: Option<String>,
}
```

### 7.1.4 Project API

```text
GET    /v1/projects/{project_id}
GET    /v1/projects
POST   /v1/projects
PATCH  /v1/projects/{project_id}
DELETE /v1/projects/{project_id}
```

请求示例：

```json
POST /v1/projects
{
  "name": "talos",
  "directory": "/Users/xujiang/data/ailabs/talos"
}
```

路径校验规则：

- `directory` 必须是服务端可见的绝对路径
- 该路径必须已存在
- 该路径必须是目录
- 不接受空字符串、相对路径、桌面端本地专属路径别名

如果校验失败，返回 `400 invalid_request`。

`PATCH /v1/projects/{project_id}` 采用 partial update 语义：

- `name` 可选
- `directory` 不允许修改
- 请求体至少要有一个字段存在
- 若请求中出现 `directory`，返回 `400 invalid_request`

### 7.1.5 Session create / bind / current_directory 语义

创建 session 时支持可选 `project_id` 与 `current_directory`：

```json
POST /v1/sessions
{
  "context": { "...": "..." },
  "model": "gpt-5.4",
  "project_id": "proj_123",
  "current_directory": "/Users/xujiang/data/ailabs/talos"
}
```

规则如下：

1. `project_id` 对 embedded / remote session 都是可选字段
2. `project_id` 缺省时，session 不绑定 project
3. `project_id` 存在时，服务端校验该 project 存在，并写入 `session.project_id`
4. `current_directory` 缺省时，表示客户端未提供当前目录
5. `embedded` 模式下，TUI 创建 session 时自动上报当前 cwd 到 `current_directory`
6. `current_directory` 存在时：
   - 必须是非空绝对路径字符串
   - 必须存在且为目录
   - 否则返回 `400 invalid_request`
   - 写入 `session.current_directory`
7. `remote` 模式下：
   - 客户端不应传 `current_directory`
   - 该字段应保持为空
8. session 执行时：
   - 若 `session.project_id != null`，有效根路径取 `Project.directory`
   - 若 `session.project_id == null` 且 `connection_mode == Embedded` 且 `session.current_directory != null`，有效根路径取 `session.current_directory`
   - 若 `session.project_id == null` 且不满足上条，有效根路径取 `config.agent.kernel.memory.workspace_dir`
9. `PATCH /v1/sessions/{session_id}` 中：
   - `project_id = set` 表示重新绑定 project
   - `project_id = clear` 表示解除 project 绑定
   - `current_directory = set` 表示更新客户端当前目录
   - `current_directory = clear` 表示清空客户端当前目录
10. patch `project_id` 只改绑定关系，不复制路径、不写回匿名 `project_root`
11. patch `current_directory` 只改客户端目录上下文，不创建任何隐式 project
12. `CreateSessionRequest.context` 若仍携带 `project_root`，shared API 直接返回 `400 invalid_request`

这个约束必须严格执行，否则又会回到 `Project`、`current_directory` 和 `project_root` 三套语义互相污染。

effective root 解析职责进一步明确：

- `session-store` 只返回 `session.project_id`、`session.current_directory`
- `coordinator` 在进入 agent/kernel 之前解析：
  - `project_id -> Project.directory`
  - 或 `Embedded + current_directory -> current_directory`
  - 或回退到 `config.agent.kernel.memory.workspace_dir`
- 解析出的 effective root 只作为当前执行请求的派生值向下游传递
- 该派生值不得回写到 shared `Session` DTO 或 `session_records`

### 7.1.6 Embedded / Remote 目录语义

这批不做兼容模式与灰度开关，直接统一到新规范：

1. 共享协议只认：
   - `project_id`
   - `current_directory`
2. 旧的 `project_root` 从 shared DTO、SDK、TUI、Web 一次性移除
3. `embedded` 模式下：
   - 客户端与服务端同机
   - `current_directory` 可被服务端验证并直接作为未绑定 project session 的有效根路径
4. `remote` 模式下：
   - 客户端与服务端不同机
   - `project_id` 仍然是可选字段
   - `current_directory` 应保持为空
   - 服务端只操作：
     - `Project.directory`
     - `config.agent.kernel.memory.workspace_dir`
     - 以及显式指定的其他服务端可见路径
5. 因此 remote 场景下，客户端本地 cwd 不能再被误当成服务端项目目录
6. 客户端若需要改变 remote 场景中的真实执行目录，应通过：
   - 绑定/切换 `project_id`
   - 或后续单独设计的服务端目录选择能力

### 7.1.7 Project delete 语义

`DELETE /v1/projects/{project_id}` 采用 `restrict delete`：

- 若仍有任意 session 绑定该 project，返回 `409 conflict`
- 只有当绑定 session 数量为 `0` 时，才允许物理删除
- 删除 project 不删除磁盘文件

这样可以避免：

- 删除 project 后留下悬空 `project_id`
- 或在未告知用户的情况下，隐式把一批 session 迁回默认根路径

### 7.1.8 持久化

新增 `project_records` 表：

```text
project_records
  project_id TEXT PRIMARY KEY
  name TEXT NOT NULL
  directory TEXT NOT NULL
  created_at INTEGER NOT NULL
  updated_at INTEGER NOT NULL
```

扩展 `session_records`：

```text
session_records
  ...
  project_id TEXT NULL
  current_directory TEXT NULL
```

约束：

- `session_records.project_id` 引用 `project_records.project_id`
- `DELETE project` 采用应用层或数据库层 `RESTRICT`
- `Session` 持久化模型中不再保留 `project_root`
- `current_directory` 允许为空
- `current_directory` 仅作为客户端目录上下文字段持久化
- `project_records.directory` 创建后不可修改

## 7.2 Session Delete API

### 7.2.1 问题定义

当前只有 `close session`，没有真实 delete。  
Web UI 想做删除时只能误用 close，语义不对。

### 7.2.2 设计结论

- 保留现有 `close session`
- 新增真实 `delete session`
- 两个能力语义严格分开
- 删除 API 路径直接使用标准资源删除语义，避免继续引入 `/record` 之类的过渡命名

### 7.2.3 API

```text
DELETE /v1/sessions/{session_id}
POST   /v1/sessions/{session_id}/close
```

说明：

- `DELETE` 是永久删除 session 记录与关联产物
- `close` 只表示结束会话并触发必要的 flush / 收尾

### 7.2.4 删除语义

删除 session 需要清理：

- session record
- session turns
- session transcript artifacts
- session summaries
- 相关缓存

但不应误删：

- project record
- workspace memory 独立文件
- workspace docs

### 7.2.5 Active session delete 语义

若目标 session 当前存在以下任一状态：

- 活跃 turn
- 活跃 stream
- pending approval
- 尚未完成的 dynamic tool continuation

则 `DELETE /v1/sessions/{session_id}` 返回 `409 conflict`。

本阶段不允许：

- 隐式先 `close` 再 `delete`
- 强制中止后继续删除

调用方必须先显式结束运行态，再发起删除。  
这样可以避免 turn 持久化与删除并发交错。

## 7.3 Memory Markdown APIs

### 7.3.1 问题定义

当前 Web `/memory` 使用本地 placeholder records。  
但真实 memory 来自：

- `~/.talos/workspace-talos/memory/*.md`
- 以及对应索引文件

所以这批不能抽象成新的数据库 memory 表。  
同时 save 还必须解决与后台 flush 的并发覆盖问题。

### 7.3.2 设计结论

Memory 页的真实能力收敛成 4 类：

1. 搜索 memory
2. 列出 memory 日文件
3. 读取某个 memory 日文件
4. 保存某个 memory 日文件

### 7.3.3 类型定义

```rust
struct MemoryDayFileSummary {
    date: String,
    path: String,
    title: String,
    updated_at: i64,
    section_count: usize,
    version_token: String,
}

struct MemoryDayFileDetail {
    date: String,
    path: String,
    content: String,
    updated_at: i64,
    version_token: String,
}

struct MemorySearchHit {
    path: String,
    date: String,
    snippet: String,
    start_line: usize,
    end_line: usize,
}

enum ExpectedFileVersion {
    Missing,
    Match { version_token: String },
}

struct DocUpdateRequest {
    content: String,
    base_version: ExpectedFileVersion,
}
```

`version_token` 统一使用 `content_hash` 生成。  
关键要求不是是否带时间戳，而是必须稳定表示“客户端基于哪一版文件编辑”，并与索引过期判定使用同一真源。

### 7.3.4 API

```text
GET  /v1/memory/files?limit=...&before_date=...&date_from=...&date_to=...
GET  /v1/memory/files/{date}
PUT  /v1/memory/files/{date}
GET  /v1/memory/search?q=...&limit=...&date_from=...&date_to=...
```

其中：

- `{date}` 对应按天 markdown 文件，例如 `2026-04-17`
- 服务端自行映射到 workspace memory 目录下的真实路径
- `/v1/memory/files` 按日期倒序返回
- `before_date` 用于 cursor 式翻页，表示仅返回更早的文件
- `/v1/memory/files.limit` 默认使用 `config.agent.kernel.memory.file_list_default_limit`
- `/v1/memory/files.limit` 最大值受 `config.agent.kernel.memory.file_list_max_limit` 限制
- `/v1/memory/search.limit` 默认使用 `config.agent.kernel.memory.search_default_limit`
- `/v1/memory/search.limit` 最大值受 `config.agent.kernel.memory.search_max_limit` 限制
- `date_from` / `date_to` 为可选过滤条件，格式为 `YYYY-MM-DD`

说明：

- `file_list_default_limit < file_list_max_limit`
- `search_default_limit < search_max_limit`
- 默认值用于首屏体验，最大值用于服务端硬上限保护
- `before_date` 表示严格早于该日期的 cursor 条件
- `before_date` 与 `date_from/date_to` 不互斥，语义为取交集
- 处理顺序为：
  1. 先按 `date_from/date_to` 过滤候选文件
  2. 再应用 `before_date`
  3. 最后按日期倒序裁剪 `limit`

### 7.3.5 Save 语义

保存某天 memory 文件时，服务端必须：

1. 获取该 `{date}` 文件级写锁
2. 读取当前 `.md` 与 `.index.json` 快照，计算当前 `version_token`
3. 校验 `base_version`
4. 校验规则：
   - `Missing` + 文件不存在 => 允许创建
   - `Missing` + 文件已存在 => 返回 `409 conflict`
   - `Match { version_token }` + token 匹配 => 允许更新
   - `Match { version_token }` + token 不匹配 => 返回 `409 conflict`
5. 基于新 content 在内存中构建目标 `.index.json`
6. 分别写入临时文件：
   - `{date}.md.tmp`
   - `{date}.index.json.tmp`
7. 在同一锁内提交文件替换：
   - 先备份旧 `.md` / `.index.json` 元数据
   - 再 rename temp files 覆盖正式文件
8. 若任一步提交失败，服务端必须在释放锁前恢复旧 `.md` 与旧 `.index.json`
9. 两个正式文件都提交成功后，返回新的 `MemoryDayFileDetail`

不能允许：

- blind overwrite
- Markdown 内容更新了，但索引仍然是旧的
- 用户覆盖掉后台 flush 刚刚追加的 block
- `.md` 已更新但 `.index.json` 停留旧版本

### 7.3.6 与索引文件的关系

保存 memory 日文件后，服务端必须保证：

1. `.md` 写入成功
2. 对应 `.index.json` 刷新成功
3. 后续 memory search / retrieval 读取到的是新内容

如果索引缺失或过期，search / get / list 路径必须先同步修复，再继续读取。  
若修复失败，整个请求报错，不做静默降级，也不 fallback 到全文扫描。

索引过期判定规则：

- `.index.json` 缺失
- `.index.json` 中记录的源文件版本落后于当前 `.md`

为支持这一点，索引文件必须包含：

- `source_content_hash`

要求：

- `source_content_hash` 取构建索引时 `.md` 全文件内容的 hash
- search / get / list 在读取索引前，必须重新计算当前 `.md` 的 content hash
- 若当前 hash 与索引中的 `source_content_hash` 不一致，则视为索引过期

本方案不采用 `mtime` 作为过期判定真源，避免时间精度和并发写导致误判。

## 7.4 Workspace Markdown Docs APIs

### 7.4.1 问题定义

Settings 里需要一个专门 tab，编辑 workspace 根目录下的一组 Markdown 文件。  
这些文件不是普通附件，而是现有 prompt/bootstrap 真源的一部分。

但必须注意：

- workspace 顶层 docs
- project root 下的 `AGENTS.md`

不是同一作用域。

### 7.4.2 设计结论

本批 `Settings -> Docs` 只支持 workspace 顶层白名单文件：

- `SOUL.md`
- `IDENTITY.md`
- `USER.md`
- `HEARTBEAT.md`
- `MEMORY.md`

不包含：

- `AGENTS.md`

原因：

- `AGENTS.md` 的真实作用域是 project root
- 当前 session 已改为通过 `project_id` 绑定 project
- `AGENTS.md` 应跟着 project 解析，而不是混进 workspace Settings tab

如需编辑 `AGENTS.md`，后续单独设计 project-scoped docs API。

### 7.4.3 类型定义

```rust
enum WorkspaceDocKind {
    Soul,
    Identity,
    User,
    Heartbeat,
    Memory,
}

struct WorkspaceDocSummary {
    kind: WorkspaceDocKind,
    file_name: String,
    path: String,
    exists: bool,
    updated_at: Option<i64>,
    version_token: Option<String>,
}

struct WorkspaceDocDetail {
    kind: WorkspaceDocKind,
    file_name: String,
    path: String,
    exists: bool,
    content: String,
    updated_at: Option<i64>,
    version_token: Option<String>,
}

// Reuse shared DocUpdateRequest
```

### 7.4.4 API

```text
GET  /v1/workspace/docs
GET  /v1/workspace/docs/{kind}
PUT  /v1/workspace/docs/{kind}
```

说明：

- `{kind}` 必须是强类型枚举值，不用任意文件名字符串
- 服务端自行把 kind 映射到 workspace 根目录具体文件
- 保存时可复用现有 bootstrap 文件创建逻辑
- 与 memory save 一样，workspace docs save 也必须做 optimistic concurrency
- `GET /v1/workspace/docs/{kind}` 若目标文件不存在，返回 `200 OK`
- 不存在时响应体必须满足：
  - `exists = false`
  - `content = ""`
  - `updated_at = null`
  - `version_token = null`

这样客户端可以直接渲染“文件不存在，可创建”的状态，而不需要依赖额外的 `404` 分支。

### 7.4.5 Workspace docs save 语义

保存 `workspace/docs/{kind}` 时，服务端必须：

1. 获取 `{kind}` 对应文件的文件级写锁
2. 读取当前文件内容并生成 `version_token`
3. 校验 `base_version`
4. 校验规则：
   - `Missing` + 文件不存在 => 允许创建
   - `Missing` + 文件已存在 => 返回 `409 conflict`
   - `Match { version_token }` + token 匹配 => 允许更新
   - `Match { version_token }` + token 不匹配 => 返回 `409 conflict`
5. 使用 temp file + rename 覆盖正式文件
6. 返回新的 `WorkspaceDocDetail`

这批不假设 workspace docs “只有人工编辑”。  
只要是可被 prompt/bootstrap 读取的真源文件，就统一采用版本冲突保护。

## 7.5 Shared API / DTO 同步要求

这批改造必须同步修改共享契约：

### 7.5.1 Session DTO

- `TalosSession` / session list DTO 增加 `project_id: Option<String>`
- `TalosSession` / session list DTO 增加 `current_directory: Option<String>`
- shared API 不再暴露 `context.project_root`

### 7.5.2 Create session 请求

```rust
struct CreateSessionRequest {
    context: TalosContext,
    model: Option<String>,
    project_id: Option<String>,
    current_directory: Option<String>,
}
```

约束：

- `context.project_root` 在 shared API 中是非法字段
- 传入即返回 `400 invalid_request`
- `current_directory` 允许为空
- `current_directory` 若存在，必须是绝对路径且存在的目录

### 7.5.3 SessionPatch

```rust
struct SessionAgentConf {
    model: Option<String>,
}

struct SessionConf {
    agent: Option<SessionAgentConf>,
}

struct SessionPatch {
    conf: Option<SessionConf>,
    project_id: Option<OptionalProjectIdPatch>,
    current_directory: Option<OptionalCurrentDirectoryPatch>,
}
```

不再允许：

- `project_root`

本阶段 `SessionConf` 只支持：

- `agent.model`

不允许：

- 任何未在 spec 中显式声明的 conf 字段

若收到未知字段，返回 `400 invalid_request`，而不是静默丢弃。  
后续若需要支持更多 session config，必须以新增强类型字段的方式扩展此结构。

### 7.5.4 Effective root 解析约束

执行层允许派生“有效根路径”，但必须满足：

- 派生值不进入 shared DTO
- 派生值不重新变成 `Session` 持久化字段
- shared API / TUI / TS SDK / Web 统一只认：
  - `project_id`
  - `current_directory`
- `project_id` 负责服务端项目绑定
- `current_directory` 负责客户端目录上下文

## 8. 模块复用边界

| 能力 | 优先复用 | 需要新增 |
|---|---|---|
| Project CRUD | `session-store`、`coordinator`、`gateway/http` | `project_records` 表与 CRUD store |
| Session project binding | 现有 `create_session / patch_session` 路径 | `project_id`、`current_directory` 字段、project lookup、coordinator 内 effective root resolution |
| Session delete | `session-store`、`coordinator`、memory artifact store | 新 delete handler 与底层删除流程 |
| Memory markdown | `talos-memory` 的 workspace memory 目录、索引构建、检索逻辑 | Web memory adapter、version token 校验 |
| Workspace docs | `talos-prompt` 的 bootstrap 文件名与创建逻辑 | Workspace doc adapter、白名单 docs handler |

补充约束：

- `adapter` 在本方案里表示“HTTP/coordinator 到现有模块的薄适配层”
- `adapter` 不能演化成新的领域子系统
- 若某个适配层开始同时持有 prompt 和 memory 的核心规则，说明分层已经失控，需要回退重构

## 9. API 清单

```text
GET    /v1/projects/{project_id}
GET    /v1/projects
POST   /v1/projects
PATCH  /v1/projects/{project_id}
DELETE /v1/projects/{project_id}

POST   /v1/sessions
PATCH  /v1/sessions/{session_id}
DELETE /v1/sessions/{session_id}
POST   /v1/sessions/{session_id}/close

GET    /v1/memory/files
GET    /v1/memory/files/{date}
PUT    /v1/memory/files/{date}
GET    /v1/memory/search

GET    /v1/workspace/docs
GET    /v1/workspace/docs/{kind}
PUT    /v1/workspace/docs/{kind}
```

## 10. 分阶段落地建议

### 第一批

- `Project APIs`
- `POST /v1/sessions` 支持 `project_id`
- `PATCH /v1/sessions/{id}` 支持 `project_id + current_directory`
- `Session Delete API`
- `Memory Markdown APIs`

理由：

- 直接解决当前最明显的“页面是假功能”问题
- 同时完成 `Project` 与 `Session` 契约解耦，以及 `current_directory` 引入
- 能在不触碰 replayable stream 的情况下独立推进

### 第二批

- `Workspace Markdown Docs APIs`

理由：

- 与 settings 页面强相关
- 但比 Project / Memory / Delete 的优先级略低

### 第三批

- `Project-scoped AGENTS.md editor`
- `Replayable session stream`
- `Admin APIs`

## 11. 风险与注意事项

### 11.1 Project 必须成为单一执行根路径真源

必须始终满足：

- `session.project_id` 决定是否绑定 project
- `Project.directory` 决定绑定 session 的执行根路径
- shared `Session` 契约中不再出现 `project_root`
- `project_id` 对任意 session 都是可选字段

### 11.1.1 current_directory 不能与 Project 混成双真源

必须始终满足：

- `current_directory` 只是客户端上报的当前目录
- 只有 `embedded` 同机场景下，`current_directory` 才能参与未绑定 session 的 effective root 解析
- `remote` 场景下，`current_directory` 绝不能直接作为服务端文件操作根路径
- `remote` 场景下，客户端默认不传 `current_directory`

### 11.2 Memory 编辑必须带版本冲突校验与索引刷新

否则会出现：

- 页面看到新内容
- 后台 flush 又写入旧上下文
- 搜索和 prompt 仍然命中旧内容

### 11.3 Workspace docs 也必须带版本冲突校验

否则会出现：

- Web 保存覆盖他人修改
- prompt/bootstrap 读取到非预期版本

### 11.4 Project delete 必须 restrict

否则会出现：

- session 悬挂到不存在的 `project_id`
- 或用户未感知的执行根路径漂移

### 11.5 Active session 不允许直接 delete

否则会出现 turn 持久化与删除并发交错。

### 11.6 Workspace docs 与 Project docs 必须分 scope

Settings 里的 Markdown 编辑不能放开成任意路径写入，也不能把 `AGENTS.md` 混进 workspace docs。

### 11.7 共享协议必须同步更新

如果只改这份 spec，不同步：

- Rust DTO
- TS SDK
- gateway schema
- Web domain types

就会继续产生协议漂移。

## 12. 建议 Review 点

这版 spec 最值得 review 的是 6 个问题：

1. `Project` 是否成为绑定 session 的单一服务端执行根路径真源，并从 shared `Session` 中移除 `project_root`
2. `current_directory` 是否作为独立 session 字段引入，并与 `project_id` 明确分责
3. `embedded` / `remote` 是否已经清楚区分“客户端目录上下文”和“服务端可操作目录”
4. `SessionPatch.conf` 是否已收敛为强类型对象，而不是 `HashMap<String, Value>`
5. `DELETE /v1/projects/{id}` 是否采用 `restrict delete`，`DELETE /v1/sessions/{id}` 是否对 active session 返回 `409`
6. `Memory` 与 `Workspace docs` 是否都采用 `version_token + temp file + rollback-on-failure` 写入语义，并把 `AGENTS.md` 继续留在 project-scoped 设计里
