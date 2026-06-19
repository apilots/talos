# Plugin Marketplace 与 Plugin Manager 设计

Status: draft
Owner: agent
Created: 2026-06-19
Updated: 2026-06-19

## 背景

Talos 已有第一版 Skills 系统，可以从本地 roots 加载 `SKILL.md`，并通过 prompt 注入让模型使用 skill。下一步需要补齐 Marketplace 与 Plugin Manager：

- 用户可以添加官方或第三方 marketplace source。
- Talos 可以从 source 中搜索可安装 plugins。
- Plugin Manager 统一管理安装、更新、卸载、启用、禁用。
- 第一版先支持 `PluginKind::Skill`，后续扩展 MCP server、package、feature。

典型 marketplace source：

```text
url: https://github.com/apilots/skills
ref: main
sparse_paths:
  - skills
```

该仓库在 `skills/` 下包含多个 skill plugin：

```text
skills/
  docx/
    SKILL.md
  frontend-design/
    SKILL.md
  pdf/
    SKILL.md
```

远端 source 只用于 search、install、update。远端内容不能直接进入 Skills roots，也不能伪造成已加载的 `SkillSummary`。

## 核心判断

Plugin Manager 统一管理四类对象：

```text
MarketplaceSource  # 官方或第三方 plugin 来源
Plugin             # 可搜索、可安装、可启停、可更新的统一 plugin 对象
PluginInstall      # 已安装 plugin 的本地安装记录
PluginKind         # Skill / McpServer / Package / Feature
```

不要再引入这些中间概念：

```text
MarketplaceItem
AvailablePlugin
DiscoveredExtension
ExtensionItem
InstalledExtension
```

它们本质上都是 `Plugin` 的不同来源或状态表现。

本 spec 与 `feat-026-talos-skills-system` 属于同一个未正式发布迭代。实现时不需要兼容旧 Skills lifecycle 行为，不允许为旧 `skills.install` / `skills.uninstall` / `skills.enable` / `skills.disable` 做 fallback、转发或降级实现；旧入口应删除，由 `plugin.*` 成为唯一管理面。

核心关系：

```text
MarketplaceSource
  -> plugin.search
     -> Plugin(status = NotInstalled | Installed | UpdateAvailable | ...)
        -> plugin.install
           -> PluginInstall
              -> kind-specific files under ${TALOS_HOME}/plugins/installs/<install-id>/
```

## 分层设计

新增独立 domain crate：

```text
talos-plugin
```

职责：

- 管理 marketplace sources。
- 基于 sources 搜索 plugins。
- 管理 plugins 的 install / update / uninstall / enable / disable。
- 读写 Plugin Manager JSON 状态文件。
- 调用 kind-specific installer：
  - `talos-skills` for `PluginKind::Skill`
  - future MCP installer for `PluginKind::McpServer`
  - future package installer for `PluginKind::Package`
  - future feature installer for `PluginKind::Feature`

不负责：

- 不直接暴露 HTTP / JSON-RPC envelope。
- 不持久化到 DB。
- 不参与 agent-kernel prompt 构建。
- 不重复定义 protocol DTO。
- 不执行 plugin 自带脚本或 install command。

分层关系：

```text
talos-types
  owns shared skills and plugin manager objects

talos-plugin
  owns Plugin Manager domain logic and JSON state persistence
  delegates kind-specific parsing/copying to talos-skills / future installers

application-service
  exposes PluginManagerService as the product facade
  calls talos-plugin

gateway/http and gateway/app-server
  only wrap talos-types objects in HTTP / JSON-RPC envelopes

Rust SDK / TS SDK
  expose the same talos-types core objects to clients
  hide transport envelope differences
```

核心约束：

- Plugin Manager 的核心对象统一定义在 `talos-types::plugin`。
- 对外可见的 Skills 对象统一定义在 `talos-types::skills`。
- `talos-plugin`、`application-service`、`app-protocol`、HTTP、Rust SDK、TS SDK 都引用同一组 core types。
- HTTP 和 JSON-RPC 只允许有 envelope 差异，不允许各自复制 source/plugin/install DTO。
- 对外 request / response 对象统一命名为 `XRequest` / `XResponse`，不使用 `Params` 表达业务请求对象。
- 当前没有 DB entity，不存在 DB model / DTO / runtime model 三层转换。
- JSON 文件 schema 使用同一组 `talos-types` 对象序列化。
- JSON 只是持久化和传输格式，不是业务类型。
- 禁止用 `serde_json::Value`、`TalosJsonValue`、`HashMap<String, Value>` 或自由 metadata map 表达核心字段。
- 所有 kind/status/source/location/conflict_policy 必须是枚举，不允许裸 `String`。
- unknown fields 默认拒绝，除非字段明确设计成 typed extension point。

