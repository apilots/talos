# Talos MCP System 设计

Status: implemented
Owner: agent
Created: 2026-06-20
Updated: 2026-06-21

## 背景

Talos 当前已经有三块相关基础：

1. `talos-types::plugin` 已预留 `PluginKind::McpServer` 和 `PluginDefinition::McpServer`。
2. `talos-tools` 里存在历史遗留的手写 MCP client / transport / connection manager。
3. `ToolRegistry`、`ToolSearch`、deferred tool selection、canonical tool result、Skills prompt injection 已有基础能力。

现有手写 MCP 实现没有正式启用，属于历史遗留。新的 MCP 支持必须以 `rmcp` Rust 标准库为唯一 MCP 协议实现，并删除旧手写实现，不保留双轨兼容路径。

本 spec 专门定义 Talos MCP 的目标架构。它承接 `feat-026-talos-skills-system` 和 `feat-027-plugin-marketplace-skill-install` 的边界结论：

```text
Skill teaches the model how to work.
Tool performs an action for the model.
MCP server exposes external tools and resources.
Plugin manages lifecycle for installable capabilities.
```

核心问题不是“是否把 MCP 当作 Plugin”或“是否把 MCP 当作 Tool”，而是两个维度必须分开：

- MCP server 的安装、启用、禁用、更新、卸载归 Plugin Manager 生命周期管理。
- MCP server 暴露出来的 MCP tools 必须投影到 Talos tool calling 系统，否则模型无法通过原生 tool call 调用。

## Codex 参考结论

对 `~/data/ailabs/codex` 的调研结论：

- Codex 有独立 `codex-mcp` / `codex-rmcp-client`，使用 `rmcp` 管理 MCP runtime。
- Codex plugin manager 会加载 active plugins，并把 plugin 内声明的 MCP servers 与普通 config MCP servers 合并成 runtime MCP catalog。
- Codex 每轮构建 tools 时会从 `McpConnectionManager::list_all_tools()` 获取 MCP tools，再放进 tool router。
- Codex 支持 MCP tools 少量直接暴露或 deferred tools；Talos 在本设计中统一采用外部 tools 通过 `tool_search` 按需激活。
- Skills 走 prompt injection，不进入 MCP runtime，也不作为可执行 tool 注册。
- Skill 可以声明或提示 MCP dependency，但 dependency 的安装、启用、刷新仍通过 MCP/plugin/config 管理链路完成。

Talos 不复制 Codex 的具体对象命名和产品概念，但采用同一个架构判断：

```text
Plugin lifecycle owns MCP server availability.
MCP runtime owns protocol execution.
Tool registry owns model-callable projections.
Prompt builder owns guidance, not protocol execution.
```

## 目标

1. 新增 `talos-mcp` 模块，基于 `rmcp` 作为唯一 MCP runtime。
2. 删除 `talos-tools` 中历史遗留的手写 MCP protocol / client / transport 实现。
3. 将 `McpServerPlugin` 作为 Plugin Manager 的一等生命周期对象支持。
4. 让 enabled MCP server plugins 能被上层 runtime 组装、连接、发现 tools/resources/templates。
5. 将 MCP tools 通过 `ToolRegistry` / `ToolRouter` 暴露为模型可调用 tools。
6. 保持 Skills 和 MCP 的执行路径分离：Skills 走 prompt injection，MCP 走 tool calling。
7. 保持 WebUI、Desktop、TUI、SDK、replay 消费同一套 tool result schema。
8. 为 MCP resource list/read 和 deferred discovery 留出强类型路径。

## 非目标

- 不保留旧手写 MCP 实现。
- 不同时维护 `talos-tools::mcp` 和 `talos-mcp` 两套 runtime。
- 不把 MCP server 生命周期交给 `talos-tools`。
- 不让 `talos-plugin` 直接依赖 `talos-tools` 或 agent kernel。
- 不把 MCP 调用协议塞进 prompt，由模型按文本拼 JSON-RPC 调用。
- 不把 Skill 注册成 executable tool。
- 不在第一版实现 MCP server side，也不让 Talos 作为 MCP server 暴露自身能力。
- 不实现 marketplace 远端服务协议之外的新 plugin 分发系统。
- 不支持 legacy SSE transport 作为第一版目标；第一版只支持 stdio 和 streamable HTTP。
- 不通过 fallback 掩盖 MCP server 配置缺失、启动失败、tool schema 错误或权限拒绝。

