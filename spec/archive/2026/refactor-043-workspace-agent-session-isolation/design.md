# Workspace、Agent 与 Session 隔离边界设计

Status: archived
Owner: agent
Created: 2026-08-29
Updated: 2026-08-29
Related: `feat-032-session-subagent`, `refactor-041-user-workspace-memory`, `refactor-042-dream-memory-okf`, `archive/2026/feat-021-user-scoped-agent-workspace`, `archive/2026/refactor-031-api-concept-alignment`, `archive/2026/refactor-032-agent-harness-api`

Supersedes: 仅在 Workspace path ownership、Session binding 和通用 Memory scope identity 发生冲突时，本 Spec 替代上述旧 Spec 的相关定义；既有 Memory 内容格式与 formation 规则继续有效。

## 1. 结论

Talos 将 Workspace、Agent、User 和 Session 定义为相互独立的业务对象：

- Workspace 是由服务端配置管理的内部数据空间，不属于任何 Agent；
- Agent 是可复用的执行能力定义，不持有 Workspace 路径或用户状态；
- User 是数据归属和权限主体；
- Session 是 User、Workspace、Agent 与可选 Project 的稳定关联；
- Turn 只继承 Session 已锁定的关联，不重新选择 Agent、Workspace 或 Project。

当前产品只有一个内部 Workspace：

```text
workspace_id = default
```

用户侧不感知、不创建、不选择 Workspace。当前 `default` Workspace 的 prefix 固定为：

```text
${TALOS_HOME}/workspace-talos
```

`TALOS_HOME` 表示 Talos Runtime Home，默认值为 `$HOME/.talos`，因此本地默认实际路径是 `~/.talos/workspace-talos`。系统再由可信 `user_id` 派生有效目录：

```text
${TALOS_HOME}/workspace-talos/<user-id>
```

当前目录不增加 `users/` 层，也不增加无实际语义的 `default/` tenant 层。未来出现真实 Tenant 后，可以在不改变核心对象关系的前提下扩展为：

```text
${TALOS_HOME}/workspace-talos/<tenant-id>/<user-id>
```

Workspace prefix 根目录只承担目录分区，不是任何 User 的 EffectiveWorkspace。`SOUL.md`、`USER.md` 等用户态文件不得创建、读取或保留为该根目录的共享事实源。

用户发送第一条消息前的“新对话”只是客户端临时状态，不是持久化 Session。系统接受第一个 Turn 时，使用最终的 `user_id + workspace_id + agent_id + optional project_id` 创建 Session；Session 一经创建，这四个绑定均不可修改。普通会话允许 `project_id = None`，从 Project 上下文创建的会话必须携带属于当前 User 的有效 `project_id`。首个 Turn 在建立执行事实前失败时，可以沿用现有清理语义丢弃新建 Session；需要切换 Agent、Workspace 或 Project 时，必须创建或 fork 新 Session。

## 2. 目标

1. 建立 Workspace、Agent、User、Session 和 Turn 的单一业务定义与稳定关系。
2. 解除 Agent 配置、Agent identity 与 Workspace 物理路径的强绑定。
3. 让 Local TUI、Local Desktop 和 Cloud Web Server 使用同一套隔离模型。
4. 当前仅支持内部 `default` Workspace，且不向用户暴露 Workspace 选择。
5. 不依赖 Workspace 数据库表，通过配置和可信身份确定有效目录。
6. 保持当前多 Agent 架构：同一 Agent 可运行于不同用户空间，多个 Agent 也可运行于同一用户空间。
7. 保证同一 Session 的 transcript、memory、tools、prompt 和状态始终使用同一 Workspace 与 Agent 绑定。
8. 为未来多 Workspace 和 Tenant 提供清晰升级边界，但不提前实现其管理功能。

## 3. 非目标

1. 不实现用户可见的 Workspace 创建、列表、切换或管理 UI。
2. 不新增 Workspace Store、Workspace 数据库表或通用 Workspace Registry Service。
3. 不实现 Tenant、组织成员、Workspace 共享、配额或计费。
4. 不让 Workspace 决定 Agent 路由；Agent 选择继续属于路由或 Session 创建策略。
5. 不把 Project、`project_id`、`project_root` 或 `current_directory` 重命名为 Workspace。
6. 不允许 request 直接提交 Workspace 路径、`user_id` 或 Tenant 路径片段。
7. 不为多 Workspace 提前增加用户协议字段或 UI 状态。
8. 不在本 Spec 中定义实施阶段；对应 Plan 在实现开始时单独编写。

## 4. 当前实现事实

1. `SessionRecord` 持久化必需的 `user_id`、`workspace_id`、`agent_id` 和可选的 `project_id`；SQL migration 已回填历史空 Workspace 并将 `workspace_id` 收紧为非空字段。
2. 当前没有独立的公开 Session Create API；缺失 Session 会在首个 Start Turn 链路中解析 Agent、读取配置的默认 Workspace，并创建完整绑定。
3. Session Store 的普通 update 只更新可变状态，不再写入 `user_id`、`workspace_id`、`agent_id`、`project_id` 或 identity context。
4. `StartTurnRequest` 不再接受 per-Turn `workspace_id` override；已有 Session 若收到不同的非空 `project_id` 也会明确拒绝。
5. `Config.workspaces` 是真实序列化配置对象；当前三个发布配置都定义 `default + workspace-talos`，Loader 要求该配置块存在并执行引用、ID 与路径校验。
6. EffectiveWorkspace 统一由 `WorkspaceConfig + TALOS_HOME + user_id` 解析，当前默认目录为 `${TALOS_HOME}/workspace-talos/<user-id>`，不再由 Agent name 或 Agent Kernel 路径派生。
7. AgentDefinition、Agent Runtime 和 Session lifecycle 使用 Session 持久化的精确 Agent 与 Workspace binding；Agent 不拥有 Workspace path 或默认 Workspace。
8. Project Session 创建时会校验 Project 存在且属于同一 User；`session.patch` 已删除 `project_id` Set/Clear。
9. Workspace Docs、User Workspace Memory、Dream Memory、Prompt、Tool、Transcript 和 History 使用同一个 EffectiveWorkspace scope；通用 Memory 的 Agent ID 只保留为 provenance 或 formation policy 输入。
10. 当前发布配置只定义一个默认 Agent，但 AgentDefinition、AgentRegistry、Coordinator 和 Subagent 继续按多 Agent 方向设计；Subagent 继承父 Session binding，并拒绝模型输入 `agentId`。
11. Prompt bootstrap 只接收 EffectiveWorkspace root；Workspace prefix 根目录不会被当作 User scope 创建或读取 `SOUL.md`、`USER.md`。
12. `talos-upgrade` 在 Runtime 构建前串行执行 SQLx database migration 与通用 Upgrade Task；Task 必须声明目标 Talos 产品版本，只执行产品升级区间内的任务，新安装直接登记当前产品版本。
13. 未登录模式继续使用 `user_id = anonymous`，初始化路径为 `${TALOS_HOME}/workspace-talos/anonymous`，不再创建 `users/anonymous`。
14. App Protocol、SDK 和 Web 已删除 `workspace.info`、绝对 Workspace 路径展示与已有 Session 的 Project 切换入口。
15. `AGENTS.md` 属于 Project root 的上下文文件，不是 User Workspace 文件。