## 与 Skills API 的边界

Plugin Manager 是安装管理面的唯一权威入口。

保留的 `skills.*` API：

```text
skills.list
skills.read
skills.suggest
skills.resolve
```

这些 API 属于 Skill catalog / prompt 使用侧：

- 列出已加载 skills。
- 读取 `SKILL.md`。
- 根据用户输入 suggest / resolve skill。
- 服务 prompt injection。

不定义为安装管理入口：

```text
skills.install
skills.uninstall
skills.enable
skills.disable
```

处理策略：

- 新目标态不暴露这些 API。
- 最终安装、卸载、启用、禁用、更新都走 `plugin.*`。
- Skills 只负责加载、读取、选择和 prompt 注入，不拥有 install ownership。

## 与现有 Skills 对象的关系

Plugin Manager 不能复用 `SkillSummary` 表示远端搜索结果。

原因：

- `SkillSummary` 表示已经从本地 skill roots 加载出来的 skill。
- 它包含 `root`、`path`、`scope`、`enabled`、`read_only` 等本地 catalog 语义。
- 远端 marketplace search result 还未安装，没有真实本地 skill root/path。

需要新增 manifest 层对象：

```rust
pub struct SkillManifest {
    pub name: String,
    pub description: String,
    pub short_description: Option<String>,
    pub metadata: Option<SkillMetadata>,
    pub license: Option<String>,
}
```

对象边界：

| 对象 | 用途 |
| --- | --- |
| `SkillManifest` | 远端或待安装 skill 的 manifest 信息，不包含本地 root/path/scope/enabled |
| `SkillSummary` | 已加载到本地 skill catalog 的 summary |
| `SkillDocument` | 已加载 skill 的 `SKILL.md` 内容 |
| `SkillSelector` | 已加载 skill 的选择器 |
| `PluginSkillStateKey` | plugin-owned skill enable/disable 状态 key |

类型归属建议：

- 将 `SkillManifest`、`SkillMetadata`、`SkillSummary`、`SkillSelector`、`SkillDocument`、`SkillWarning`、`SkillLoadError` 沉淀到 `talos-types::skills`。
- `talos-skills` 继续保留 loader/catalog/prompt 逻辑，并引用 `talos-types::skills`。
- Skill 生命周期管理不进入 `talos-skills`，也不新增 `skills.install` / `skills.enable` 这类管理 API。

## TALOS_HOME 路径

所有路径必须基于 Talos runtime home 派生，不能写死 `${HOME}/.talos`。

```text
${TALOS_HOME}/
  plugins/
    marketplace-sources.json
    plugin-installs.json
    skills/
      skills-state.json
    installs/
      <install-id>/
        skills/
          <skill-name>/
            SKILL.md
        mcp/
          <server-name>/
        packages/
          <package-name>/
        features/
          <feature-name>/
    cache/
      marketplaces/
        <source-id>/
          repo/
          scan.json
```

说明：

- `${TALOS_HOME}/plugins` 是 Plugin Manager 的唯一本地管理根。
- `${TALOS_HOME}/plugins/marketplace-sources.json` 保存 marketplace sources。
- `${TALOS_HOME}/plugins/plugin-installs.json` 保存 plugin install records。
- `${TALOS_HOME}/plugins/skills/skills-state.json` 保存 plugin-owned skill enable/disable 状态。
- `${TALOS_HOME}/plugins/installs/<install-id>` 保存 plugin-owned install files。
- `${TALOS_HOME}/plugins/cache/marketplaces/<source-id>` 是 source fetch / scan cache，不进入 `skills.roots`。
- marketplace 安装的 skill 写入 `${TALOS_HOME}/plugins/installs/<install-id>/skills/<name>`。
- MCP server、package、feature 后续写入同一个 install root 下的 kind-specific 子目录。
- plugin-owned skill 通过 plugin root 以 `SkillScope::Plugin` 暴露给 Skills catalog。