## 实现状态

| 领域 | 当前状态 | 目标判断 |
| --- | --- | --- |
| Plugin 类型 | `PluginKind::McpServer`、`McpServerPlugin`、`PluginDocumentRef` 已接入共享类型 | MCP 和 Skills 使用同一套 plugin 外层对象，类型特有字段只放在各自 definition 内 |
| Plugin 安装状态 | `PluginInstall` 已区分 `exposed_skill_root` 与 `exposed_mcp_servers` | Skill 与 MCP 暴露字段不复用语义 |
| Plugin 扫描 | scanner 已拆分 discovery、materialize、skill manifest、MCP manifest 解析 | `SKILL.md` 与 `talos-mcp.json` 都通过 plugin scanner 进入统一 catalog |
| Plugin enable/disable | plugin lifecycle API 统一管理 Skills 与 MCP server plugins | lifecycle 不绕过 Plugin Manager |
| MCP runtime | 历史手写 MCP client/transport 已删除 | `talos-mcp` 基于 `rmcp` 作为唯一 MCP runtime |
| Runtime 组装 | `talos-base` 合并 enabled MCP server plugin configs，并将 MCP tool projections 接入 tools | MCP server lifecycle 与 tool projection 分层 |
| ToolSearch | 外部 MCP tools 进入 deferred catalog，通过 `tool_search` 按需激活 | 系统内置 tools 默认可见，外部 tools 按需可见 |
| PromptBuilder | Skills prompt context 与 MCP/plugin guidance 分离 | Prompt 只注入指导，不执行 MCP |

## 核心设计判断

### MCP 和 Skills 都是 Plugin 类型

Plugin Manager 管理安装态能力。MCP 和 Skills 都可以来自 marketplace、local roots 或未来远端同步，因此它们都属于 plugin lifecycle：

```text
Plugin
  Skill
  McpServer
  Package
  Feature
```

但不同 plugin kind 的运行方式不同：

| Kind | 生命周期归属 | 运行方式 | 模型入口 |
| --- | --- | --- | --- |
| `Skill` | `talos-plugin` | prompt injection | `PromptBuilder` |
| `McpServer` | `talos-plugin` | MCP client runtime | `ToolRegistry` |
| `Package` | `talos-plugin` | 后续定义 | 后续定义 |
| `Feature` | `talos-plugin` | 后续定义 | 后续定义 |

### MCP tools 必须注册为 Tools

MCP server 本身不是模型直接调用的 tool。MCP server 暴露的每个 MCP tool 才是模型可调用能力。

```text
McpServerPlugin
  -> InstalledMcpServer
     -> talos-mcp discovery
        -> McpToolDescriptor[]
           -> talos-tools adapter
              -> RegisteredToolSpec[]
                 -> model tool declarations
```

注册为 `ToolRegistry` 并不表示 `ToolRegistry` 拥有 MCP lifecycle。它只拥有当前 turn/session 的模型可见投影和执行路由。

### Prompt build 不执行 MCP

Prompt build 可以告诉模型：

- 当前有哪些 plugin capabilities。
- Skills 如何被触发。
- MCP tools 通过 `tool_search` 按需发现和激活。
- 某个 Skill 建议使用哪些 MCP capabilities。

但 Prompt build 不能：

- 连接 MCP server。
- 解析 MCP JSON-RPC。
- 直接调用 MCP tool。
- 将 MCP tool schema 用纯文本替代原生 tool declaration。

## 模块边界

### `talos-types`

拥有所有对外核心对象：

- `Plugin`
- `PluginKind`
- `PluginMetadata`
- `PluginDocumentRef`
- `PluginDefinition`
- `McpServerPlugin`
- `PluginInstall`
- `InstalledMcpServer`
- `McpServerRuntimeConfig`
- `McpTransportConfig`
- `McpToolDescriptor`
- `McpResourceDescriptor`
- `McpResourceTemplateDescriptor`
- `McpServerStatus`
- plugin / MCP request-response DTO

MCP 新增的结构化对象必须放在 `talos-types`，作为服务端、协议层、SDK、UI、TUI 和运行时 adapter 共享的唯一业务模型。`talos-mcp` 可以在内部使用 `rmcp` 类型，但对外 API 入参和返回值必须使用 `talos-types` 中定义的 Talos 自有类型。