### 4.1 代码核对基线

| 事实域 | 已核对实现 |
| --- | --- |
| 配置与路径派生 | `talos-rs/crates/infra/config/src/paths.rs`、`loader.rs`、`workspace.rs`、`domains/kernel.rs` |
| Session 持久化与首 Turn | `talos-rs/crates/orchestration/store/src/session/types.rs`、`service/src/access/service.rs`、`turn/service.rs` |
| Session lifecycle 与 Agent 路由 | `talos-rs/crates/orchestration/service/src/session/service.rs`、`coordinator/src/coordinator.rs` |
| Agent runtime 与 Subagent | `talos-rs/crates/orchestration/agent/src/definition.rs`、`agent/src/agent/kernel.rs`、`action/tools/src/tools/subagent.rs` |
| Prompt、Transcript 与 History | `talos-rs/crates/common/prompt/src/builder.rs`、`orchestration/agent/src/session_artifacts.rs`、`orchestration/service/src/history/service.rs` |
| Workspace Docs 与 Memory | `talos-rs/crates/orchestration/service/src/workspace/service.rs`、`workspace_docs/service.rs`、`cognition/memory/src/ports.rs` |
| Startup 与 Upgrade | `talos-rs/crates/app/talos-base/src/application_runtime.rs`、`talos-rs/crates/app/talos-upgrade/src/lib.rs` |
| App Protocol、SDK 与 Web UI | `talos-rs/crates/gateway/app-protocol/src/requests.rs`、`talos-app/ts-sdk/packages/core/src/generated/app-protocol.ts`、`talos-app/web/src/pages/chat/useChatSessionComposer.ts` |

本表只记录本次设计判断的事实来源，不把文件位置固化为目标架构。

## 5. 核心业务对象

### 5.1 User

User 是数据归属、鉴权和隔离主体。

```text
User
  user_id       stable storage and authorization identity
  username      display identity only
```

约束：

- Workspace 路径、Store 查询和权限判断只使用可信 `user_id`；
- `username` 不参与路径、索引 key、Session ownership 或访问控制；
- Cloud 使用认证系统提供的 `user_id`；
- 未登录或关闭鉴权的本地运行使用稳定、合法的 `user_id = anonymous`，不得每次启动随机生成；
- `anonymous` 与其它 User 使用完全相同的隔离规则，可以拥有 Session、Workspace Docs、Memory 和状态数据；
- 登录后的真实 User 与 `anonymous` 是两个独立 owner，不自动合并或转移数据；
- request、Tool 输入和模型输出不能覆盖有效 `user_id`。

### 5.2 WorkspaceDefinition

WorkspaceDefinition 是配置层拥有的内部 Workspace 定义：

```text
WorkspaceDefinition
  workspace_id
  root_prefix
```

当前只有：

```text
WorkspaceDefinition {
  workspace_id: default
  root_prefix: ${TALOS_HOME}/workspace-talos
}
```

WorkspaceDefinition 不包含：

- `agent_id`；
- 用户状态；
- Session；
- Project；
- runtime client 或 filesystem handle；
- 用户可提交的任意路径。

WorkspaceDefinition 是顶层 `workspaces` 配置中的真实对象，不需要数据库持久化：

```yaml
workspaces:
  default_workspace_id: default
  definitions:
    - workspace_id: default
      root_prefix: workspace-talos
```

`root_prefix` 为相对路径时基于 `TALOS_HOME` 解析。配置加载必须校验 Workspace ID 非空、路径安全且唯一，默认 ID 必须存在，root prefix 不得不安全或相互重叠。代码中的 `default` 只作为默认配置值存在，业务链路必须从 `WorkspaceConfig.default_workspace_id` 查找定义。

### 5.3 EffectiveWorkspace

EffectiveWorkspace 是一个 WorkspaceDefinition 在可信 User scope 下的运行时解析结果：

```text
EffectiveWorkspace
  workspace_id
  user_id
  root
```

当前解析规则：

```text
WorkspaceDefinition(default).root_prefix
  + validated user_id
  -> ${TALOS_HOME}/workspace-talos/<user-id>
```

`root` 是解析结果，不是 Agent 配置，也不能进入客户端 request。Tenant 当前没有真实业务语义，不进入当前类型或目录；未来引入 Tenant 时再扩展该对象。

跨 Service、Coordinator、Agent Runtime、Prompt、Tool、Memory 和 History 传递时，必须携带同一个强类型 EffectiveWorkspace context，不得只传裸 `PathBuf` 后由各层重新推导。该对象只表达已授权的执行 scope，不承担 Workspace 注册、路由或生命周期管理。