User scope：

- 第一版默认 local single-user。
- 若 `gateway.auth=true` 且启用多用户，Plugin Manager 状态路径必须增加 user scope，例如 `${TALOS_HOME}/users/<user-id>/plugins/...`。
- HTTP / JSON-RPC 必须复用同一权限检查。

## 配置

```yaml
plugins:
  sources:
    - source_type: official
      display_name: Apilots Skills
      kind: git
      url: https://github.com/apilots/skills
      reference: main
      sparse_paths:
        - skills
  roots:
    - kind: skill
      path: ~/.agents/skills
      read_only: true
    - kind: skill
      path: ~/.codex/skills
      read_only: true
```

配置规则：

- `plugins.sources` 是默认 marketplace sources 配置。
- 官方仓库默认包含 `https://github.com/apilots/skills`，`source_type = official`。
- 用户通过 UI 添加的三方仓库使用 `source_type = user`。
- UI 必须根据 `source_type` 标记官方 source 与用户 source。
- `plugins.roots` 是配置化本地 roots 数组，用于接入 Talos 外部已经存在的 plugins / skills。
- `plugins.roots` 与 marketplace sources 不同：它不执行 search / install / update，只作为 catalog source。
- `plugins.roots` 与 `PluginInstall` 不同：它没有 install record，也没有 lifecycle ownership。
- 第一版只支持 `kind: skill` 且 `read_only: true`。
- `~/.agents/skills` 是常见只读 skill root，用来加载用户自己已经安装到 Agent Skills 生态的 skills。
- `~/.codex/skills` 或其它目录可以通过同一数组配置。
- configured skill roots 由 `PluginManagerService` 传给 `SkillsService` 的 `SkillContext.external_roots`，并以三方只读 `SkillScope::External` 暴露。

## 强类型对象

### ID

```rust
pub struct MarketplaceSourceId(pub String);
pub struct PluginId(pub String);
pub struct PluginInstallId(pub String);
```

ID 规则：

- marketplace plugin 的 `PluginId` 由 `{source_id, kind, path_in_source}` 的 canonical key 确定性派生，例如 `plugin_<sha256>`。
- local path plugin 使用独立 namespace，由 canonical local path 派生，例如 `local_<sha256>`。
- `PluginInstallId` 是安装实例 ID，不等同于 `PluginId`。
- 不允许只用 plugin name 作为 ID。同名 plugin 可来自多个 sources。

### MarketplaceSource

```rust
pub struct MarketplaceSource {
    pub id: MarketplaceSourceId,
    pub source_type: MarketplaceSourceType,
    pub kind: MarketplaceSourceKind,
    pub display_name: String,
    pub enabled: bool,
    pub location: MarketplaceSourceLocation,
    pub sparse_paths: Vec<String>,
    pub last_checked_commit: Option<String>,
    pub last_checked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub enum MarketplaceSourceType {
    Official,
    User,
}

pub enum MarketplaceSourceKind {
    Git,
    LocalFolder,
}

pub enum MarketplaceSourceLocation {
    Git {
        url: String,
        reference: String,
    },
    LocalFolder {
        path: PathBuf,
    },
}
```

### Plugin