建议归属：

| 对象类别 | 类型归属 | 说明 |
| --- | --- | --- |
| Plugin lifecycle objects | `talos-types::plugin` | `Plugin`、`PluginInstall`、`InstalledMcpServer` |
| MCP runtime config | `talos-types::mcp` 或 `talos-types::plugin::mcp` | `McpServerRuntimeConfig`、`McpTransportConfig` |
| MCP descriptors | `talos-types::mcp` | `McpToolDescriptor`、`McpResourceDescriptor`、`McpResourceTemplateDescriptor` |
| MCP API objects | `talos-types::mcp` | `McpCallToolRequest` / `McpCallToolResponse` 等 |
| Protocol envelopes | gateway / app-protocol | HTTP、JSON-RPC 只包装，不复制业务对象 |

要求：

- 枚举语义必须用 enum。
- 核心对象不能用 `serde_json::Value` 或 map 承载。
- JSON 只作为传输和落盘格式，不是业务模型。
- HTTP、JSON-RPC、SDK、UI 都引用同一套 core objects。
- `talos-mcp`、`talos-tools`、`talos-plugin`、`application-service` 不允许各自定义结构相似但不同的 MCP DTO。
- Request / Response 命名必须和 API 一一对应，例如 `McpCallToolRequest` / `McpCallToolResponse`，避免使用含义模糊的 `Params`、`Item`、`Details` 承载核心语义。

### `talos-plugin`

职责：

- 管理 marketplace sources。
- 搜索、安装、更新、卸载、启用、禁用 plugins。
- 扫描 skill plugin 和 MCP server plugin。
- 维护 plugin install state。
- 输出 enabled plugin capabilities 给上层 runtime。

不负责：

- 不连接 MCP server。
- 不调用 MCP tool。
- 不注册 `ToolRegistry`。
- 不拼 prompt。
- 不理解 agent turn lifecycle。

建议新增 facade：

```rust
pub struct PluginRuntimeCapabilities {
    pub skill_roots: SkillRoots,
    pub mcp_servers: Vec<InstalledMcpServer>,
}
```

`PluginManagerService` 根据当前 config / user scope 返回该对象，上层 runtime 再分别交给 Skills 和 MCP。

`PluginManagerService` 是 Service 层统一生命周期入口。它管理 Skill 和 MCP server plugin 的 install / update / uninstall / enable / disable。其它 service、runtime 或 tool adapter 只能消费它输出的 runtime capabilities，不能绕过它直接修改 plugin state。

### `talos-mcp`

新增 crate：

```text
talos-rs/crates/action/mcp
package = "talos-mcp"
```

职责：

- 依赖 `rmcp`。
- 管理 MCP server runtime connection / session，不管理 plugin 安装生命周期。
- 支持 stdio 和 streamable HTTP。
- 执行 initialize。
- discovery tools/resources/resource templates。
- 调用 tools/call。
- 读取 resources/read。
- 维护 server startup status、不可用原因、tool catalog cache。
- 输出 Talos 自有强类型对象，不把 `rmcp` 类型泄漏到上层协议。
- 封装 MCP 自身业务 API，向上层提供简洁、稳定、强类型的 runtime facade。

不负责：

- 不读写 plugin install state。
- 不决定 plugin 是否 enabled。
- 不注册 `ToolRegistry`。
- 不拼 PromptBuilder。
- 不处理 UI / SDK envelope。
- 不定义对外共享 DTO；共享对象必须来自 `talos-types`。
- 不暴露 `rmcp` client、transport、model object 给 `talos-plugin`、`talos-tools`、gateway、SDK 或 UI。

建议核心接口：

```rust
pub struct McpRuntime {
    // internal rmcp clients
}

impl McpRuntime {
    pub async fn start(configs: Vec<McpServerRuntimeConfig>) -> TalosResult<Self>;
    pub async fn list_tools(&self) -> TalosResult<Vec<McpToolDescriptor>>;
    pub async fn list_resources(&self, request: McpListResourcesRequest) -> TalosResult<McpListResourcesResponse>;
    pub async fn list_resource_templates(&self, request: McpListResourceTemplatesRequest) -> TalosResult<McpListResourceTemplatesResponse>;
    pub async fn read_resource(&self, request: McpReadResourceRequest) -> TalosResult<McpReadResourceResponse>;
    pub async fn call_tool(&self, request: McpCallToolRequest) -> TalosResult<McpCallToolResponse>;
    pub async fn shutdown(&self) -> TalosResult<()>;
}
```