### 5.4 AgentDefinition

AgentDefinition 是独立、可复用的执行能力定义：

```text
AgentDefinition
  agent_id
  name
  kind
  capabilities
  model policy
  prompt policy
  tools / skills / MCP
  runtime configuration
```

AgentDefinition 不拥有：

- Workspace root；
- `default_workspace_id`；
- User identity；
- User Memory；
- Session transcript；
- 当前执行目录。

同一个 AgentDefinition 可以注入多个 EffectiveWorkspace；多个 AgentDefinition 也可以注入同一个 EffectiveWorkspace。Agent 与 Workspace 的关系由 Session 产生，不存在直接 ownership。

### 5.5 Session

Session 是一次可持续对话的业务 owner，也是 User、Workspace 与 Agent 的稳定关联：

```text
Session
  session_id
  user_id
  workspace_id
  agent_id
  project_id?
```

Session 的 `workspace_id` 是必选、非空、不可变字段，当前规范值来自配置的 `default_workspace_id`。`project_id` 是条件必选绑定：普通 Session 可以为空，从 Project 上下文创建时必须非空、存在且属于同一 User。虽然用户不感知 Workspace，内部仍显式保存完整绑定，避免路径、配置默认值或 Agent 名称成为隐式关系。持久化 Session 从创建开始就是完整绑定，不增加只为表示客户端空白对话的 `binding_state`。

### 5.6 Project

Project 是可选的工作上下文和文件归属对象，不是 Workspace。Project Session 在创建时绑定 `project_id`，此后不能通过 Session Patch Set/Clear；切换 Project 必须创建新 Session。Project 的 `user_id` 必须与 Session 的可信 `user_id` 一致。

### 5.7 Turn

Turn 是 Session 下的一次执行：

```text
Turn
  turn_id
  session_id
  input
  lifecycle
  output
```

Turn 不拥有独立的 `agent_id` 或 `workspace_id` 选择权。执行所需的 User、Workspace 和 Agent 绑定全部来自 Session。

### 5.8 Tenant

Tenant 是未来 Cloud 隔离扩展，不是当前活跃对象。

当出现真实组织或租户需求时，关系扩展为：

```text
Tenant -> User -> EffectiveWorkspace
```

Tenant 必须来自可信认证和授权上下文，不能来自任意客户端路径输入。当前不创建 Tenant Store、不增加目录层，也不把所有用户放入名为 `default` 的虚构 Tenant。

## 6. 关系模型

```text
┌──────────────────────┐       ┌──────────────┐
│ WorkspaceDefinition  │       │ User         │
│ workspace_id=default │       │ user_id      │
│ root_prefix          │       │ username     │
└──────────┬───────────┘       └──────┬───────┘
           │                           │ trusted identity
           └─────────────┬─────────────┘
                         │ resolve
                         ▼
                ┌──────────────────────┐
                │ EffectiveWorkspace   │
                │ workspace_id         │
                │ user_id              │
                │ root                 │
                └──────────┬───────────┘
                           │ bound to
                           ▼
┌────────────────┐   ┌──────────────────────┐   ┌──────────────┐
│ AgentDefinition│◄──│ Session              │──►│ Project      │
│ agent_id       │   │ session_id           │   │ project_id   │
│ capabilities   │   │ user_id              │   │ user_id      │
│ runtime        │   │ workspace_id=default │   └──────────────┘
└────────────────┘   │ agent_id             │    optional binding
                     │ project_id?          │
                     └──────────┬───────────┘
                                │ 1:N
                                ▼
                         ┌──────────────┐
                         │ Turn         │
                         │ turn_id      │
                         │ session_id   │
                         └──────────────┘
```

Tenant 不进入当前关系图；未来引入时位于可信身份与 EffectiveWorkspace 解析之间，并由独立 Spec 定义。

基数关系：

```text
User                1 -> N Session
WorkspaceDefinition 1 -> N EffectiveWorkspace
EffectiveWorkspace  1 -> N Session
AgentDefinition     1 -> N Session
Project             1 -> N Session, optional binding
Session             1 -> N Turn
AgentDefinition     N -> N EffectiveWorkspace, through Session
```

## 7. Session 绑定生命周期

```text
client new conversation
  transient agent selection, no persisted Session
                       │ first Turn accepted
                       ▼
persist Session(user_id, workspace_id, agent_id, optional project_id)
  immutable binding for its whole lifetime
```

### 7.1 首条消息前

首条消息前的空白对话由客户端暂存：

- 当前由路由策略决定 `agent_id`；未来若开放用户选择，也只能修改尚未持久化的客户端草稿；
- `workspace_id` 由服务端读取 `WorkspaceConfig.default_workspace_id`，当前发布配置解析为 `default`；
- 用户不能直接提交 Workspace 选择；
- 不创建持久化 Session，不存在 active Turn、Turn artifact 或已提交对话内容。

### 7.2 Session 创建后

第一个 Turn 被接受并创建 Session 后：

- `user_id` immutable；
- `workspace_id` immutable；
- `agent_id` immutable；
- `project_id` immutable；
- Turn request 不允许覆盖 Agent 或 Workspace；
- 配置热更新不重绑定既有 Session；
- Agent 被禁用或 Workspace 配置失效时显式失败，不能 fallback 到其它 Agent 或 Workspace。

绑定建立时点是第一个 Turn 被系统接受并建立执行事实，而不是模型成功返回。首个请求若在建立执行事实前失败，可以删除本次新建 Session；执行事实一旦建立，即使 Turn 随后失败、取消或中断，也不得重绑定。

### 7.3 切换语义

既有 Session 需要更换 Agent、Workspace 或 Project 时：

```text
existing Session
  -> create or fork new Session
  -> resolve new binding
  -> accept first Turn
  -> new Session
```

不提供原地切换、历史搬迁或隐式重绑定。

## 8. Workspace 选择与配置

### 8.1 当前配置语义

