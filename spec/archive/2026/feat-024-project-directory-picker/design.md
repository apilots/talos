# Project Directory Picker

Status: archived
Archived: 2026-07-14

## 背景与问题

Talos 当前已经有 Project 创建 API，但创建 Project 时需要用户手动输入绝对目录路径。外部反馈显示，这对不熟悉命令行路径的用户不友好，尤其是第一版发布后，用户期望像常见文件上传或目录选择体验一样，可以直接通过 UI 选择本地目录或创建新目录。

Talos Project 的 `directory` 不是一次性上传内容，而是后续 agent、tools、workspace 执行的真实工作目录。WebUI 和 Desktop 都应保持一致体验：用户在 Project 下拉中选择 `Add new project` 后，可以选择已有目录，或者在某个父目录下创建新目录，再注册为 Project。

当前事实：

| 领域 | 当前事实 | 问题 |
| --- | --- | --- |
| Project HTTP API | `POST /v1/projects` 接收 `name` 和 `directory` | 只能注册已有目录，用户需要手写路径 |
| Project app-server API | `project.create` 接收 `name` 和 `directory` | Desktop 与 WebUI 语义一致，但无目录浏览能力 |
| Project 校验 | 后端要求目录为绝对路径、存在、且是目录 | 校验已存在，但没有用户友好的目录选择入口 |
| Project 列表 | 返回已注册 Project 记录 | 目录被移动或删除时缺少可渲染的目录状态 |
| WebUI 创建入口 | Projects 页面有创建弹窗 | 仍是路径输入体验 |
| Desktop | 通过 app-server 协议复用 WebUI | 不应分叉成另一套产品行为 |

## 目标

- Project 创建支持目录选择体验，不要求用户手写 Linux/macOS 路径。
- WebUI 与 Desktop 使用同一套目录选择 UI 和同一套后端 FS API。
- 目录选择默认打开当前 Talos 运行用户的 home 目录。
- 第一版目录选择能力限定在当前 Talos 运行用户的 home 目录内。
- 前端记住用户最后一次使用的目录，下次打开目录选择器时优先使用该目录。
- 用户可以手动修改路径；路径变更后由后端校验并返回目录树。
- 支持两种创建 Project 的路径：
  - `Use an existing folder`: 选择已有目录并注册 Project。
  - `Start from scratch`: 选择父目录，输入新目录名，创建目录后注册 Project。
- 保持现有 `project.create` 语义：注册一个已存在目录为 Project。
- Project 列表返回目录可用性状态，目录不存在或不可访问时前端能展示提示。
- HTTP 与 app-server JSON-RPC 能力一致，业务逻辑复用 application/service 层。

## 非目标

- 不实现文件上传或目录内容上传。
- 不使用浏览器 File System Access API 作为 Project 目录来源。
- 不在 Desktop 使用系统原生目录选择器形成产品行为分叉。
- 不把最近目录保存到服务端用户配置或 Project 记录。
- 不在本期实现复杂权限模型、团队级目录策略或云端多租户隔离策略。
- 不修改 agent/tools 执行目录语义。
- 不把 FS API 做成通用服务器文件管理器：不展示文件、不删除、不重命名、不移动目录。

## 设计决策

### 1. WebUI 与 Desktop 统一走后端 FS API

目录选择器由前端渲染，但目录数据来自 Talos 后端：

```text
WebUI / Desktop WebView
  |
  | GET /v1/fs/directory?path=...
  v
Talos HTTP / app-server
  |
  v
FS application service
  |
  v
Server-side filesystem
```

这保证 WebUI 和 Desktop 行为一致：

- WebUI 浏览 Talos Server 所在机器 home root 内的目录。
- Desktop 浏览 sidecar/app-server 所在机器 home root 内的目录，通常就是用户本机 home 内的目录。
- Project 注册时使用的路径一定是 Talos 后端可访问的真实路径。

### 2. 最近目录由前端本地持久化

最近一次使用的目录属于 UI 体验状态，不属于服务端业务状态。

前端保存策略：

- 存储位置：浏览器 `localStorage`。
- 建议 key：`talos.projectDirectoryPicker.lastDirectory`。
- 保存时机：
  - 用户成功选择已有目录并创建 Project。
  - 用户成功创建新目录并创建 Project。
  - 用户在 picker 中成功打开某个目录时，可以更新当前会话状态，但持久化建议等确认成功后再写。
