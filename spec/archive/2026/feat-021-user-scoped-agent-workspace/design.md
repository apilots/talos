# User Scoped Agent Workspace

Status: archived
Archived: 2026-07-14

## 背景与问题

Talos 当前的 agent workspace 主要由 `agent.kernel.memory.workspace_dir` 指向一个静态目录。`MemoryStore`、workspace docs、prompt bootstrap、memory API，以及部分工具在缺少 project root 时都会从这个目录读写文件。

随着 WebUI、Desktop、HTTP、SDK 都进入真实用户鉴权路径，同一个 Talos runtime 可能服务多个用户。如果 Memory、Instructions、Workspace Docs 继续共享同一个静态 workspace，会导致不同用户之间的 durable memory、个人指令和会话 transcript 串读串写。

当前系统已经具备用户归属基础：

- `RequestIdentity` 包含由 cloud/auth identity 提供的稳定 `user_id` 和展示/登录用 `username`。
- `users` 表已保证 `user_id`、`username` 唯一。
- session 创建时 access 层会把 actor 的 `user_id` 写入 `TalosContext`。
- `TalosSession` 和 session store 已持久化 `user_id`。
- `ToolContext` 已包含 `user_id`，且 memory tools 通过注入的 `MemoryStore` 工作。

因此本 spec 目标不是给 tools 增加用户参数，而是把 agent workspace 解析从静态配置改为 request/session scoped 的运行时派生。

## 目标

- Memory、Instructions、Workspace Docs、session transcript 按 `user_id` 隔离存储。
- 展示层继续使用 `username`，但文件系统路径、缓存 key 和持久化隔离均使用 `user_id`。
- open mode 固定使用 `anonymous` 作为用户 ID。
- agent 执行过程中从 session owner 派生 effective user workspace，并注入到 kernel config、MemoryStore、ToolContext。
- 保持 `MemorySearch` / `MemoryGet` 等模型可调用 tool schema 不暴露 `user_id`。
- WebUI 与 Desktop 共享同一行为；Desktop packaged 形态不得退回共享 workspace。
- `user_id` 的唯一来源是 cloud/auth identity；Talos runtime 不本地生成真实用户 ID，不做 fallback。

## 非目标

- 不支持用 `username` 作为真实存储目录。
- 不在本 spec 中设计 username rename。
- 不在 tool 输入 schema 中加入 `user_id`、`username` 或 workspace path。
- 不引入独立权限模型；继续复用 `RequestIdentity`、`Actor`、session owner 校验。
- 不改变 Memory search 的语义检索能力。
- 不为缺失或非法 `user_id` 创建替代用户目录。

## 当前实现事实

| 领域 | 当前事实 | 影响 |
| --- | --- | --- |
| Auth | `RequestIdentity` 同时有 `user_id` 和 `username` | 可以用 `user_id` 做稳定存储 key |
| User storage | `users.user_id`、`users.username` 均唯一 | username 可继续做登录/展示名 |
| Session | session 创建时写入 actor `user_id` | session 可以作为 agent 执行的用户归属来源 |
| Agent | `AgentHarness` 当前持有 agent 级 `memory_store` | 多用户下需要改为 per-user/per-turn effective store |
| Tool context | `ToolContext` 已有 `user_id` 和可注入 `MemoryStore` | tools 适合保持无感 |
| Workspace docs | `put_doc` 有 actor 参数但当前未用于路径解析，读接口无 actor | 需要读写统一 actor scoped |
| Memory API | `put_file` 有 actor 参数但当前未用于路径解析，读/search 无 actor | 需要读写统一 actor scoped |
| Cloud user ID | cloud server 当前生成 `user_<uuid-simple>`；Talos runtime 只消费 cloud/auth identity 提供的 user_id | 不本地生成真实 user_id；格式不符合 path-safe 规则即报错 |
| File tools | 无 project root 时 fallback 到 `kernel_config.workspace_dir` | effective config 必须已是用户目录 |

## 设计决策

### 1. Session Owner 是执行期用户来源

每次 agent 执行必须基于已加载并已通过 access 层校验的 `TalosSession`。运行时使用 `session.user_id` / `session.context.user_id` 作为 workspace 派生输入。

如果 `session.user_id` 与 `session.context.user_id` 不一致，应 fail fast，返回内部一致性错误，不继续执行 agent。

禁止从前端请求参数、tool 参数、metadata override 中读取用户 ID 来决定 workspace。

除 open mode 的固定 `anonymous` 外，`user_id` 的唯一来源是 cloud/auth identity。Talos runtime 不得在鉴权失败、identity 缺失或 user_id 非法时生成 fallback user_id。任何缺失、空值或路径不安全的 `user_id` 都应 fail fast。