API 设计约束：

- 入参和返回值全部使用 `talos-types` 中的 MCP domain object。
- `talos-mcp` 内部可以有 `rmcp` adapter / mapper，但 mapper 是 crate 私有实现细节。
- 上层不需要知道 MCP JSON-RPC method name、transport framing、初始化握手细节。
- 对外错误必须是 Talos 语义错误，包含 server、tool/resource、operation、原始 MCP error code/message。
- API 粒度以 Talos 业务动作命名，例如 `call_tool`、`list_resources`、`read_resource`，不暴露通用 `send_json_rpc`。

### `talos-tools`

职责：

- 接收 `McpToolDescriptor`。
- 映射为 `RegisteredToolSpec`。
- 执行时把 model tool call 路由到 `talos-mcp::McpRuntime`。
- 把 `McpCallToolResponse` 映射为 `ToolOutputPayload` / `TalosContentItem`。
- 提供 MCP resource tools：
  - `ListMcpResources`
  - `ListMcpResourceTemplates`
  - `ReadMcpResource`
- 支持 deferred MCP tool discovery 和 `ToolSearch`。

不负责：

- 不启动 MCP server。
- 不读取 plugin state。
- 不持久化 MCP catalog。
- 不实现 MCP 协议。

### `agent-kernel`

职责：

- 使用 `ToolRegistry` 获取当前 turn model-visible tool definitions。
- 执行 tool loop。
- 记录 MCP tool call events。
- 将 MCP tool result 回灌模型。
- 通过 PromptBuilder 注入 Skills 和 plugin/MCP guidance。

不负责：

- 不直接依赖 `rmcp`。
- 不直接读 plugin install state。
- 不直接解析 MCP manifest。

### `application-service` / `talos-base`

职责：

- 作为上层组装层。
- 从 `PluginManagerService` 获取 enabled plugin capabilities。
- 合并静态 config MCP servers 和 plugin-owned MCP servers。
- 创建 `talos-mcp::McpRuntime`。
- 将 discovered MCP tool descriptors 注册进 `ToolRegistry`。
- 在配置刷新、plugin enable/disable、install/update/uninstall 后刷新 MCP runtime。

Service 层边界：

- `PluginManagerService` 是生命周期管理 facade。
- `McpRuntimeService` 或 runtime holder 只负责根据 `PluginManagerService` 输出的 enabled MCP server configs 管理连接快照。
- `TalosApplicationService` 负责编排 plugin state、MCP runtime 和 tool registry 的刷新顺序。
- gateway / SDK 只能调用 service facade，不直接操作 `talos-mcp` 或 plugin state files。

## 数据模型

### MCP server plugin manifest

MCP plugin 应有显式 manifest，建议默认文件名：

```text
talos-mcp.json
```

示例：

```json
{
  "schema_version": 1,
  "name": "filesystem",
  "description": "Expose filesystem operations through MCP.",
  "transport": {
    "type": "stdio",
    "command": "node",
    "args": ["server.js"],
    "env": {
      "LOG_LEVEL": "info"
    },
    "cwd": "."
  },
  "tools": {
    "enabled": ["read_file", "list_directory"],
    "disabled": []
  }
}
```

Streamable HTTP 示例：

```json
{
  "schema_version": 1,
  "name": "docs",
  "description": "Expose documentation search through MCP.",
  "transport": {
    "type": "streamable_http",
    "url": "https://example.com/mcp",
    "bearer_token_env": "DOCS_MCP_TOKEN"
  }
}
```

规则：

- `schema_version` 必填。
- `name` 必填，必须是稳定 server identity。
- `description` 必填，用于 UI、ToolSearch source description、prompt guidance。
- `transport.type` 第一版只允许 `stdio` 和 `streamable_http`。
- stdio `cwd` 可选；相对路径必须解析到 plugin install root 内。
- stdio `command` 可以是 PATH 命令或 plugin root 内相对可执行文件。
- HTTP token 不允许明文落盘，必须使用 env var 或未来 secret ref。
- unknown fields 默认拒绝，除非明确标记为 typed extension point。

### `PluginDefinition`

目标态：