```rust
pub struct Plugin {
    pub id: PluginId,
    pub kind: PluginKind,
    pub name: String,
    pub description: String,
    pub source_id: Option<MarketplaceSourceId>,
    pub path_in_source: Option<String>,
    pub source_commit: Option<String>,
    pub status: PluginStatus,
    pub definition: PluginDefinition,
    pub warnings: Vec<PluginWarning>,
}

pub enum PluginKind {
    Skill,
    McpServer,
    Package,
    Feature,
}

pub enum PluginStatus {
    NotInstalled,
    Installed {
        install_id: PluginInstallId,
        installed_path: PathBuf,
        installed_commit: Option<String>,
        enabled: bool,
    },
    UpdateAvailable {
        install_id: PluginInstallId,
        installed_path: PathBuf,
        installed_commit: Option<String>,
        latest_commit: String,
        enabled: bool,
    },
    SourceMissing {
        install_id: PluginInstallId,
        installed_path: PathBuf,
        installed_commit: Option<String>,
        enabled: bool,
    },
    Conflict {
        reason: PluginConflictReason,
    },
}

pub enum PluginDefinition {
    Skill(SkillPlugin),
    McpServer(McpServerPlugin),
    Package(PackagePlugin),
    Feature(FeaturePlugin),
}

pub struct SkillPlugin {
    pub manifest: SkillManifest,
    pub skill_md_path: String,
}

pub struct McpServerPlugin {
    pub name: String,
    pub transport: McpServerTransportKind,
    pub manifest_path: String,
}

pub enum McpServerTransportKind {
    Stdio,
    Http,
    Sse,
}

pub struct PackagePlugin {
    pub name: String,
    pub package_kind: PackageKind,
    pub manifest_path: String,
}

pub enum PackageKind {
    Npm,
    Python,
    Binary,
}

pub struct FeaturePlugin {
    pub name: String,
    pub feature_kind: FeatureKind,
    pub manifest_path: String,
}

pub enum FeatureKind {
    Ui,
    Runtime,
    Integration,
}

pub struct PluginWarning {
    pub code: PluginWarningCode,
    pub message: String,
    pub kind: Option<PluginKind>,
    pub source_id: Option<MarketplaceSourceId>,
    pub path: Option<String>,
}

pub enum PluginWarningCode {
    InvalidManifest,
    MissingRequiredField,
    UnsupportedPluginKind,
    NestedPlugin,
    PathEscapedSource,
    InvalidSourceLayout,
    OrphanInstallDirectory,
}

pub enum PluginConflictReason {
    SameNameDifferentSource,
    TargetPathExists,
    TargetPathMissing,
    LocalChangesDetected,
    UnsupportedKind,
}
```

### PluginInstall

```rust
pub struct PluginInstall {
    pub id: PluginInstallId,
    pub plugin_id: PluginId,
    pub kind: PluginKind,
    pub name: String,
    pub installed_path: PathBuf,
    pub exposed_skill_root: Option<PathBuf>,
    pub source: PluginInstallSource,
    pub installed_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub file_manifest: FileManifest,
}

pub enum PluginInstallSource {
    Marketplace {
        source_id: MarketplaceSourceId,
        url: String,
        reference: String,
        commit: String,
        path_in_source: String,
    },
    LocalPath {
        path: PathBuf,
    },
    Manual,
}

pub struct FileManifest {
    pub files: Vec<FileEntry>,
}

pub struct FileEntry {
    pub relative_path: String,
    pub sha256: String,
    pub size_bytes: u64,
}
```

说明：

- `PluginInstall` 是本地安装记录，不替代 `Plugin`。
- `installed_path` 指向 `${TALOS_HOME}/plugins/installs/<install-id>`。
- Skill plugin 的 `exposed_skill_root` 指向 `${TALOS_HOME}/plugins/installs/<install-id>/skills/<name>`。
- enable/disable 状态不写入 `PluginInstall`，避免同一状态写两处。

### PluginRootConfig

```rust
pub struct PluginRootConfig {
    pub kind: PluginRootKind,
    pub path: PathBuf,
    pub read_only: bool,
}

pub enum PluginRootKind {
    Skill,
    McpServer,
    Package,
    Feature,
}
```

说明：

- `PluginRootConfig` 表示配置化本地 roots，不表示 installed plugin。
- `Skill` root 示例包括 `~/.agents/skills`、`~/.codex/skills` 或用户显式配置的其它 skill 目录。
- 第一版只支持 `read_only = true` 的 configured roots。
- configured roots 进入对应 kind 的 catalog，但不进入 Plugin Manager lifecycle。
- configured skill roots 以三方只读 `SkillScope::External` 暴露给 Skills catalog。
- configured roots 不写入 `PluginInstallsFile`，也不支持 `plugin.update` / `plugin.uninstall` / `plugin.enable` / `plugin.disable`。

### State Files

```rust
pub struct SkillStateFile {
    pub version: u32,
    pub disabled: BTreeSet<PluginSkillStateKey>,
}

pub struct PluginSkillStateKey {
    pub install_id: PluginInstallId,
    pub relative_skill_md_path: String,
}

pub struct MarketplaceSourcesFile {
    pub version: u32,
    pub sources: Vec<MarketplaceSource>,
}

pub struct PluginInstallsFile {
    pub version: u32,
    pub installs: Vec<PluginInstall>,
}
```