### 2. 新增 `agent.kernel.workspace_dir` 作为 Prefix

新增 `agent.kernel.workspace_dir`，用于表达 agent workspace prefix。`agent.kernel.memory.workspace_dir` 不再作为对外配置入口，避免把 memory 子系统目录和 agent workspace 根目录混在一起。

```text
base agent workspace prefix:
~/.talos/workspace-talos

effective user workspace:
~/.talos/workspace-talos/users/{user_id}
```

运行时派生出的 effective kernel config 中，`agent.kernel.workspace_dir` 指向当前用户目录。memory、prompt bootstrap、workspace docs 和 file tool fallback 均从 effective `agent.kernel.workspace_dir` 派生各自路径。

`agent.kernel.memory.workspace_dir` 字段从运行时配置模型中移除。旧配置文件中显式出现该路径时应被配置校验拒绝，不能反序列化为有效运行时输入。

### 3. Open Mode 固定 anonymous

当 `gateway.auth=false` 或其它 open mode 路径注入 `RequestIdentity::auth_disabled()` 时，用户 ID 固定为：

```text
anonymous
```

open mode 的 effective workspace 为：

```text
~/.talos/workspace-talos/users/anonymous
```

这表示同一个 Talos runtime 的 open mode 使用共享匿名 workspace。它不是登录用户身份，也不能绕过真实 auth mode 下的用户隔离。

### 4. Username 只用于展示

`username` 只用于 UI 展示、登录输入、account/session label。不得参与：

- workspace path 派生
- memory index key
- session transcript path
- access control 判断
- tool execution context 的存储隔离

如果未来支持 username rename，只需更新用户资料和展示层，不迁移 workspace。

每个用户 workspace 需要写入一个 profile snapshot，方便人工排查目录归属：

```text
users/{user_id}/profile.json
```

建议结构：

```json
{
  "user_id": "user_0123456789abcdef0123456789abcdef",
  "username": "alice",
  "updated_at_ms": 1780000000000
}
```

`profile.json` 是诊断辅助文件，不参与鉴权判断，不作为 username 的权威来源。每次登录、获取 current user、写 workspace docs 或执行 agent 前可以 opportunistic 更新该文件。

### 5. Admin 不跨用户管理 Workspace Docs

Workspace markdown 管理能力仅面向当前个人用户。Admin 账号可以管理用户账号，但不通过 Agent Workspace UI 或 API 读取、编辑其它用户的 Memory / Instructions / Workspace Docs。

这保持个人 memory 的隐私边界，也避免把 admin 权限扩展成跨用户 durable context 读取权限。

### 6. Tools 不感知用户目录

`MemorySearch`、`MemoryGet`、file tools 仍通过 `ToolContext` 和 effective `kernel_config` 获取执行环境。

模型看到的 memory path 仍保持相对路径：

```text
MEMORY.md
memory/2026-06-08.md
```

不暴露：

```text
users/{user_id}/...
~/.talos/...
```

## 运行流程

```text
HTTP / App Server / Desktop Request
  |
  v
Auth middleware / app actor resolver
  |
  v
RequestIdentity { user_id, username, role, scopes }
  |
  v
Actor { user_id, access }
  |
  v
ResourceAccessService verifies session owner
  |
  v
TalosSession { user_id, context.user_id }
  |
  v
EffectiveWorkspaceResolver
  |
  +--> effective workspace path:
  |    {base_workspace_prefix}/users/{user_id}
  |
  +--> effective AgentKernelConfig
  |    workspace_dir = effective workspace path
  |
  +--> MemoryStore
  |
  v
Agent execution / general_turn
  |
  v
ToolContext { user_id, memory_store, kernel_config }
  |
  v
Tools read/write only current user's effective workspace
```

## 核心 API 伪代码

```rust
pub struct EffectiveWorkspaceResolver {
    base_config: TalosConfig,
}

pub struct EffectiveAgentWorkspace {
    pub user_id: String,
    pub base_workspace_dir: PathBuf,
    pub user_workspace_dir: PathBuf,
    pub config: TalosConfig,
}

impl EffectiveWorkspaceResolver {
    pub fn resolve_for_session(
        &self,
        session: &TalosSession,
    ) -> TalosResult<EffectiveAgentWorkspace> {
        ensure_session_user_consistency(session)?;
        self.resolve_for_user_id(&session.user_id)
    }

    pub fn resolve_for_actor(
        &self,
        actor: &Actor,
    ) -> TalosResult<EffectiveAgentWorkspace> {
        self.resolve_for_user_id(&actor.user_id)
    }

    fn resolve_for_user_id(&self, user_id: &str) -> TalosResult<EffectiveAgentWorkspace> {
        let safe_user_id = sanitize_user_workspace_segment(user_id)?;
        let base_workspace_dir = resolve_runtime_path(
            &self.base_config.agent.kernel.workspace_dir,
        );
        let user_workspace_dir = base_workspace_dir.join("users").join(safe_user_id);

        let mut config = self.base_config.clone();
        config.agent.kernel.workspace_dir =
            user_workspace_dir.to_string_lossy().to_string();

        Ok(EffectiveAgentWorkspace {
            user_id: user_id.to_string(),
            base_workspace_dir,
            user_workspace_dir,
            config,
        })
    }
}

fn ensure_session_user_consistency(session: &TalosSession) -> TalosResult<()> {
    if session.user_id != session.context.user_id {
        return Err(TalosError::InternalError(
            "session user_id does not match session context user_id".to_string(),
        ));
    }
    Ok(())
}
```