- 读取顺序：
  1. 读取 localStorage 中的最近目录。
  2. 调用后端校验该目录是否仍可访问。
  3. 如果不可访问，展示后端错误，不自动回退到 home。

服务端不需要知道“最近目录”，也不需要持久化该状态。

### 3. Directory picker 暴露有边界的服务端文件系统视图

Talos 配置文件是 `talos.yaml`，配置结构是强类型并启用 `deny_unknown_fields`。因此 FS 规则必须进入 Rust typed config，而不是只在 YAML 中临时约定。

建议新增顶层配置：

```yaml
fs:
  directory_picker:
    enabled: true
    root: home
    show_hidden: true
```

对应类型：

```rust
pub struct FsConfig {
    pub directory_picker: DirectoryPickerConfig,
}

pub struct DirectoryPickerConfig {
    pub enabled: bool,
    pub root: DirectoryPickerRoot,
    pub show_hidden: bool,
}

pub enum DirectoryPickerRoot {
    Home,
}
```

`root` 不是 UI 默认值，而是权限边界。第一版只支持 `home`：

- `GET /v1/fs/directory` 不传 path 时打开 home。
- 用户手动输入 path 时，path 必须仍在 home root 内，包含 home 本身。
- 创建目录时，parent 必须仍在 home root 内，包含 home 本身。
- 不允许通过 `..` 或手动输入跳出 home root。
- 第一版不支持 symlink directory。目录列表可以显示 symlink entry，但用户选中或进入 symlink 时必须返回明确错误。

第一版默认显示隐藏目录，`show_hidden` 配置默认为 `true`。如果配置为 `false`，后端过滤 dot directories。

### 4. FS API 只负责目录浏览和目录创建

FS API 不负责注册 Project。Project 注册继续使用已有 `project.create` / `POST /v1/projects`。

这样职责清晰：

| 能力 | 负责模块 |
| --- | --- |
| 浏览目录树 | FS API |
| 创建文件夹 | FS API |
| 注册 Project | Project API |
| Project 名称冲突校验 | Project API |
| Project 执行目录语义 | 现有 session/project 逻辑 |

### 5. Project 列表携带目录状态

已注册 Project 的目录可能在 Talos 外部被删除、移动或改权限。Project list 不能因为其中一个目录不可用而整体失败，否则用户无法看到并修复问题。

建议在 Project record 中增加状态对象：

```rust
pub struct ProjectStatus {
    pub state: ProjectStatusState,
    pub reason: Option<ProjectUnavailableReason>,
}

pub enum ProjectStatusState {
    Available,
    Missing,
    NotDirectory,
    PermissionDenied,
}

pub enum ProjectUnavailableReason {
    PathNotFound,
    NotDirectory,
    PermissionDenied,
}
```

HTTP / SDK 返回示例：

```json
{
  "project_id": "project_123",
  "name": "talos",
  "directory": "/Users/xujiang/data/ailabs/talos",
  "status": {
    "state": "available",
    "reason": null
  }
}
```

目录不存在时：

```json
{
  "project_id": "project_123",
  "name": "talos",
  "directory": "/Users/xujiang/data/ailabs/talos",
  "status": {
    "state": "missing",
    "reason": "path_not_found"
  }
}
```

前端渲染要求：

- Project 下拉和 Projects 页面都展示该状态。
- `available` 正常展示。
- `missing` / `not_directory` / `permission_denied` 显示 warning icon 和简短 tooltip。
- 不从列表中静默隐藏不可用 Project。
- 用户选中不可用 Project 时，不应开始新的需要该目录的任务；应提示目录不可用，并引导用户重新选择目录或删除 Project。
- 该状态是运行时快照，不写回 Project 存储。

## HTTP API

### GET `/v1/fs/directory`

浏览目录。`path` 可选，不传时使用 picker root，目前为当前 Talos runtime user 的 home。

请求：

```text
GET /v1/fs/directory
GET /v1/fs/directory?path=/Users/xujiang
```

响应：

```json
{
  "path": "/Users/xujiang",
  "parent": null,
  "entries": [
    {
      "name": "Code",
      "path": "/Users/xujiang/Code",
      "kind": "directory",
      "readable": true,
      "writable": true,
      "hidden": false,
      "is_symlink": false
    }
  ],
  "readable": true,
  "writable": true
}
```