状态文件路径：

```text
${TALOS_HOME}/plugins/skills/skills-state.json
${TALOS_HOME}/plugins/marketplace-sources.json
${TALOS_HOME}/plugins/plugin-installs.json
```

JSON 文件不存在时返回空状态对象。JSON 文件损坏时 fail fast，不静默重置。

## Plugin Manager API

Plugin Manager API 分四组：

```text
plugin.source.*  # 管 marketplace sources
plugin.list      # 列出本地已管理 plugins
plugin.search    # 从 marketplace sources 搜索 plugins
plugin.*         # install/update/uninstall/enable/disable
```

`plugin.list` 与 `plugin.search` 语义不同：

- `plugin.list`：列出本地已管理 plugins，主要来自 `PluginInstallsFile`，不访问网络。
- `plugin.search`：基于已添加 sources 的 scan cache 搜索 plugins，可按 refresh policy 决定是否访问网络。

Methods：

```text
plugin.source.list
plugin.source.add
plugin.source.refresh
plugin.source.remove

plugin.list
plugin.search

plugin.install
plugin.update
plugin.uninstall
plugin.enable
plugin.disable
```

API 清单：

| Method | Request | Response | 职责 |
| --- | --- | --- | --- |
| `plugin.source.list` | `PluginSourceListRequest` | `PluginSourceListResponse` | 列出 marketplace sources |
| `plugin.source.add` | `PluginSourceAddRequest` | `PluginSourceAddResponse` | 添加 source |
| `plugin.source.refresh` | `PluginSourceRefreshRequest` | `PluginSourceRefreshResponse` | 刷新 source scan cache |
| `plugin.source.remove` | `PluginSourceRemoveRequest` | `PluginSourceRemoveResponse` | 移除 source，不删除 installs |
| `plugin.list` | `PluginListRequest` | `PluginListResponse` | 列出本地已管理 plugins |
| `plugin.search` | `PluginSearchRequest` | `PluginSearchResponse` | 从 sources 搜索 plugins |
| `plugin.install` | `PluginInstallRequest` | `PluginInstallResponse` | 安装 plugin |
| `plugin.update` | `PluginUpdateRequest` | `PluginUpdateResponse` | 更新 installed plugin |
| `plugin.uninstall` | `PluginUninstallRequest` | `PluginUninstallResponse` | 卸载 installed plugin |
| `plugin.enable` | `PluginEnableRequest` | `PluginEnableResponse` | 启用 installed plugin |
| `plugin.disable` | `PluginDisableRequest` | `PluginDisableResponse` | 禁用 installed plugin |

Request / Response：