```rust
pub struct Plugin {
    pub id: PluginId,
    pub kind: PluginKind,
    pub metadata: PluginMetadata,
    pub documents: Vec<PluginDocumentRef>,
    pub source_id: Option<MarketplaceSourceId>,
    pub path_in_source: Option<String>,
    pub source_commit: Option<String>,
    pub status: PluginStatus,
    pub definition: PluginDefinition,
    pub warnings: Vec<PluginWarning>,
}

pub struct PluginMetadata {
    pub name: String,
    pub display_name: Option<String>,
    pub description: String,
    pub short_description: Option<String>,
    pub icon: Option<String>,
    pub license: Option<String>,
}

pub struct PluginDocumentRef {
    pub role: PluginDocumentRole,
    pub path: String,
    pub title: String,
    pub media_type: PluginDocumentMediaType,
}

pub enum PluginDefinition {
    Skill(SkillPlugin),
    McpServer(McpServerPlugin),
    Package(PackagePlugin),
    Feature(FeaturePlugin),
}

pub struct SkillPlugin {
    pub skill_md_path: String,
}

pub struct McpServerPlugin {
    pub server: McpServerDefinition,
    pub manifest_path: String,
}
```

`Plugin.metadata` 是列表、搜索、详情页通用展示字段的唯一归属。`Plugin.documents` 是详情页可读材料的统一索引。`SkillPlugin` 和 `McpServerPlugin` 只保存各自领域真正不同的执行语义和 manifest 路径，不重复定义 icon、display name、description 等公共展示字段。

### `PluginInstall`

目标态：

```rust
pub struct PluginInstall {
    pub id: PluginInstallId,
    pub plugin_id: PluginId,
    pub kind: PluginKind,
    pub name: String,
    pub installed_path: PathBuf,
    pub exposed_skill_root: Option<PathBuf>,
    pub exposed_mcp_servers: Vec<InstalledMcpServer>,
    pub source: PluginInstallSource,
    pub installed_at_ms: i64,
    pub updated_at_ms: i64,
    pub file_manifest: FileManifest,
}
```

说明：

- `exposed_skill_root` 保留给 Skill。
- `exposed_mcp_servers` 专属 MCP，不复用 Skill 字段。
- 不同 kind 的 exposed fields 可以为空，但不能混用语义。
- `PluginInstall` 仍是 plugin lifecycle 的单一安装记录。

### `InstalledMcpServer`

```rust
pub struct InstalledMcpServer {
    pub install_id: PluginInstallId,
    pub plugin_id: PluginId,
    pub server_name: String,
    pub display_name: Option<String>,
    pub description: String,
    pub installed_path: PathBuf,
    pub config: McpServerRuntimeConfig,
    pub enabled: bool,
}
```

### `McpServerRuntimeConfig`

```rust
pub struct McpServerRuntimeConfig {
    pub name: String,
    pub origin: McpServerOrigin,
    pub transport: McpTransportConfig,
    pub required: bool,
    pub startup_timeout_ms: u64,
    pub tool_timeout_ms: u64,
    pub supports_parallel_tool_calls: bool,
    pub tool_filter: McpToolFilter,
}

pub enum McpServerOrigin {
    Config,
    Plugin {
        plugin_id: PluginId,
        install_id: PluginInstallId,
    },
}

pub enum McpTransportConfig {
    Stdio {
        command: String,
        args: Vec<String>,
        env: Vec<McpEnvVar>,
        cwd: PathBuf,
    },
    StreamableHttp {
        url: String,
        headers: Vec<McpHttpHeader>,
        bearer_token_env: Option<String>,
    },
}
```

配置字段必须从 plugin install root、runtime config、secret/env 解析后进入 runtime config。`talos-mcp` 不负责猜测默认路径。

### MCP tool descriptor

```rust
pub struct McpToolDescriptor {
    pub server_name: String,
    pub server_origin: McpServerOrigin,
    pub raw_tool_name: String,
    pub callable_namespace: String,
    pub callable_name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub input_schema: ToolInputSchema,
    pub annotations: McpToolAnnotations,
}
```

规则：

- `raw_tool_name` 用于回调 MCP server。
- `callable_namespace` / `callable_name` 用于模型可见 tool name。
- model-visible name 必须稳定、合法、去重。
- 名称冲突必须通过确定性 suffix 或 explicit conflict error 处理，不能随机。

## Plugin 扫描与安装