当前只有一个 Workspace，但它是顶层配置中的真实定义：

```yaml
workspaces:
  default_workspace_id: default
  definitions:
    - workspace_id: default
      root_prefix: workspace-talos
```

配置层将相对 `root_prefix` 基于 `TALOS_HOME` 解析，因此当前结果为 `${TALOS_HOME}/workspace-talos`。`TALOS_HOME` 缺省时解析为 `$HOME/.talos`；不得在 `TALOS_HOME` 下再次增加 `.talos`。

当前选择规则固定为：

```text
WorkspaceConfig.default_workspace_id
  -> lookup WorkspaceDefinition
```

以下来源均不得选择 Workspace：

- Web、Desktop 或 TUI 用户输入；
- `StartTurnRequest`；
- retry request；
- Tool 参数；
- 模型输出；
- AgentDefinition；
- Agent routing result。

### 8.2 未来多 Workspace

未来增加多 Workspace 时，继续向 `workspaces.definitions` 增加定义，并由独立 Spec 扩展选择策略、权限和生命周期；不需要改变 Session 的必填 `workspace_id`。可能的内部选择依据包括 Tenant、组织、应用入口或 Project binding。只有出现动态成员、共享权限、配额或生命周期后才引入 Workspace Store。用户侧是否感知 Workspace 仍由届时产品设计决定，不能因为内部支持多个定义就自动暴露 UI 或 API 字段。

## 9. Agent 选择与注入

Agent 选择与 Workspace 选择是两条独立链路：

```text
Agent routing policy
  -> agent_id

Workspace policy
  -> workspace_id

Trusted identity
  -> user_id
```

Service 在接受首个 Turn 时将三者与可选的 `project_id` 组成稳定 Session binding。随后解析：

```text
WorkspaceDefinition + user_id
  -> EffectiveWorkspace

AgentDefinition + EffectiveWorkspace + Session + Turn
  -> AgentRun
```

Service 是 EffectiveWorkspace 的解析 owner，并将同一个强类型 context 注入 Coordinator 和 Agent dispatch。Agent Runtime、Prompt、Tool、Memory、Transcript 与 History 只消费该 context，不从 Agent config、Agent name、Workspace key 或当前工作目录自行猜测 Workspace 路径。

## 10. 数据归属与目录布局

当前规范目录为：

```text
${TALOS_HOME}/
├── upgrades/
│   ├── state.json                   Product Upgrade Task ledger
│   └── 202608290001_workspace_layout_v1/
│       └── source/                  Temporary legacy Workspace backup
└── workspace-talos/                 Workspace prefix
    └── <user-id>/                   EffectiveWorkspace root
        ├── profile.json
        ├── SOUL.md
        ├── IDENTITY.md
        ├── USER.md
        ├── HEARTBEAT.md
        ├── MEMORY.md
        ├── memory/
        └── sessions/
            └── <session-id>/
```

`${TALOS_HOME}/workspace-talos/SOUL.md`、`${TALOS_HOME}/workspace-talos/USER.md` 以及其它用户态 Markdown 均属于非法的 scope。Prompt bootstrap 只能接收已解析的 EffectiveWorkspace root，并且只允许在 `<user-id>/` 内自动创建文件。

`AGENTS.md` 继续属于 Project root context，不进入 User Workspace。当前没有 Agent-private Workspace 目录；如未来出现真实 Agent 私有持久状态，必须由独立设计定义，不能预留一个无消费者的 `agents/<agent-id>/` 目录。

目录所有权：

| 内容 | 业务 owner | 隔离 key |
| --- | --- | --- |
| Workspace Docs | User + Workspace | `user_id + workspace_id` |
| User Workspace Memory | User + Workspace | `user_id + workspace_id` |
| Dream Memory canonical files | User + Workspace | `user_id + workspace_id` |
| Session transcript/artifacts | Session | `session_id`，且 Session 已绑定 User/Workspace/Agent |
| Project files/context | Project + User | `project_id + user_id`，仅 Project Session 使用 |
| Agent definition/config | AgentDefinition | `agent_id`，不进入用户 Workspace |

通用 Workspace 数据不得因为写入者是某个 Agent 就自动增加 `agent_id` 隔离。`agent_id` 可以作为 producer、provenance 或审计信息保存，但 producer identity 不等于资源 ownership。

Dream Memory 的规范 Markdown 由 User + Workspace 唯一拥有。其 SQLite 索引是可重建的派生状态，目标索引 scope 不再以 `agent_id` 区分同一物理事实源；生成内容的 Session ID 写入 provenance，并可通过不可变 Session 绑定审计实际 `agent_id`。维护流程仍可依据 Session 的 `agent_id` 选择 formation policy，但不能改变资源 ownership。

如果未来确实存在 Agent 私有 Memory，必须定义独立的 `AgentWorkspaceScope` 和明确目录/Store，不得继续复用通用 User Workspace Memory scope。

## 11. 核心业务流程

### 11.1 首条消息前

```text
authenticated/local identity
  -> trusted user_id
  -> client holds transient new-conversation state
  -> routing policy chooses agent_id
  -> future user selection may only affect the transient draft
```

此时没有持久化 Session，也不解析或暴露 Workspace 选择。

### 11.2 接受第一个 Turn

```text
resolve trusted user_id
  -> resolve current valid AgentDefinition
  -> resolve default WorkspaceDefinition
  -> resolve EffectiveWorkspace for trusted user_id
  -> validate optional project_id exists and belongs to trusted user_id
  -> create Session with immutable user_id + workspace_id + agent_id + optional project_id
  -> accept first Turn and establish execution facts
  -> dispatch AgentRun with EffectiveWorkspace
```

并发首个请求必须通过现有幂等和 Session 创建边界收敛到一个绑定。若首个请求在建立执行事实前失败，删除本次新建 Session；一旦 transcript、active Turn 或等效执行事实建立，不再清理或重绑定。

### 11.3 后续 Turn