## 模块接口清单

| 模块 | 变更 | 说明 |
| --- | --- | --- |
| `talos_config::TalosPaths` | 新增或调整 agent workspace prefix helper | `workspace_dir_for_agent(name)` 保持返回 prefix，不直接包含 user |
| `AgentKernelConfig` | 新增 `workspace_dir` | `agent.kernel.workspace_dir` 是统一 workspace root，memory 等子系统从这里派生 |
| `application-service` | 新增 `EffectiveWorkspaceResolver` | 统一从 actor/session 派生用户 workspace |
| `WorkspaceDocService` | `list_docs/get_doc/put_doc` 均接收 actor 或 effective workspace | 读写路径一致按 user scoped |
| `MemoryService` | `list_files/get_file/search_files/put_file` 均接收 actor 或 effective workspace | 避免只有写隔离、读共享 |
| `AgentHarness` | 不再使用单例 `self.memory_store` 执行所有用户 turn | 每次 run 按 session owner 创建或缓存 per-user `MemoryStore` |
| `general_turn` | 继续接收 `MemoryStore` 和 `AgentKernelConfig` | 不感知全局/用户路径差异 |
| `ToolContext` | 继续携带 `user_id`、`memory_store`、`kernel_config` | tool schema 不变 |
| HTTP/App protocol | 不新增 user_id 请求字段 | user_id 来自 auth/session，不来自客户端输入 |
| WebUI/Desktop | 展示 username，调用现有 workspace docs/memory API | API 返回内容已按当前用户隔离 |

## 路径与文件模型

```text
~/.talos/
  workspace-talos/
    users/
      anonymous/
        profile.json
        SOUL.md
        IDENTITY.md
        USER.md
        HEARTBEAT.md
        MEMORY.md
        memory/
          2026-06-08.md
          2026-06-08.index.json
        sessions/
          {session_id}/
            2026-06-08.jsonl
      usr_01HX...
        profile.json
        SOUL.md
        IDENTITY.md
        USER.md
        HEARTBEAT.md
        MEMORY.md
        memory/
        sessions/
```

`user_id` path segment must be validated as path-safe. 除 open mode 固定 `anonymous` 外，Talos 只接受 cloud/auth identity 提供的 `user_id`，不得自行生成或修复。Cloud server 当前使用 `UuidIdGenerator` 生成 `user_<uuid-simple>`，例如：

```text
user_0123456789abcdef0123456789abcdef
```

该格式只包含 ASCII 字母、数字和 `_`，可以直接作为目录名。Talos 本地仍必须执行 path-safe 校验；校验失败说明上游 identity 不符合 Talos runtime 要求，应报错并停止请求。Recommended policy:

- Allow ASCII alphanumeric, `_`, `-`.
- Reject empty value.
- Reject unsupported characters and fail fast.
- 不做字符替换，避免两个不同 user_id 被替换成同一个目录。
- 不创建 fallback user_id，不落到 `anonymous`，除非当前请求本身处于 open mode。

## 兼容与迁移

项目尚未上线，当前阶段优先最佳方案，不在 runtime 中固化历史迁移逻辑。`agent.kernel.workspace_dir` 是 workspace prefix，只允许承载 `users/` 等容器目录；不应在 prefix 顶层生成 workspace docs、memory 或 sessions。

开发期本机若存在旧共享 workspace，可手工整理以下旧路径：

```text
~/.talos/workspace-talos/SOUL.md
~/.talos/workspace-talos/IDENTITY.md
~/.talos/workspace-talos/USER.md
~/.talos/workspace-talos/HEARTBEAT.md
~/.talos/workspace-talos/MEMORY.md
~/.talos/workspace-talos/memory/
~/.talos/workspace-talos/sessions/
```

手工整理目标：

```text
~/.talos/workspace-talos/users/anonymous/
```

处理规则：