Marketplace scan 需要识别两类第一版 plugin：

```text
skills/<name>/SKILL.md
mcp/<name>/talos-mcp.json
```

也允许后续 manifest 支持一个 plugin 同时包含 skill 和 MCP server，但第一版建议保持一个 plugin 一个主要 kind，避免生命周期和 UI 语义混乱。

扫描规则：

- Skill plugin 继续按 `SKILL.md` 解析。
- MCP server plugin 按 `talos-mcp.json` 解析。
- 嵌套 plugin 继续产生 warning。
- manifest 路径必须在 source root 内。
- source sparse path 不存在时跳过。
- 单个 plugin manifest 无效时只跳过该 plugin，不丢弃同 source 其它 plugins。

安装规则：

- MCP plugin 文件复制到 `${TALOS_HOME}/plugins/installs/<install-id>/mcp/<server-name>/`。
- 生成 `PluginInstall.exposed_mcp_servers`。
- 相对 `cwd`、相对 executable、相对 resource path 都以 installed MCP server root 为基准解析。
- 安装事务继续使用 temp dir、atomic rename、rollback、state lock。
- 更新不能先删除旧 install 再写新 install。

启停规则：

- `plugin.enable` / `plugin.disable` 是 MCP server plugin 启停的唯一管理入口。
- disabled MCP plugin 不进入 runtime MCP config。
- 启停状态必须纳入 plugin lifecycle state，不借用 `skills-state.json`。
- 对 Skill 的 enable state 和 MCP 的 enable state 可以共享统一 plugin state 文件，但字段必须强类型区分。

## Runtime 组装流程

启动时：

```text
Config::load
  -> PluginManagerService::runtime_capabilities
     -> enabled skill roots
     -> enabled installed MCP servers
  -> merge config MCP servers + plugin MCP servers
  -> talos_mcp::McpRuntime::start
  -> talos_mcp::McpRuntime::list_tools
  -> talos_tools::register_mcp_tools
  -> AgentHarness / ToolRegistry ready
```

配置或 plugin 状态变化时：

```text
plugin.install/update/uninstall/enable/disable
  -> PluginManager state updated
  -> Application runtime rebuilds MCP server set
  -> talos-mcp starts/stops changed servers
  -> ToolRegistry refreshes MCP tool projections
  -> next turn sees updated model-visible tools
```

第一版可以选择“下一轮生效”，不要求 turn 中途热插拔工具。若当前 turn 正在执行，MCP runtime refresh 应等待 turn 边界或使用 snapshot，避免模型看到的 tool schema 与执行 registry 不一致。

## Tool exposure 策略

Tool 注册和模型上下文可见性必须分离：

- 系统内置 tools 默认可进入模型上下文。
- 外部 tools 必须先注册进 `ToolRegistry` catalog，但默认不进入模型上下文。
- 外部 tools 只能通过 `ToolSearch` 在 session 内按需激活后进入后续 model step 的 tool definitions。
- MCP tools、plugin 暴露 tools、external dynamic tools 都属于外部 tools。

这个规则不按工具数量、server 来源或用户 mention 做 direct-visible 例外。目的是避免上下文爆炸和外部 tool schema 污染模型上下文。

```text
MCP discovery
  -> McpToolDescriptor[]
     -> ToolRegistry catalog
        -> ToolSearch(query)
           -> activate matching external tools in session state
           -> next model step in the same user turn can call activated tools
```

`ToolSearch` 需要增强：

- namespace-aware。
- 搜索 raw MCP tool name、callable name、server name、plugin display name、description、schema parameter names。
- 返回 unavailable reason。
- 支持 source summary。
- 激活结果必须写入 session-scoped deferred tool state。

## Prompt 集成

PromptBuilder 应注入三类信息：

### Available Skills

沿用 `feat-026`：

- 可用 Skill summary。
- 用户显式选择或提到后注入完整 `SKILL.md`。
- Skill 不进入 `ToolRegistry`。

### Available Plugins

用于告诉模型已安装 plugin capabilities：

- plugin name。
- plugin kind。
- description。
- 若是 MCP plugin，说明其 tools 需要通过 `ToolSearch` 发现并激活。

### MCP Tool Discovery Guidance

当存在 deferred MCP tools 时，PromptBuilder 可以注入简短说明：

```text
Some MCP tools may not be visible by default. Use ToolSearch to discover tools by plugin, server, capability, or parameter name.
```