说明：

- 只返回目录，不返回普通文件。
- `entries` 默认按目录名排序。
- 默认显示隐藏目录。仅当 `show_hidden=false` 时过滤 dot directories，例如 `.git`、`.config`。
- `parent` 为 `null` 表示已到文件系统根。
- 当当前目录等于 picker root 时，`parent` 返回 `null`，避免 UI 引导用户跳出 root。

错误：

| 场景 | HTTP 状态 | error code |
| --- | --- | --- |
| directory picker disabled | 403 | `fs_disabled` |
| path is not absolute | 400 | `invalid_path` |
| path is outside picker root | 403 | `path_outside_root` |
| path is a symlink | 400 | `symlink_not_supported` |
| path does not exist | 404 | `path_not_found` |
| path is not a directory | 400 | `not_directory` |
| path is not readable | 403 | `permission_denied` |

### POST `/v1/fs/directory`

创建目录。

请求：

```json
{
  "parent": "/Users/xujiang/Code",
  "name": "new-project"
}
```

响应：

```json
{
  "path": "/Users/xujiang/Code/new-project"
}
```

校验：

- `parent` 必须是绝对路径。
- `parent` 必须在 picker root 内，包含 root 本身。
- `parent` 必须存在且是目录。
- `parent` 必须可写。
- `name` 不能为空。
- `name` 不能是 `.` 或 `..`。
- `name` 不能包含路径分隔符。
- 只允许在 parent 下创建单层目录。
- 目标目录已存在时返回冲突错误。

错误：

| 场景 | HTTP 状态 | error code |
| --- | --- | --- |
| directory picker disabled | 403 | `fs_disabled` |
| invalid parent path | 400 | `invalid_path` |
| parent is outside picker root | 403 | `path_outside_root` |
| parent is a symlink | 400 | `symlink_not_supported` |
| parent does not exist | 404 | `path_not_found` |
| parent is not a directory | 400 | `not_directory` |
| parent is not writable | 403 | `permission_denied` |
| invalid directory name | 400 | `invalid_name` |
| target already exists | 409 | `already_exists` |

## app-server Protocol

保持与 HTTP 功能一致，新增 JSON-RPC methods：

```text
fs.directory.read
fs.directory.create
```

参数：

```rust
pub struct FsDirectoryReadParams {
    pub path: Option<PathBuf>,
}

pub struct FsDirectoryCreateParams {
    pub parent: PathBuf,
    pub name: String,
}
```

响应：

```rust
pub struct FsDirectorySnapshot {
    pub path: PathBuf,
    pub parent: Option<PathBuf>,
    pub entries: Vec<FsDirectoryEntry>,
    pub readable: bool,
    pub writable: bool,
}

pub struct FsDirectoryEntry {
    pub name: String,
    pub path: PathBuf,
    pub kind: FsEntryKind,
    pub readable: bool,
    pub writable: bool,
    pub hidden: bool,
    pub is_symlink: bool,
}

pub enum FsEntryKind {
    Directory,
}

pub struct FsDirectoryCreateResponse {
    pub path: PathBuf,
}
```

## 前端交互

Project 下拉新增：

```text
Project selector
  |
  +-- existing projects
  |
  +-- Add new project
      |
      +-- Start from scratch
      +-- Use an existing folder
  |
  +-- Don't work in a project
```

### Use an existing folder

```text
Open Project menu
  |
  v
Add new project -> Use an existing folder
  |
  v
DirectoryPickerDialog(mode = existing)
  |
  | initial path = lastDirectory or home
  v
GET /v1/fs/directory
  |
  v
User selects directory
  |
  v
Derive default project name from directory basename
  |
  v
POST /v1/projects
  |
  v
Persist lastDirectory in localStorage
  |
  v
Switch active project
```

### Start from scratch

```text
Open Project menu
  |
  v
Add new project -> Start from scratch
  |
  v
DirectoryPickerDialog(mode = create)
  |
  | initial path = lastDirectory or home
  v
User selects parent directory and enters folder name
  |
  v
POST /v1/fs/directory
  |
  v
POST /v1/projects
  |
  v
Persist created directory as lastDirectory
  |
  v
Switch active project
```

失败语义：