```text
load persisted Session
  -> authorize user_id
  -> load exact agent_id
  -> load exact workspace_id
  -> resolve EffectiveWorkspace
  -> dispatch Turn
```

后续请求只提供业务输入，不提供 Agent 或 Workspace override。

### 11.4 Subagent

Subagent 是独立子 Session：

- 必须继承父 Session 的 `user_id`；
- 必须继承父 Session 的 `workspace_id` 和同一个 EffectiveWorkspace；
- 当前实现继承调用方 `agent_id`，并继续拒绝模型通过 Subagent 参数提交 `agentId`；
- 未来若支持不同 Agent，只能由可信编排策略显式选择，并需独立定义权限与路由语义；
- 创建子 Session 时立即写入完整绑定；
- 后续不能更换 Agent 或 Workspace。

因此多个 Agent 可以在同一 EffectiveWorkspace 内通过不同 Session 协作，而不需要 Workspace 拥有 Agent。

### 11.5 Agent 或 Workspace 不可用

既有 Session 对应的 Agent 未注册、被禁用或 Workspace 配置缺失时：

- 明确返回不可用错误；
- 不路由到默认 Agent；
- 不解析到其它 Workspace；
- 不创建替代目录；
- 不静默读取其它 scope 的 transcript 或 Memory。

## 12. 部署形态

### 12.1 Local TUI

- 未登录时使用稳定 `user_id = anonymous`；
- 使用内部 `default` Workspace；
- 未登录用户有效目录为 `${TALOS_HOME}/workspace-talos/anonymous`；
- 不展示 Workspace 选择；
- Session binding 规则与 Server 相同。

### 12.2 Local Desktop

- 与 TUI 使用相同 identity 和 Workspace resolver；
- 在相同 `talos_home` 与配置下应看到相同 Session 和 Workspace 数据；
- 多本地账号登录时按各自稳定 `user_id` 隔离；
- Desktop 不自行拼接或覆盖 Workspace path。

### 12.3 Cloud Web Server

- 使用认证后的稳定 `user_id`；
- 当前使用内部 `default` Workspace；
- 所有 Session、Memory、Workspace Docs 和文件访问都必须先执行 owner authorization；
- 目录隔离只是存储布局，不代替鉴权和 Store 查询 scope；
- 远程客户端不接收或提交服务器绝对 Workspace path。

### 12.4 未来 Tenant Cloud

未来真实 Tenant 到来时：

```text
Auth Principal
  -> tenant_id + user_id
  -> WorkspaceDefinition
  -> ${TALOS_HOME}/workspace-talos/<tenant-id>/<user-id>
```

Tenant 需要独立 Spec 定义成员关系、跨用户共享、权限、迁移和审计，不能只增加目录层就宣称完成租户隔离。

## 13. API 与产品表面

用户产品表面不暴露 Workspace 概念：

- 首个 Start Turn 的 Session 创建链路不接收 `workspace_id`；
- Start、Retry、Resume 和 Dynamic Input request 不接收 `workspace_id` override；
- Workspace Docs 和 Memory API 自动使用当前可信 User 的 configured default Workspace；
- Web、Desktop 和 TUI 不显示 Workspace selector、Workspace ID、Workspace root 或 “Agent Workspace” 概念；
- SDK 不要求调用方理解 Workspace root 或路径布局；
- 当前公开 `workspace.info`、SDK 对应方法和 Web Settings 的绝对路径面板从用户表面删除；
- Workspace Docs、Memory 等现有能力在产品文案中按真实用户概念展示，例如 Personalization、Instructions 或 Memory，而不是暴露内部 Workspace 名称。

如果未来需要本地路径诊断，必须新建明确受控、仅本地可达的诊断表面；不能继续复用 Cloud 可调用的普通用户 API。

内部对象继续显式携带 `workspace_id`，用于 Session binding、scope 校验、日志和未来升级。内部字段存在不代表它必须进入用户 API。

## 14. 安全与隔离不变量

1. `user_id` 只来自可信 identity，不能由 request、模型或 Tool 覆盖。
2. `workspace_id` 是内部必选参数，只来自服务端 Workspace policy，当前读取 `WorkspaceConfig.default_workspace_id`；不得以缺省、空值或路径 fallback 代替。
3. Effective Workspace root 只由当前 WorkspaceDefinition 和 path-safe 的可信 `user_id` 派生。
4. Agent Runtime、Tool、Prompt、Memory 和 History 不自行拼接 Workspace root。
5. 所有 Workspace 文件访问必须限制在解析后的 Effective Workspace root 内。
6. 所有数据库查询必须带资源 owner scope；文件目录隔离不能替代授权。
7. Session 一经持久化，`user_id`、`workspace_id`、`agent_id` 和 `project_id` 不得因配置更新、Patch、重试、恢复或路由结果改变。
8. 同一物理 Workspace root 的共享必须是配置事实，不得因 Agent ID 不同制造相互矛盾的文件事实源。
9. 不存在或不合法的 Workspace 配置必须 fail fast，不得 fallback 到 Agent workspace 或进程当前目录。
10. `project_root` 和 `current_directory` 只控制工作目录语义，不能改变 durable Workspace ownership。
11. Workspace prefix 与 EffectiveWorkspace root 必须使用不同的强类型对象；会读写用户文件的接口不得接收未解析的裸 prefix 路径。
12. Workspace prefix 根目录不得作为 Prompt、Memory、History、Tool 或 Workspace Docs 的读写 scope。

## 15. Breaking Change 与迁移要求

目标态包含以下行为变化：