约束：

- 不列出完整 MCP JSON schema 到 prompt，tool declaration 是 schema 主通道。
- 不要求模型构造 MCP JSON-RPC。
- 不把 MCP resources 当作 apps/skills 的替代入口。
- Prompt 只做 discoverability guidance。

## Tool 调用流程

```text
Model emits tool call:
  namespace/name or mcp__server__tool
    |
    v
ToolRouter resolves RegisteredToolSpec
    |
    v
McpToolHandler maps callable name to:
  server_name + raw_tool_name
    |
    v
talos_mcp::McpRuntime::call_tool
    |
    v
MCP server returns CallToolResult
    |
    v
McpToolHandler maps result to:
  ToolOutputPayload
  ToolResultEnvelope
  TalosContentItem[]
    |
    v
Kernel streams event + feeds compact result back to model
```

MCP tool result 映射规则：

- text content 映射为 `Text` content item。
- structured content 映射为 canonical JSON content item 或 `structured_content` 附加字段。
- resource reference 映射为 `McpResourceRef`。
- binary/blob content 必须持久化为 artifact ref，不能直接塞进模型上下文。
- MCP `is_error` 必须映射为 Talos failure status，并保留 server/tool/error context。
- 大输出必须走统一 truncation / artifact 策略。

## Resource 工具

第一版系统 MCP resource tools：

```text
ListMcpResources
ListMcpResourceTemplates
ReadMcpResource
```

参数：

- `server` 可选用于 list，必填用于 read。
- `cursor` 支持分页。
- `uri` 必填用于 read。

返回：

- resource list 必须保留 `server`、`uri`、`name`、`description`、`mime_type`、`next_cursor`。
- template list 必须保留 `server`、`uri_template`、`name`、`description`、`mime_type`、`next_cursor`。
- read text resource 返回 `McpResourceRef` + `Text`。
- read blob resource 返回 `McpResourceRef` + `ArtifactRef`。

## 权限与安全

MCP tools 不能绕过 Talos tool policy。

约束：

- MCP tool 进入 `RegisteredToolSpec` 时必须设置 `ToolBehaviorFlags`。
- 优先使用 MCP annotations，例如 read-only / destructive / idempotent hints。
- annotations 缺失时采用保守策略：未知 MCP tool 不默认视为 read-only。
- plugin manifest 可补充 per-tool policy override，但必须强类型。
- stdio MCP server 的 command/cwd/env 必须经过路径和环境变量校验。
- HTTP bearer token 不允许明文写入 plugin manifest。
- MCP tool call 需要进入现有 tool event / audit / replay。
- MCP server 启动 stderr 可进入日志，但不能进入模型上下文。
- plugin-owned MCP server 默认在 plugin install root 或 configured workspace 下运行，不继承任意 cwd。

## 错误处理

错误必须 fail fast，并包含可行动上下文：

| 场景 | 行为 |
| --- | --- |
| manifest schema invalid | marketplace scan warning，plugin 不进入可安装结果 |
| install path escapes root | install fail |
| disabled plugin | 不进入 runtime config |
| required server startup fail | runtime 启动失败或 session warning，按 required 策略处理 |
| optional server startup fail | server status failed，tools unavailable |
| tools/list fail | server tools unavailable，保留 startup/error status |
| tool schema invalid | 该 tool unavailable，不影响同 server 其它 valid tools |
| tools/call fail | 返回 ToolResult failure，包含 server/tool/error |
| resources/list unsupported | `ListMcpResources` 返回 method unsupported failure |
| read blob persist fail | `ReadMcpResource` failure，不吞错 |

禁止：

- 静默跳过 enabled MCP plugin。
- 用空 tool list 掩盖 server 启动失败。
- 因 optional MCP server 失败导致所有 tools 不可用。
- 对 schema parse 失败使用字符串 fallback 注册工具。

## 配置

静态 config MCP servers 仍允许存在，用于本地开发和手动接入：

```yaml
agent:
  kernel:
    mcp:
      servers:
        - name: docs
          required: false
          transport:
            type: streamable_http
            url: https://example.com/mcp
            bearer_token_env: DOCS_MCP_TOKEN
        - name: filesystem
          transport:
            type: stdio
            command: node
            args:
              - server.js
            cwd: /absolute/path/to/server
```

Plugin MCP servers 和 config MCP servers 合并规则：