- `POST /v1/fs/directory` 成功但 `POST /v1/projects` 失败时，第一版不自动删除已创建目录。
- UI 必须明确提示：目录已创建，但 Project 注册失败。
- 提示中应保留已创建目录路径，并允许用户重试注册 Project。
- 失败原因仍使用 Project API 返回的错误，例如 Project name 冲突、鉴权过期或网络错误。
- 不做自动 cleanup 的原因是删除用户刚创建的文件系统目录有误删风险，且失败后用户仍可能希望保留该目录。

### DirectoryPickerDialog 状态

组件状态：

| 状态 | 说明 |
| --- | --- |
| `loading` | 正在读取目录 |
| `ready` | 当前目录可浏览 |
| `error` | 当前路径不可用，展示后端错误 |
| `creating` | 正在创建新目录 |
| `submitting` | 正在注册 Project |

交互要求：

- 路径输入框支持手动输入，按 Enter 后调用 `GET /v1/fs/directory`。
- 列表只展示目录。
- 支持返回上一级目录。
- 选择目录后可确认。
- 创建模式下，目录名输入必须前端先做基础校验，但最终以后端校验为准。
- 错误信息直接使用后端上下文，不静默吞错。

## 核心 API 伪代码

### FS service

```rust
impl FsService {
    pub async fn read_directory(
        &self,
        actor: &Actor,
        path: Option<PathBuf>,
    ) -> TalosResult<FsDirectorySnapshot> {
        self.ensure_enabled()?;
        let root = canonicalize(resolve_picker_root(self.config.root)?)?;
        let path = path.unwrap_or_else(|| root.clone());
        validate_absolute_directory(&path)?;
        reject_symlink(&path)?;
        let path = canonicalize(path)?;
        ensure_under_root(&path, &root)?;
        ensure_readable(&path)?;
        let entries = list_child_directories(&path, self.config.show_hidden)?;
        Ok(FsDirectorySnapshot::new(path, root, entries))
    }

    pub async fn create_directory(
        &self,
        actor: &Actor,
        parent: PathBuf,
        name: String,
    ) -> TalosResult<FsDirectoryCreateResponse> {
        self.ensure_enabled()?;
        let root = canonicalize(resolve_picker_root(self.config.root)?)?;
        validate_absolute_directory(&parent)?;
        reject_symlink(&parent)?;
        let parent = canonicalize(parent)?;
        ensure_under_root(&parent, &root)?;
        validate_directory_name(&name)?;
        ensure_writable(&parent)?;
        let path = parent.join(name);
        create_dir(&path)?;
        Ok(FsDirectoryCreateResponse { path })
    }
}
```

### Frontend picker bootstrap

```ts
async function openDirectoryPicker(mode: DirectoryPickerMode) {
  const lastDirectory = localStorage.getItem(LAST_DIRECTORY_KEY);
  if (!lastDirectory) {
    return await client.fs.directory.read();
  }
  return await client.fs.directory.read({ path: lastDirectory });
}
```

目录读取失败时不做自动回退。UI 直接展示后端错误，并提供显式的 `Open Home` 操作；用户点击后再调用 `client.fs.directory.read()`。

## 模块接口清单

| 模块 | 新增/修改 | 职责 |
| --- | --- | --- |
| `talos_config` | 新增 `FsConfig` / `DirectoryPickerConfig` | 读取 YAML 中的 FS 配置 |
| `talos_application_service` | 新增 FS service 或 adapter 方法 | 统一承载目录浏览和创建逻辑 |
| `talos_types` | 扩展 Project record 状态类型 | 表达已注册 Project 的目录可用性 |
| `gateway/http` | 新增 `/v1/fs/directory` GET/POST | HTTP 协议映射 |
| `gateway/app-protocol` | 新增 `fs.directory.read/create` params/response | app-server 协议类型 |
| `gateway/app-server` | dispatch 新增 FS methods | Desktop 协议映射到 application service |
| `talos-app/desktop` | 更新 IPC guard，允许 `fs.directory.read/create` | Renderer 能调用新增 app-server methods |
| `ts-sdk/core` | 新增 `client.fs.directory.read/create` | WebUI 统一调用入口 |
| `ts-sdk/browser` | 无特殊逻辑 | 继续通过 HTTP adapter 调用 |
| `ts-sdk/app-server` | 新增 FS JSON-RPC wrapper | Desktop 走同一语义 |
| `web/features/projects` | 新增 Project menu 创建入口 | Project 下拉与创建流程 |
| `web/shared/ui` | 新增或复用 `DirectoryPickerDialog` | 目录浏览、路径输入、创建目录 |