- runtime 不自动 move、copy、merge 旧文件。
- 不提供正式迁移命令或脚本。
- 新请求只读写 `users/{user_id}`。
- 测试生成的 user_id 需显式标记为测试身份，例如 `talos-test-*`。

## 影响范围

### 后端

- Auth disabled/open mode identity 固定为 `anonymous`。
- App-server 与 HTTP handler 继续从 auth extension / app actor 获取 actor。
- Session access 层继续保证 actor 只能访问自己的 session。
- Workspace docs 和 memory files API 从 actor/session 派生目录。
- auth mode 下缺失或非法 `user_id` 直接拒绝请求，不 fallback 到 anonymous。

### Agent Kernel

- Kernel 不需要知道用户名。
- Kernel 接收的 `AgentKernelConfig.workspace_dir` 已是 effective user workspace。
- Prompt bootstrap 从当前用户 workspace 加载 `SOUL.md`、`USER.md`、`MEMORY.md` 等。

### Tools

- `MemorySearch`、`MemoryGet` 无 schema 变化。
- file tools 在没有 project root 时 fallback 到当前用户 workspace。
- tool output 中继续返回相对路径。

### UI

- Settings -> Agent Workspace 展示当前登录用户自己的 docs。
- Account/session/chats 展示 username。
- Admin 看不到其它用户的 Agent Workspace markdown 管理入口。
- open mode 可以显示为 anonymous 或 open mode user，但不能暗示真实登录身份。

## 验证策略

### 单元测试

- `EffectiveWorkspaceResolver`：
  - `user-1` 解析到 `workspace-talos/users/user-1`。
  - `anonymous` 解析到 `workspace-talos/users/anonymous`。
  - session `user_id` 与 `context.user_id` 不一致时报错。
  - 非 path-safe user ID 报错，不做替换。
  - cloud 生成格式 `user_<32hex>` 可通过校验。
  - auth mode 下缺失 user ID 不创建 fallback。
- `WorkspaceDocService`：
  - user A 写 `MEMORY.md`，user B 读不到。
  - user A 与 user B 可各自写同名 doc。
  - admin 不能通过 workspace docs API 读取 user B 的 docs。
- `MemoryService`：
  - user A 写 `memory/2026-06-08.md`，user B list/get/search 不可见。
- `AgentHarness`：
  - 同一个 agent 连续执行 user A 和 user B session，不共享 `MemoryStore`。
  - 执行前写入或更新 `users/{user_id}/profile.json`。

### 集成测试

- HTTP auth mode：
  - 注册两个用户。
  - 用户 A 写 Agent Workspace instructions。
  - 用户 B 登录后读 Agent Workspace，内容为空或为 B 自己内容。
  - 两个用户分别触发 memory flush，memory search 只返回自己的结果。
- App-server/Desktop：
  - Desktop packaged app 登录用户后 Settings Agent Workspace 展示该用户内容。
  - 退出登录/切换用户后不显示前一个用户的 docs。
- Open mode：
  - `gateway.auth=false` 下 memory/docs 固定写入 `users/anonymous`。
  - open mode 不访问真实登录用户目录。

### 回归测试

- `talos cli` 通过 server e2e。
- memory search 仍使用语义检索，不降级纯文本。
- prompt bootstrap 仍加载 workspace docs。
- file tools 在 project root 存在时优先 project root，不被 user workspace 覆盖。

## 风险与待评审问题

- 旧 `agent.kernel.memory.workspace_dir` 只作为非法配置路径被拒绝，不再作为运行时配置字段存在。
- `profile.json` 是辅助快照，若写入失败是否阻断 agent 执行需要实现阶段决定。建议记录 warning，不阻断核心请求。
- 开发期本机旧共享 workspace 若与新 `users/anonymous` 内容冲突，需要人工确认后整理，runtime 不负责迁移。
- cloud 当前生成 `user_<32hex>`；Talos 本地 path-safe 校验必须保留。任何不符合的 auth identity 都是错误，不允许 fallback。

## 成功标准

- 任意用户的 Memory、Instructions、Workspace Docs、session transcript 均落在 `users/{user_id}` 下。
- 每个用户 workspace 包含可读的 `profile.json` username 快照。
- username 改名不会影响已有 workspace。
- auth mode 下所有 workspace user_id 均来自 cloud/auth identity，Talos 不生成 fallback user_id。
- `MemorySearch` / `MemoryGet` tool schema 不变。
- open mode 始终使用 `users/anonymous`。
- Admin 不提供跨用户 Workspace Docs 管理能力。
- runtime 不在 workspace prefix 顶层生成 MD、memory 或 sessions。
- HTTP、App-server、WebUI、Desktop 对同一用户看到一致的 workspace 内容。