1. 默认目录从 `${TALOS_HOME}/workspace-<agent-name>/users/<user-id>` 收敛为 `${TALOS_HOME}/workspace-talos/<user-id>`。
2. `AgentConfig.workspace`、Agent Kernel 的 durable `workspace_dir` ownership 和 per-Agent `default_workspace_id` 不再作为 Workspace 来源。
3. 用户请求中的 `workspace_id` 和 per-Turn override 被删除。
4. Session 的 `workspace_id` 从可选上下文值收敛为内部必需绑定，历史空值回填 `default`。
5. 所有历史 Session 保留原有 `user_id` 和 `agent_id`，并回填 `workspace_id = default`；不新增 `binding_state`。
6. Session transcript、History、Prompt、Memory 和 Tools 必须统一使用 Session 解析出的 EffectiveWorkspace。
7. User Workspace Memory 和 Dream Memory 的通用 ownership 收敛为 `user_id + workspace_id`；Agent producer identity 作为 provenance，而不是默认资源隔离键。
8. 物理 Markdown 和 transcript 迁移必须先检测目标冲突，再执行可恢复搬迁；不得直接删除旧目录。
9. Memory SQLite 是派生索引，可以在规范文件迁移完成后按新 scope 重建。
10. 旧版本只有一个由顶层 `agent.name` 派生的物理 Workspace；新增的多 Agent definitions 不是历史目录发现来源，Upgrade 不得据此猜测多个 legacy roots。
11. Prefix 根目录已有的 `SOUL.md`、`USER.md` 等文件必须被识别为 scope 错误产物；运行时不得读取，也不得自动复制给任意 User。
12. 对上述错误产物的迁移必须可恢复并输出诊断；没有明确归属时在迁移期间保留于 Upgrade 备份，成功清理时迁至 `${TALOS_HOME}/recovered/<task-id>/`，不得复制到新 Workspace，也不得猜测归属到 `anonymous` 或首个用户。
13. `workspace.info`、生成的 App Protocol/SDK 类型与 Web Settings 路径面板删除，属于用户 API 和产品表面的 breaking change。
14. 新增顶层必需 `workspaces` 配置；默认配置 Upgrade 补齐该对象，显式外部配置缺失时 fail fast。
15. `session.patch` 删除 `project_id` Set/Clear，已有 Session 的 Project 绑定保持原值。

本 Spec 不要求保留新旧路径双读 fallback。实施 Plan 必须定义一次性迁移、失败恢复、冲突处理和验证顺序。

### 15.1 Upgrade owner 与执行时点

历史目录迁移由现有 `talos-upgrade` 启动升级边界统一拥有，不放入 Agent、Prompt、Memory、Workspace resolver 或 UI。它属于一次性 Runtime Upgrade：

```text
prepare config and migration resources
  -> load and validate config
  -> connect database
  -> run SQLx database migrations
  -> run registered product Upgrade Tasks
  -> initialize current Workspace directories
  -> build and publish application runtime
  -> accept user traffic
```

Local TUI、Local Desktop 和 Cloud Server 必须复用 `talos-upgrade::run_startup_upgrades`。该入口按固定顺序等待 database migration 与全部 product Upgrade Task 成功；任一步失败都停止启动，不得并行执行，也不得提前构建 Service 或 Agent Runtime。

配置加载阶段只能解析并校验配置，不得先创建 `users/anonymous` 或写入其它 Workspace 状态。旧代码只通过顶层 `agent.name` 派生唯一的 `${TALOS_HOME}/workspace-<agent-name>`，再在其下使用 `users/<user-id>`；Session 的 Agent、Workspace 和 Project 绑定始终属于数据库逻辑记录。Upgrade 必须复用这一真实旧语义，不遍历当前 `agent.agents`，也不扫描任意目录猜测多个历史 Agent root。

当前 Cloud 是单 Server 实例，本次只保证同一主机/共享 Talos Home 上的进程间互斥。未来多实例 Cloud 必须由部署层提供单升级实例或可靠的分布式协调；普通文件锁不能被描述为已解决跨主机升级问题。

配置模板与 migration resource 安装属于启动资源准备，不写入 Upgrade Task ledger。数据库 schema 迁移与 product Upgrade Task 是两套独立模型：前者由 SQLx `_sqlx_migrations` 记录脚本版本，后者由通用 Runner 记录稳定 task ID；两者不得复用或重复记录状态。

Session 表重建 migration 必须完整恢复 `sessions_cleanup_after_delete`，删除 Session 时同步清理 `timeline_events`、`active_turns`、`message_idempotency_keys` 和 `user_actions`，不得因后续 migration 重建 trigger 而丢失已有清理语义。

### 15.2 Upgrade Task 状态控制

不新增 Product Upgrade 数据库表，也不保存逐 User 的长期迁移记录。通用 Runner 使用单一状态文件：

```text
${TALOS_HOME}/upgrades/state.json
```

状态文件必须由启用 `deny_unknown_fields` 的强类型 `UpgradeState` / `UpgradeTaskState` 对象读写，禁止使用自由 JSON。`product_version` 表示最后一次全部升级成功的 Talos 产品版本，`tasks` 记录实际进入执行阶段的任务状态：

```json
{
  "schema_version": 1,
  "product_version": "0.1.8",
  "tasks": {
    "202608290002_dream_memory_okf": {
      "revision": 1,
      "target_version": "0.1.7",
      "executed_version": "0.1.8",
      "status": "success",
      "updated_at": 1787961600,
      "cleanup": { "status": "completed", "updated_at": 1787961600 }
    },
    "202608290001_workspace_layout_v1": {
      "revision": 1,
      "target_version": "0.1.8",
      "executed_version": "0.1.8",
      "status": "success",
      "updated_at": 1787961600,
      "cleanup": { "status": "completed", "updated_at": 1787961600 }
    }
  }
}
```

每个任务必须具有不可变的稳定 ID 和正整数 revision，例如：

```text
id: 202608290001_workspace_layout_v1
revision: 1
target_version: 0.1.8
priority: 100
```

`talos-upgrade::lib.rs` 只负责任务组装和启动编排：先等待独立的 `DatabaseMigrationRunner`，再由 `ProductUpgradeRunner` 执行 Product Task，全部成功后才初始化 Workspace 并允许 Runtime 组装。所有 Runner 实现统一 `UpgradeRunner::run() -> typed report` 接口；Database Runner 和 Product Runner 保持各自强类型报告与独立 ledger，不增加动态 Manager 或自由 JSON envelope。