## 测试计划

后端：

- `GET /v1/fs/directory` 不传 path 返回 home。
- `GET /v1/fs/directory` 传入 home 外路径返回 `path_outside_root`。
- 读取 symlink directory 路径返回 `symlink_not_supported`。
- 目录列表可以显示 symlink directory，但进入或选中 symlink directory 必须返回 `symlink_not_supported`。
- `GET /v1/fs/directory` 返回目录列表且过滤普通文件。
- `GET /v1/fs/directory` 默认显示 dot directories。
- `GET /v1/fs/directory` 在 `show_hidden=false` 时过滤 dot directories。
- `GET /v1/fs/directory` 在 root 目录时返回 `parent: null`。
- 不存在路径返回 `path_not_found`。
- 普通文件路径返回 `not_directory`。
- 不可读目录返回 `permission_denied`。
- `POST /v1/fs/directory` 成功创建目录。
- `POST /v1/fs/directory` 在 home 外 parent 返回 `path_outside_root`。
- `POST /v1/fs/directory` 使用 symlink parent 返回 `symlink_not_supported`。
- `POST /v1/fs/directory` 只能创建单层目录。
- 非法目录名返回 `invalid_name`。
- 已存在目录返回 `already_exists`。
- `directory_picker.enabled=false` 时 read/create 都拒绝。
- app-server `fs.directory.read/create` 与 HTTP 结果语义一致。
- Desktop IPC guard 允许 `fs.directory.read/create`，且未知 FS method 仍被拒绝。
- `GET /v1/projects` 对可用目录返回 `status.state=available`。
- `GET /v1/projects` 对不存在目录返回 `status.state=missing`。
- `GET /v1/projects` 对普通文件路径返回 `status.state=not_directory`。
- `GET /v1/projects` 对不可读目录返回 `status.state=permission_denied`。
- 某个 Project 目录不可用时，Project 列表整体仍成功返回。

前端：

- Project 下拉显示 `Add new project` 二级菜单。
- `Use an existing folder` 打开目录选择弹窗。
- 最近目录存在且可访问时作为初始目录。
- 最近目录不可访问时展示后端错误，不自动回退到 home。
- 用户点击 `Open Home` 后显式打开 home。
- 手动输入路径后会重新请求目录。
- Project 下拉和 Projects 页面展示不可用目录 warning。
- 不可用 Project 被选中时阻止启动依赖该目录的新任务，并提示目录状态。
- 选择已有目录后调用 `projects.create`。
- 创建新目录成功后再调用 `projects.create`。
- 创建新目录成功但 `projects.create` 失败时展示“目录已创建但 Project 注册失败”的可重试状态。
- 成功创建 Project 后写入 localStorage 最近目录。

验证命令建议：

```text
cargo test -p talos-gateway-http fs
cargo test -p talos-gateway-app-protocol fs
cargo test -p talos-gateway-app-server fs
pnpm --dir talos-app/desktop test -- ipc-guard
pnpm --dir talos-app/web test -- DirectoryPicker
pnpm --dir talos-app/web typecheck
```

Desktop 涉及 WebView 与 app-server 协议，最终实现时还需要按项目规则执行 packaged DMG smoke。

## 风险与取舍

- WebUI 浏览的是 Talos Server 所在机器 home root 内的目录，不是浏览器用户电脑目录。这个语义需要在 UI 文案中表达清楚，但不要写成长篇说明。
- 最近目录在前端 localStorage 中，换浏览器或清缓存后会丢失，这是可接受的 UI 状态。
- 第一版只允许在 Talos runtime user home root 内浏览和创建目录，避免把目录选择器做成通用服务器文件系统浏览器。后续如进入多用户云端场景，应扩展更明确的 per-user root 策略或 allowed roots。
- `Start from scratch` 可能出现目录已创建但 Project 注册失败的部分成功状态。第一版不自动删除目录，避免误删用户数据，UI 负责提示并支持重试注册。
- Desktop 不使用原生目录选择器，牺牲一部分系统原生体验，但换来 WebUI/Desktop 完全一致的产品行为。