```rust
pub struct PluginSourceListRequest {
    pub include_disabled: bool,
}

pub struct PluginSourceListResponse {
    pub sources: Vec<MarketplaceSource>,
}

pub struct PluginSourceAddRequest {
    pub source: MarketplaceSourceInput,
    pub refresh: bool,
}

pub struct PluginSourceAddResponse {
    pub source: MarketplaceSource,
    pub plugins: Vec<Plugin>,
}

pub enum MarketplaceSourceInput {
    Git {
        url: String,
        reference: String,
        sparse_paths: Vec<String>,
    },
    LocalFolder {
        path: PathBuf,
        sparse_paths: Vec<String>,
    },
}

pub struct PluginSourceRefreshRequest {
    pub source_id: MarketplaceSourceId,
}

pub struct PluginSourceRefreshResponse {
    pub source: MarketplaceSource,
    pub plugins: Vec<Plugin>,
}

pub struct PluginSourceRemoveRequest {
    pub source_id: MarketplaceSourceId,
}

pub struct PluginSourceRemoveResponse {
    pub source_id: MarketplaceSourceId,
    pub removed: bool,
}

pub struct PluginListRequest {
    pub kind: Option<PluginKind>,
    pub status: Option<PluginListStatusFilter>,
    pub source_id: Option<MarketplaceSourceId>,
    pub include_disabled: bool,
}

pub enum PluginListStatusFilter {
    Installed,
    Disabled,
    UpdateAvailable,
    SourceMissing,
    Conflict,
}

pub struct PluginListResponse {
    pub plugins: Vec<Plugin>,
}

pub struct PluginSearchRequest {
    pub query: Option<String>,
    pub source_id: Option<MarketplaceSourceId>,
    pub kind: Option<PluginKind>,
    pub status: Option<PluginSearchStatusFilter>,
    pub refresh: PluginSearchRefreshPolicy,
    pub limit: Option<usize>,
}

pub enum PluginSearchStatusFilter {
    Any,
    NotInstalled,
    Installed,
    UpdateAvailable,
    Conflict,
}

pub enum PluginSearchRefreshPolicy {
    CacheOnly,
    RefreshStale,
    ForceRefresh,
}

pub struct PluginSearchResponse {
    pub plugins: Vec<Plugin>,
}

pub struct PluginInstallRequest {
    pub target: PluginInstallTarget,
    pub enable: Option<bool>,
    pub conflict_policy: PluginInstallConflictPolicy,
}

pub enum PluginInstallTarget {
    Plugin {
        plugin_id: PluginId,
        expected_source_commit: Option<String>,
    },
    LocalPath {
        path: PathBuf,
    },
    Git {
        url: String,
        reference: String,
        sparse_paths: Vec<String>,
        path_in_source: String,
    },
}

pub enum PluginInstallConflictPolicy {
    Reject,
    Replace,
}

pub struct PluginInstallResponse {
    pub plugin: Plugin,
    pub install: PluginInstall,
}

pub struct PluginUpdateRequest {
    pub install_id: PluginInstallId,
    pub expected_source_commit: Option<String>,
    pub conflict_policy: PluginUpdateConflictPolicy,
}

pub enum PluginUpdateConflictPolicy {
    RejectLocalChanges,
    ReplaceLocalChanges,
}

pub struct PluginUpdateResponse {
    pub plugin: Plugin,
    pub install: PluginInstall,
}

pub struct PluginUninstallRequest {
    pub install_id: PluginInstallId,
}

pub struct PluginUninstallResponse {
    pub install_id: PluginInstallId,
    pub uninstalled: bool,
}

pub struct PluginEnableRequest {
    pub install_id: PluginInstallId,
}

pub struct PluginEnableResponse {
    pub plugin: Plugin,
}

pub struct PluginDisableRequest {
    pub install_id: PluginInstallId,
}

pub struct PluginDisableResponse {
    pub plugin: Plugin,
}
```

## 行为语义

### Source Add / Refresh

`plugin.source.add` 添加 source。`refresh=true` 时立即 fetch 并扫描，返回 `plugins`；`refresh=false` 时只保存 source。

`plugin.source.refresh` 更新 source scan cache：

- resolve ref 到 commit。
- fetch sparse paths。
- 扫描支持的 plugin manifests。
- 第一版扫描 `SKILL.md`。
- 更新 `last_checked_commit` / `last_checked_at`。

### Plugin Search

`plugin.search` 查询 source scan cache，并返回 `Plugin` 列表。

返回的 `Plugin.status` 可能是：

- `NotInstalled`
- `Installed`
- `UpdateAvailable`
- `SourceMissing`
- `Conflict`

refresh policy：

- `CacheOnly`：只读 cache，不访问网络。
- `RefreshStale`：过期时刷新。
- `ForceRefresh`：强制刷新。

### Plugin List

`plugin.list` 列出本地已管理 plugins，主要来自 `PluginInstallsFile`。它不访问网络，也不扫描 remote source。

### Install Skill Plugin

第一版 `plugin.install` 只支持 `PluginKind::Skill`。

安装行为：

- materialize plugin 到 temp。
- 调用 `talos-skills` 解析 `SKILL.md`。
- 同名冲突默认拒绝。
- 复制完整 skill 目录到 `${TALOS_HOME}/plugins/installs/<install-id>/skills/<name>`。
- 写入 `${TALOS_HOME}/plugins/plugin-installs.json`。
- 根据 `enable` 更新 `${TALOS_HOME}/plugins/skills/skills-state.json`。
- 将 plugin-owned skill root 以 `SkillScope::Plugin` 暴露给 Skills catalog。

ownership 规则：