所有 Product Task 实现统一的 `UpgradeTask` 接口，暴露 `definition()`、`execute(context)` 和 `cleanup(context)`。当前函数式 Task 通过 `UpgradeTaskRegistration` 适配该接口；具体 Dream Memory 和 Workspace 实现只处理业务数据，不读取或写入 ledger，不判断版本、状态或重试。版本过滤、排序、文件锁、日志、`running/failed/success`、失败重试和强类型 JSON 持久化全部由 `ProductUpgradeRunner` 负责。

Runner 串行处理并遵守以下规则：

- `target_version`、`priority` 是 Upgrade Task 框架层必填字段，版本使用强类型 SemVer；Runner 按 `target_version + priority + task ID` 得到确定顺序，不依赖调用方注册顺序；
- 稳定版本仅执行 `installed_version < target_version <= current_version` 的任务；目标版本高于当前发布线时不得执行；
- 开发版本统一使用 `<release>-SNAPSHOT`。框架将 `0.1.8-SNAPSHOT` 的 Task 选择上限解释为同发布线的 `0.1.8`，因此开发阶段可以真实执行 `0.1.8` Task；`product_version` 和 `executed_version` 仍记录实际程序版本 `0.1.8-SNAPSHOT`，正式切换为 `0.1.8` 后相同成功 Task 按 ledger 跳过，不修改 Task 的稳定 `target_version`；
- Upgrade Runner 首次引入于 `0.1.7`：检测到真实旧 Workspace 时以 `0.1.6` 建立迁移起点；没有 legacy Workspace 的新安装直接将 `product_version` 初始化为当前版本，不执行历史任务；
- 已存在相同 task ID/revision/target version 的完成记录：跳过；
- Task 开始执行前写入 `running`，失败更新为 `failed`，成功更新为 `success`；Task State 同时记录 revision、`target_version`、`executed_version` 和更新时间；
- 重启时只有 `success` 跳过，`running` 或 `failed` 均按原 Task 幂等重试；任一任务失败时不推进 `product_version`，全部区间任务成功后才推进到当前版本；
- 已完成 task 的 revision 或 target version 与代码不一致、状态文件损坏、产品版本非法或任务 ID 重复：fail fast；
- 已发布任务不得原地修改迁移语义；后续数据变更增加新的稳定 task ID；
- `.DS_Store` 等明确列出的系统文件不作为 User 数据；位于旧 `users/` 下时跳过，位于备份根时在成功清理中删除。
- Workspace Layout Task 被版本区间选中但不存在 legacy source/backup 时成功 no-op，用于支持全新 `0.1.7 -> 0.1.8`；不得把“没有历史数据”当作迁移冲突。
- 当前程序版本低于 Workspace Layout Task 且仍检测到旧 `users/<user-id>` 布局时，允许更早版本 Task 完成并记账，但必须在 Runtime 初始化前明确停止启动，禁止旧数据尚未迁移就按新路径提供服务。

默认初始化未登录用户 Workspace 的逻辑必须保留：先完成 database migration 与全部 Upgrade Task，再创建 `${TALOS_HOME}/workspace-talos/anonymous`。不得在任务检测前创建旧路径 `users/anonymous`，否则会把新安装误判为 legacy 安装。

Runner 在进程间文件锁内执行任务，并在每个任务成功后用临时文件、fsync 和原子 rename 更新状态。数据库 schema 和 Session binding 回填继续由 SQLx migration ledger 管理；Product Runner 不复制 SQL migration 状态。

Dream Memory legacy→OKF 转换是 `0.1.7` Product Upgrade Task，必须在旧 `workspace-<agent>/users/<user>` 目录结构仍存在时先执行；Workspace Layout 是 `0.1.8` Product Upgrade Task，随后整体 rename 旧 Workspace 并按新目录规范重建。Runtime 组装阶段只能执行当前格式索引重建，不得再次扫描或转换 legacy 数据。

启动日志必须记录总流程开始/完成、SQLx 本次新增与累计 migration、产品升级前后版本，以及每个 Product Task 的 `applied`/`skipped`/`deferred`/`failed`、task ID、revision、target version 和耗时；不得记录用户文件内容。

### 15.3 一次性迁移流程

1. **Database migration**：SQLx 根据 `_sqlx_migrations` 识别待执行脚本并串行完成，历史 Session binding 回填属于该阶段。
2. **Lock and load**：对 `${TALOS_HOME}/upgrades/.lock` 获取进程间排他锁并读取通用 task state。
3. **Resolve product interval**：读取最后成功产品版本；state 不存在时，仅使用稳定备份或顶层 `agent.name` 推导的唯一 legacy source 区分 `0.1.6` 旧安装与当前版本新安装。
4. **Convert Dream Memory**：由 `0.1.7` Task 在旧 `users/<user-id>` 结构中完成 legacy→OKF 转换；它不感知 Upgrade State、重试或产品版本。
5. **Rename source**：由 `0.1.8` Workspace Task 将整个 legacy Workspace 原子 rename 到 `${TALOS_HOME}/upgrades/202608290001_workspace_layout_v1/source`，然后才创建配置定义的新 Workspace prefix。备份已存在表示上次迁移中断，直接进入恢复流程。
6. **Migrate users**：只从稳定备份中的每个合法 `users/<user-id>` 重建 `<prefix>/<user-id>`，不依赖 Session DB 枚举用户；`users/anonymous` 作为合法未登录用户数据正常迁移。
7. **Preserve unowned state**：旧根目录中的 `SOUL.md`、`USER.md` 等无归属内容在迁移期间保留在完整备份中；成功清理时迁至 `${TALOS_HOME}/recovered/<task-id>/`，不复制到新 prefix，也不猜测 User owner。
8. **Commit product version**：每个 Task 按 `running -> failed|success` 原子更新强类型状态；所有区间任务均为 `success` 后推进 `product_version`，随后在同一升级锁内清理成功任务的过程数据，完成后任务目录为空或不存在；最后初始化当前 Workspace 并允许 Runtime 继续组装。

