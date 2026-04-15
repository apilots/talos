# Project Root And Workspace Semantics Design

## 背景

Talos 当前同时存在两类目录语义：

- `directory`：TUI/CLI 当前运行目录，代表用户正在操作的项目目录
- `workspace_dir`：Talos 服务端自身的工作目录，用于 memory、session flush、临时产物等内部数据

这两个概念天然不同，但现有实现里：

- `directory` 已经被放进 session environment，并出现在 prompt / UI 中
- 工具默认根目录仍主要取 `kernel_config.memory.workspace_dir`

结果是：

- UI 说“当前目录是 A”
- prompt 说“当前目录是 A”
- `grep_files/list_dir/read_file/exec_command` 默认实际跑在 B

这会让模型看起来像“乱选工具”，但根因是默认根目录语义不一致。

## 目标

本设计目标是把目录语义拆清，并给出稳定的默认执行规则：

- `project_root`：当前用户任务默认分析/操作的项目目录
- `workspace_dir`：Talos 内部工作目录
- `workspace_id`：逻辑工作空间标识，不等于路径

并按连接模式定义默认行为：

1. Embedded / local embedded
   - 默认 `project_root = TUI 当前启动目录`
   - 模型仍可通过显式绝对路径切换分析目标
2. Remote
   - 默认不能信任客户端本地 cwd
   - 需要用户手动选择一个服务端路径作为 `project_root`
   - 若未设置，则默认回落到 `workspace_dir`
   - 若默认路径不存在或不可访问，明确告知用户指定服务端目录

## 核心概念

| 概念 | 含义 | 生命周期 | 是否等于文件路径 |
|---|---|---|---|
| `project_root` | 当前任务默认分析/操作的项目目录 | session-scoped，可切换 | 是 |
| `workspace_dir` | Talos 内部工作目录 | process/config-scoped | 是 |
| `workspace_id` | 逻辑工作空间标识 | session-scoped | 否 |
| `directory` | 客户端本地运行目录，仅描述客户端环境 | client/session-scoped | 是 |

## 设计原则

1. `directory` 不等于 `project_root`
2. `project_root` 不等于 `workspace_dir`
3. `workspace_id` 不等于任何路径
4. 项目类工具默认使用 `project_root`
5. Talos 内部工具默认使用 `workspace_dir`
6. 显式绝对路径优先级最高
7. 不允许 silent fallback 到错误根目录

## 连接模式语义

### 1. Embedded

规则：

- `client directory` 由 TUI 当前启动目录提供
- 默认 `project_root = client directory`
- 若模型或用户显式给出绝对路径，则按该路径执行
- `workspace_dir` 继续仅用于 Talos internal data

解释：

- Embedded 模式下，客户端与服务端同机运行
- 因此 `client directory` 可以被认为是服务端可访问的真实路径
- 这时项目类工具默认应直接以 `project_root` 为根，而不是以 `workspace_dir` 为根

### 2. Remote

规则：

- `client directory` 仅作为客户端本地信息展示，不作为服务端默认执行根目录
- session 内增加 `server project_root` 概念
- 用户未手动设置时：
  - 默认尝试 `project_root = workspace_dir`
  - 若 `workspace_dir` 不存在或不可访问，则 `project_root = unset`
- 当 `project_root = unset` 且模型发起项目类工具时：
  - 不执行
  - 返回明确错误，提示用户指定服务端路径

解释：

- Remote 模式下，客户端 cwd 与服务端文件系统天然不一致
- 因此服务端必须显式持有一份“当前项目根目录”状态

## 工具路由规则

### 默认走 `project_root`

- `list_dir`
- `read_file`
- `grep_files`
- `write_file`
- `edit`
- `apply_patch` 目标文件解析
- `exec_command` 默认 `workdir`

### 默认走 `workspace_dir`

- `memory_search`
- `memory_get`
- workspace memory flush
- session summary / compaction
- Talos internal artifacts

### 显式绝对路径

- 若调用参数中携带绝对路径，则直接使用该路径
- 不再通过默认 root 进行重写

## 数据模型改进

### 保留现有字段

- `TalosSessionEnvironment.directory`
  - 含义保持为“客户端本地运行目录”
  - 不再被误认为默认工具执行根目录

### 新增服务端会话态

建议新增 session-scoped 的 `project_root`

候选落点：

- `SessionContext.project_root: Option<PathBuf>`
- 或 `SessionExecutionContext.project_root: Option<PathBuf>`

要求：

- Embedded 创建 session 时自动填入当前目录
- Remote 创建 session 时默认：
  - 存在可访问 `workspace_dir` 则填入该路径
  - 否则为空
- 允许通过显式命令或 API 更新

### `workspace_id`

保持为逻辑标识，不承担路径语义。

## Prompt 语义

Prompt 必须同时暴露两套信息，但禁止混淆：

### Embedded

应告诉模型：

- current project root: `<project_root>`
- talos workspace dir: `<workspace_dir>`

### Remote

应告诉模型：

- server project root: `<path or unset>`
- client local directory: `<directory>` for display only
- talos workspace dir: `<workspace_dir>`

并明确：

- 项目类文件工具默认使用 `server project root`
- 当 `server project root` 未设置时，应先提示用户指定目录

## TUI 展示语义

### Embedded

展示：

- `project root: <current dir>`
- `talos workspace: <workspace_dir>`
- `connection: embedded`

### Remote

展示：

- `server project root: <path or unset>`
- `client directory: <local dir>`
- `talos workspace: <workspace_dir>`
- `connection: remote`

要求：

- 不再把 `client directory` 误渲染成“当前服务端项目根目录”

## 错误语义

### Remote 未设置项目根目录

项目类工具调用时，返回明确错误：

```text
No accessible server project root is configured for this session.
Please specify a server directory before repository exploration.
```

### 目录不存在

返回明确错误：

```text
Configured project root does not exist on the server: <path>
Please choose another server directory.
```

### 不允许静默回退

以下行为全部禁止：

- `project_root` 不可用时自动退到客户端 cwd
- `project_root` 不可用时自动退到错误目录
- UI 展示 A，工具默认却跑 B

## 交互规则

### Embedded

- 默认直接工作
- 模型可基于显式绝对路径切换分析目标
- 不需要用户先手动“选择目录”

### Remote

- 默认使用服务端已有 `project_root`
- 若无可用 `project_root`，必须提示用户指定服务端路径
- 一旦用户指定，后续 session 内沿用该 `project_root`

## 实现建议

### Phase 1

- 明确区分 `client directory` 与 `server project_root`
- 引入 session-scoped `project_root`
- TUI 展示改成三段语义：`project root / client directory / talos workspace`

### Phase 2

- 工具运行时引入 root resolver
- 项目类工具默认走 `project_root`
- internal tool 默认走 `workspace_dir`

### Phase 3

- Remote 增加“选择服务端目录”交互
- 未设置时返回明确错误而不是乱跑

### Phase 4

- e2e 与 tmux 手测
- 验证 `grep_files/list_dir/read_file/exec_command` 默认根目录正确

## 验收标准

1. Embedded 模式下，从当前仓库启动 TUI，`grep_files/list_dir/read_file` 默认工作在当前目录
2. Remote 模式下，客户端本地目录不会再被误当成服务端执行根目录
3. Remote 模式下，未设置服务端项目目录时，项目类工具不会 silent fallback
4. UI、prompt、工具默认执行根目录三者一致
5. `workspace_dir` 继续只承担 Talos internal data 目录职责