- `plugin.install` 管理 `${TALOS_HOME}/plugins/installs/<install-id>` 下的 plugin-owned files。
- `plugin.uninstall` 只能删除 `PluginInstallsFile` 中记录的 install path。
- enable/disable 统一通过 `plugin.enable/disable` 走 plugin ownership。
- `skills.*` catalog API 读取同一个 `SkillStateFile`，但不提供安装管理写入口。
- 配置型或 workspace trust roots 若存在，只作为 unmanaged/read-only catalog source，不承诺 enable/disable。

### Enable / Disable

第一版启停只对 `PluginKind::Skill` 生效：

- enable：从 `SkillStateFile.disabled` 移除该 skill 的 `PluginSkillStateKey`。
- disable：写入该 skill 的 `PluginSkillStateKey`。

未来 MCP / Package / Feature 的启停状态继续通过 `plugin.*` 暴露，但具体落盘对象必须按 kind 强类型扩展，不能用自由 JSON。

### Update

更新已安装 plugin：

- 根据 `PluginInstall.source` 找到 source。
- 检查 source 最新 commit 和 `path_in_source`。
- materialize 最新版本到 temp。
- 对 Skill 重新 parse `SKILL.md`。
- 检查本地文件是否被用户修改。
- 默认 `RejectLocalChanges`。
- 原子替换本地安装目录。
- 更新 `PluginInstall.file_manifest` 和 `updated_at`。
- 保留 enable / disable 状态。

### Remove Source

删除 source 只删除 source 配置和 cache，不删除已安装 plugins。

已安装 plugin 的 source 不存在时，`Plugin.status` 变成 `SourceMissing`，仍可使用、disable、uninstall。

## 状态写入与事务

Plugin Manager 使用 JSON 文件持久化，但必须具备文件级事务语义。

要求：

- `PluginStateStore` 对 `${TALOS_HOME}/plugins` 下的状态文件和安装目录写操作使用同一把进程间 file lock。
- JSON 写入必须使用 temp file + fsync + atomic rename。
- 安装目录写入必须先写到 `${TALOS_HOME}/plugins/installs/.tmp-<install-id>`，验证成功后 atomic rename 到最终目录。
- install / update 事务必须在 lock 内完成。
- crash recovery 需要清理 `.tmp-*` 目录。
- install record 存在但目录不存在时标记 `Conflict(TargetPathMissing)`。
- 目录存在但 install record 不存在时不自动暴露为 plugin-owned，可显示为 orphan 诊断。
- `SkillStateFile` 中指向不存在 plugin install 的 key 在读时忽略，并在下一次成功写入时清理。

## Git Source 实现策略

第一版优先使用本机 `git` 命令，不引入 libgit2。

命令策略：

```bash
git init <cache-repo>
git remote add origin <url>
git config core.sparseCheckout true
git sparse-checkout set <sparse-paths...>
git fetch --depth=1 --filter=blob:none --no-recurse-submodules origin <ref>
git checkout FETCH_HEAD
git rev-parse HEAD
```

安全限制：

- 不执行 hooks。
- 不递归 submodule。
- 使用受控 env，不继承危险 Git 配置。
- 只允许 `https://`、`ssh://`、`git@github.com:` 和本地 path；其它协议默认拒绝。
- ref 必须是 branch/tag/commit 格式，拒绝 shell metacharacters、空白和路径逃逸。
- fetch timeout。
- cache path 必须在 `${TALOS_HOME}/plugins/cache/marketplaces/<source-id>` 下。
- sparse path 必须是相对路径，不能包含 `..` 或绝对路径。
- cache repo 可复用；refresh 前清理未跟踪文件，定期清理无 source 引用的 cache。

文件安全：

- scan/copy 不 follow symlink。
- 只复制普通文件和目录。
- 对 Git materialized root 和 LocalFolder root 都做 canonical containment 校验。
- 每个复制源文件的 canonical path 必须仍位于 materialized plugin root 内。
- hardlink 不保留，复制为普通文件；LocalFolder source 不额外按 link count 拒绝，安全边界由 canonical containment、symlink 拒绝和普通文件复制规则保证。
- LocalFolder 的 sparse path 同样必须是相对路径，并通过 containment 校验。

## Skill Plugin 发现规则

扫描规则：