### 15.4 幂等、冲突与恢复规则

Upgrade 函数必须根据 source 与 target 的实际状态幂等恢复，不依赖逐目录状态表：

- backup 不存在且检测到 legacy source：先整体 rename，禁止在仍被扫描的 source 内创建 target；
- backup 已存在：视为中断恢复，不再次 rename 新目标；
- target 不存在：从稳定 backup 按稳定顺序重建；
- target 已存在且对应内容完全一致：视为该部分已完成，继续处理其余目录；
- target 已存在但内容不同：停止 Upgrade，不自动 merge、覆盖或选择较新文件；
- 进程崩溃：task 完成状态不写入；完整 backup 仍在，下次启动对已复制的相同文件幂等核对并继续；
- 无法确认 owner 的根文件：清理时迁至 recovered，不分配给 `anonymous`、首个登录用户或默认 Agent；
- Upgrade 失败：保留完整 backup 和已完成的相同 target，明确阻止启动，不回退读取旧路径。

过程备份清理由各 Task 实现并由 Runner 自动调度；失败保留待重试状态，不回退已完成的升级。

### 15.5 Upgrade Task 清理生命周期

- `UpgradeTask.cleanup(context)` 由各任务实现，只处理本任务过程产物；Runner 负责状态、调用时机及错误报告。清理必须幂等，不读取或写入账本。
- Runner 在全部当前适用任务成功、产品版本持久化后执行清理。任务的 `cleanup` 独立记录 `pending | completed` 与 `updated_at`；旧记录缺字段时按 pending 处理，时间沿用任务时间。新安装基线任务直接 completed。
- 新任务执行前随 running 持久化 pending；旧记录缺字段即为 pending。清理前不重复写入，完成删除后才持久化 completed。失败保留 pending 并报告，继续其他清理及启动；下次即使跳过 execute 也重试 cleanup。状态写入使用候选副本，失败不污染后续保存。失败、运行中、未执行或超出当前版本的任务不清理。
- Dream Memory 保留每用户转换末尾的 finish，删除 staging/旧格式备份后才允许后续 Workspace 搬迁；Task.cleanup 无额外操作。
- Workspace Layout 的 success 表示 users 已完整复制或核对。迁移目标文件与目录同步落盘后才提交成功。清理不比较可能已正常修改的新文件；删除 users 副本及系统杂项，将根目录其他原始内容保留相对路径迁至 recovered。恢复目标冲突时停止该任务清理，不覆盖；迁出目录同步后才删除 source，任务目录存在未知产物时保留并报告。
- History Repair 在文件替换及数据库事务提交、全部任务成功后删除 source、database.sqlite、database.sqlite.partial 和任务目录。数据库备份是升级过程保护数据，不是长期回滚快照；已修复数据库不再含旧历史表，不会再次触发备份。部署回滚所需的一致性备份仍由部署流程负责。
- `upgrades/state.json`、`upgrades/.lock` 和 SQLx 账本永久保留。completed 表示本任务过程数据清完；recovered 中的唯一原始数据不属于过程数据。

## 16. 成功标准

1. AgentDefinition 不再拥有 Workspace path 或 Workspace default。
2. 当前所有新 Session 内部绑定 `workspace_id = default`。
3. 用户协议和 UI 不需要提交或选择 Workspace。
4. 首条消息前不创建持久化 Session；Session 创建后 User、Workspace、Agent 和 optional Project 绑定不可修改。
5. 同一 User、同一 Workspace、不同 Agent 能读取同一 Workspace Docs 和共享 Memory。
6. 同一 Agent 在不同 User scope 下不能串读 Workspace 数据。
7. Transcript 写入、History 读取、Memory、Prompt 和 Tool 使用完全相同的 EffectiveWorkspace root。
8. TUI、Desktop 和 Cloud Server 使用同一业务解析规则。
9. 既有 Session 的 Agent 或 Workspace 不可用时明确失败，不发生 fallback。
10. 当前目标不新增 Workspace 数据库表、用户 Workspace UI 或 Tenant 实现。
11. `${TALOS_HOME}/workspace-talos` 根目录不存在被运行时创建或消费的用户态文件，bootstrap 文件只存在于 `<user-id>/` 下。
12. 历史布局只通过一次性、可恢复的 `workspace-layout-v1` Upgrade 迁移；完成后正常运行时不再扫描、双读或写入旧路径。
13. 未登录模式继续使用并初始化 `anonymous` User Workspace，但目标路径为 `${TALOS_HOME}/workspace-talos/anonymous`。
14. 用户产品表面不再显示 `workspace.info`、绝对 Workspace 路径或 “Agent Workspace” 概念。
15. 不新增持久化 `Draft/Bound` 状态、Workspace Registry、Workspace 数据表或 Product Upgrade 数据库表；Workspace 定义统一来自顶层 `workspaces` 配置。

## 17. Review 清单

- Workspace 是否仍被任何 Agent config、Agent name 或 runtime config 隐式拥有？
- 用户协议是否仍暴露 `workspace_id` 或允许 per-Turn override？
- 首条消息前是否保持为客户端临时状态，持久化 Session 是否从创建起具备完整且不可变的绑定？
- Subagent 是否继承父 Workspace，同时保留独立 Agent binding？
- Workspace Docs、User Workspace Memory、Dream Memory 和 transcript 是否使用同一 scope？
- Agent-private state 是否被明确区分，而不是混入通用 Workspace ownership？
- Local TUI、Desktop 与 Cloud 是否使用同一 resolver 和不变量？
- 是否新增了当前没有真实调用方的 Workspace Store、Tenant 层或管理 API？
- 是否把 Project、current directory 或 filesystem path 错当成 Workspace identity？
- 迁移是否避免双读 fallback、重复复制和不可恢复覆盖？