- 合并发生在 application runtime 层。
- `server_name` 必须全局唯一。
- config server 与 plugin server 同名时，默认 fail fast；后续可以设计 explicit override。
- disabled config server 不进入 runtime。
- disabled plugin server 不进入 runtime。
- plugin server origin 必须保留到 `McpToolDescriptor` 和 telemetry。

## UI / SDK / Protocol 影响

需要同步更新：

- plugin list/search/read 展示 MCP server plugin manifest。
- plugin install response 返回 `exposed_mcp_servers`。
- plugin enable/disable 后前端能看到 MCP server enabled 状态。
- tools/timeline events 能展示 MCP server/tool/status。
- SDK 暴露同一组 `talos-types` 对象。
- JSON-RPC 和 HTTP 不复制 MCP DTO。

MCP tool call timeline 建议至少包含：

- server name。
- raw tool name。
- callable tool name。
- plugin origin。
- status。
- duration。
- error kind。

## 与既有 spec 的关系

- `feat-026-talos-skills-system`：Skills 仍只负责 discovery 和 prompt injection；本 spec 不改变 Skill 本体。
- `feat-027-plugin-marketplace-skill-install`：Plugin Manager 的生命周期模型扩展到 `McpServer` kind；该 spec 中“future MCP installer”由本 spec 补齐。
- `refactor-025-talos-tool-capability-alignment`：MCP tool result 必须遵守 canonical `ToolResultEnvelope` / `TalosContentItem` 规则；本 spec 是其 MCP 部分的具体化。

## 验证策略

### Unit tests

- parse valid stdio MCP manifest。
- parse valid streamable HTTP MCP manifest。
- reject unsupported SSE transport。
- reject relative cwd escaping plugin root。
- reject inline bearer token。
- scan source 同时包含 skill plugin 和 MCP plugin。
- install MCP plugin 生成 `PluginInstall.exposed_mcp_servers`。
- disable MCP plugin 后 runtime capabilities 不包含该 server。
- merge config/plugin MCP servers detects duplicate names。
- MCP descriptor name normalization is deterministic。
- MCP annotations map to `ToolBehaviorFlags`。
- tool schema invalid only disables that tool。
- resource blob read persists artifact ref。

### Integration tests

- 启动一个 test MCP stdio server，discovery 后注册工具，模型 tool call 路由到 server。
- 启动一个 test streamable HTTP MCP server，完成 tools/list 和 tools/call。
- plugin install -> enable -> runtime refresh -> ToolRegistry 出现 MCP tools。
- plugin disable -> next turn ToolRegistry 不再出现对应 MCP tools。
- MCP tools 默认进入 deferred catalog，通过 `ToolSearch` 激活。
- Skill 提到 MCP capability 时，PromptBuilder 注入 Skill，MCP tool 仍通过 ToolRegistry 调用。

### E2E tests

- 通过 HTTP / JSON-RPC 安装 MCP plugin。
- WebUI 或 TUI 能看到已安装 MCP plugin 和其 server 状态。
- 真实用户 turn 中模型调用 MCP tool，并在 timeline 中看到 begin/end/result。
- MCP server 启动失败时 UI 显示可行动错误，不影响普通 tools 和 Skills。

## Remaining risks

- `rmcp` API 仍可能变化，`talos-mcp` 必须隔离第三方类型，避免全仓库扩散。
- MCP tool naming 与模型 provider 的 tool namespace 限制需要在 provider abstraction 下统一处理。
- plugin-owned MCP server 的进程沙箱策略需要和 Bash / tools-sandbox 对齐。
- MCP OAuth / elicitation 不进入第一版，但 HTTP MCP 迟早需要统一 secret 和 auth 流程。
- ToolSearch 已作为外部 tool 按需可见入口，后续仍需要继续优化排序、解释性和大规模 MCP catalog 下的检索质量。

## 结论

Talos MCP 的目标态是：

```text
Plugin Manager owns MCP server lifecycle.
talos-mcp owns MCP protocol runtime.
ToolRegistry owns model-callable MCP tool projections.
PromptBuilder owns guidance and Skill injection.
```

MCP 和 Skills 都是 plugin 能力，但不是同一种运行路径。Skills 通过 prompt 教模型如何做事；MCP 通过 tools 给模型可执行能力。这个分工是本 spec 后续实现的硬边界。