- 在每个 sparse path 下递归查找 `SKILL.md`。
- 一个包含 `SKILL.md` 的目录就是一个 skill plugin。
- 如果子目录也包含 `SKILL.md`，按独立 plugin 处理，并产生 `NestedPlugin` warning。
- 用现有 `talos-skills` loader 解析 frontmatter 到 `SkillManifest`。
- invalid YAML 或缺少必填字段时产出 `PluginWarning`，不中断整个 source。

Frontmatter：

- `name` 必填。
- `description` 必填。
- `license` 可选展示。
- `metadata.display_name` / `short_description` / `icon` 可选展示。
- unknown fields 第一版忽略，不写成灵活 JSON。

## 模块接口清单

| 模块 | 职责 | 主要接口 |
| --- | --- | --- |
| `talos-types` | 对外和内部共用的 Plugin Manager / Skills API 对象 | `MarketplaceSource`, `Plugin`, `PluginInstall`, `PluginInstallRequest` |
| `talos-plugin` | source、plugin search、install state、configured roots、JSON 持久化、update 判断 | `PluginManager`, `PluginStateStore`, `PluginSourceScanner`, `PluginRootResolver` |
| `talos-skills` | skill 本体解析、复制、读取；不管理 marketplace source | `parse_skill_dir`, `parse_skill_manifest`, `read_document` |
| `application-service` | service 层 facade，向多端暴露统一 Plugin Manager 能力 | `PluginManagerService`, `SkillsService` |
| `app-protocol` | JSON-RPC envelope 和 method dispatch；引用 `talos-types` core objects | `plugin.*` request/response envelope |
| `gateway/http` | HTTP envelope wrapper；引用 `talos-types` core objects | `/v1/plugin/*`, `/v1/skills/*` |
| `app-server` | JSON-RPC dispatch | dispatch `plugin.*` / existing `skills.*` |
| TS SDK | 对外 SDK high-level client；复用 shared core object | `client.plugin.*`, `client.skills.*` |
| Rust SDK | CLI/TUI/app-server-test-client 使用；直接引用 `talos-types` | `PluginClient`, `SkillsClient` |
| Web UI | Plugins 页面 source/search/install/update 交互 | source list、plugin search、install/update buttons |
| E2E | 真实在线和端到端回归 | GitHub source search/install/update scenario |

## UI 语义

Plugins 页面：

- marketplace source list
- add marketplace
- refresh source
- plugin search
- install / update / uninstall
- enable / disable

Skills 页面：

- 已加载 skills
- read `SKILL.md`
- skill-specific suggest / resolve

Add marketplace dialog：

```text
Source
  https://github.com/apilots/skills

Git ref
  main

Sparse paths
  skills
```

Search results 示例：

```text
Apilots Skills
  docx              Not installed
  pdf               Not installed
  frontend-design   Installed
  pptx              Update available
```

## E2E 策略

本项目 E2E 已经依赖真实 LLM 和网络，因此 marketplace E2E 可以覆盖真实 GitHub source。

建议新增：

```text
talos-e2e/scenarios/09_app_server/plugin_marketplace_e2e.sh
```

真实在线场景：

1. 使用独立 `TALOS_HOME`。
2. 添加 `https://github.com/apilots/skills`。
3. ref 使用 `main`。
4. sparse path 使用 `skills`。
5. `plugin.source.refresh` 能扫描到 `docx` 或 `frontend-design`。
6. `plugin.search` 能返回对应 `Plugin`。
7. `plugin.install` 安装其中一个 skill plugin。
8. `plugin.list` 能看到已安装 plugin。
9. `skills.list` 能看到该 skill，scope 为 `Plugin`。
10. `skills.read` 能读到 `SKILL.md`。
11. 启动一次 app-server turn，structured selection 选择该 skill。
12. 验证 prompt path 使用 skill injection，而不是只返回简单 OK。

同时补本地 git fixture 测试，用于快速定位代码回归：

- 临时创建 git repo。
- 写入 `skills/review/SKILL.md`。
- commit。
- 通过同一套 Git provider search / install / update。
- 验证 local changes conflict。

## 风险与待确认

1. 私有仓库认证

   第一版可以先支持用户本机 git credential；后续再设计 token 管理。

2. Source 删除语义

   删除 source 不删除已安装 plugins，这一点需要 UI 明确表达。

3. PluginKind 扩展

   第一版只实现 Skill。MCP server、Package、Feature 只保留强类型枚举和 details 占位，不能引入自由 JSON。
