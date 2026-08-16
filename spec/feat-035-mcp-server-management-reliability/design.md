# MCP Server 管理与可靠性设计

Status: draft
Owner: agent
Created: 2026-08-06
Updated: 2026-08-06
Related: `archive/2026/feat-028-talos-mcp-system`, `archive/2026/feat-027-plugin-marketplace-skill-install`

## 背景与问题

Talos 已经完成 MCP 的第一版接入：`talos-mcp` 基于 `rmcp` 连接外部 MCP server，`talos-plugin` 管理 `PluginKind::McpServer` 的安装态，`talos-tools` 将 MCP tools 投影为模型可调用 tools。当前缺口集中在两类：

1. Settings 已经有 MCP server 页面和表单骨架，但手动添加 MCP server 的保存按钮仍不可用，后端没有对应的手动安装 API。
2. MCP server 可能是重型三方服务，启动慢、挂起、崩溃、stderr 不断输出、tool call 卡住或依赖外部网络。当前 runtime 有基础超时和 failed snapshot，但连接管理、启动隔离、刷新、进程清理和前端可观测状态还不够完整。

本设计不重新定义 MCP 架构。MCP server 的生命周期仍归 Plugin Manager；MCP 协议执行仍归 `talos-mcp`；模型可调用投影仍归 `talos-tools`。本设计只补齐手动添加入口和运行时可靠性边界。

## 设计结论

1. 手动添加 MCP server 不直接写 `talos.yaml`，也不新增独立 `McpSettingsStore`。它应作为 `PluginInstallSource::Manual` 的 `PluginKind::McpServer` 安装进入现有 plugin lifecycle。
2. 手动 MCP 的落盘形态仍是托管安装目录中的 `talos-mcp.json`，由 plugin install state 记录 `install_id`、`plugin_id`、`installed_path`、enabled 状态和 digest。
3. stdio MCP server 已经是独立子进程，不需要第一阶段把整个 `talos-mcp` client runtime 再搬到 worker 进程。第一阶段隔离的是子进程崩溃、协议失败和调用阻塞，不承诺隔离 CPU、内存或系统级 OOM。
4. 可靠性优化的第一优先级是：坏 server 不阻塞 Talos 启动，不改变其他 MCP server 的连接状态，不阻塞主 agent loop，并且错误状态可被 UI/API 查询。
5. `talos-mcp` 应抽出 stdio process launcher 边界。第一版实现本地子进程 launcher；后续如果需要 executor/sidecar，只替换 launcher，不改 Plugin Manager 或 Tool Router。
6. Runtime reconcile 必须成为一等能力。plugin install/enable/disable/uninstall 和手动 MCP 保存后，应按 descriptor diff 增量 reconcile MCP runtime，并原子替换 ToolRegistry 的 MCP tool 投影。
7. ToolRegistry 不拥有 MCP lifecycle。它只拥有当前可见 tool spec 和执行 handler；runtime 连接、重连、关闭和进程清理由 `talos-mcp` 管。
8. Settings 页面展示 installed plugin 状态和 runtime snapshot 状态两个维度：已安装/启用属于 plugin lifecycle；starting/ready/failed/exited 属于 MCP runtime。
9. MCP server 启动采用后台并发连接。plugin mutation 在 desired catalog 已发布后返回，不等待 server 进入 `Ready`。
10. 第一版支持手动添加 stdio 和 streamable HTTP MCP server；manual HTTP 的 custom header 和 bearer authentication 只支持环境变量引用，不支持固定 header value、明文 secret 或 OAuth。

## 目标

1. Settings 支持手动添加 stdio 和 streamable HTTP MCP server。
2. Settings 支持查看、启用、禁用、卸载手动 MCP 和 marketplace MCP。
3. 手动 MCP 复用现有 plugin list/read/install/enable/disable/uninstall 语义，不绕过 plugin state。
4. MCP startup 在后台并发执行，单个 server 启动失败或超时不阻塞 Talos 对外可用，也不重启或关闭其他 server。
5. MCP runtime reconcile 后，新 turn 能看到最新 MCP tool catalog；已开始的 tool call 不被无故中断。
6. stdio MCP 子进程有明确的 stderr 日志、process group 终止、shutdown 和异常退出状态。
7. MCP tool call 和 resource read 有确定性超时、错误分类和 runtime 状态更新。
8. 前端能展示 runtime failure message，用户可以区分“插件已启用但 server 启动失败”和“插件已禁用”。
9. 保持 `rmcp` 为唯一 MCP 协议实现，不恢复历史手写 MCP client。
10. stdio MCP 子进程只获得 launcher 明确解析的环境变量，不默认继承 Talos 主进程的完整环境。

## 非目标

1. 不实现 Talos 作为 MCP server 对外暴露能力。
2. 不新增 MCP marketplace 分发协议。
3. 不实现完整 MCP worker/sidecar 进程模型；只预留 launcher 边界。
4. 不支持 legacy SSE transport。
5. 不实现自动安装 npm/pip/package dependency。
6. 不实现 tool-level 权限 UI、OAuth 登录 UI 或 connector marketplace。
7. 不引入 fallback 让失败 server 静默可用。
8. 不让 frontend 直接修改 YAML 或真实默认配置。
9. 不把 Plugin Manager 扩展成进程 supervisor。
10. 不通过第一阶段 launcher 提供 CPU、内存、文件系统或网络资源配额；这些能力需要 executor/sidecar。
11. 不实现自动重连。transport failure 更新 runtime 状态，用户可通过 disable/enable 或后续显式 restart 能力恢复。

## 当前实现事实

### MCP runtime

`talos-mcp` 当前基于 `rmcp`：

```rust
type RmcpService = RunningService<RoleClient, TalosMcpClientHandler>;

pub struct TalosMcpRuntime {
    servers: Arc<RwLock<HashMap<String, ManagedMcpServer>>>,
}
```

stdio transport 已经通过 `tokio::process::Command` 启动子进程：

```rust
let mut cmd = Command::new(command);
cmd.kill_on_drop(true)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .args(args)
    .envs(env);
```

已有保护：

- startup timeout: 30 seconds。
- tool call timeout: 120 seconds。
- `ManagedMcpServer::Failed` snapshot。
- `shutdown()` 通过 cancellation token 关闭 `rmcp` service。

当前不足：

- `connect_all()` 通过 `refresh()` 串行连接 server。
- stderr pipe 没有被持续 drain 和归属日志。
- 没有 process group 终止，子进程的孙进程可能残留。
- `refresh()` 会替换整个 map 并 shutdown previous servers，缺少 active call 保护。
- startup/exit/failure 状态没有对 Settings 提供稳定 API。
- tool call 出现 transport broken 后没有明确更新 server 状态或触发受控重连。

### Plugin lifecycle

`talos-plugin` 已有 `PluginInstallSource::Manual`，但当前 `PluginInstallTarget` 只有 marketplace/local scanned plugin：

```rust
pub enum PluginInstallTarget {
    Plugin {
        plugin_id: PluginId,
        expected_source_commit: Option<String>,
    },
}
```

enabled MCP plugin 通过 installed manifest 转为 runtime descriptor：

```rust
pub(crate) fn installed_mcp_servers(
    install: &PluginInstall,
) -> TalosResult<Vec<McpServerRuntimeDescriptor>>
```

这说明手动 MCP 不需要新增一套 store。它缺的是一个能创建托管 `talos-mcp.json` 并写入 install state 的 API target。

### App startup and ToolRegistry

`talos-base` 当前在启动时合并配置 MCP 和 plugin MCP：

```rust
let mut servers = configured_mcp_servers(kernel_config)?;
servers.extend(plugin_manager.mcp_servers()?);
let runtime = Arc::new(talos_mcp::TalosMcpRuntime::connect_all(servers).await);
let manager = Arc::new(McpConnectionManager::from_runtime(runtime).await);
tool_registry.register_mcp_server(manager);
```

`ToolRegistry::register_mcp_server()` 将当前 snapshot 中的 MCP tools 写入 registry。当前 registry 没有 MCP refresh/unregister 语义。因此如果用户在 Settings 中 enable/disable/uninstall MCP plugin，UI 的 plugin state 会更新，但已注册到工具系统的 MCP tool catalog 可能不会同步。

### Frontend Settings

`SettingsMcpServersSection` 已经列出 managed MCP plugins，并打开 create/update 表单。`McpServerForm` 支持 stdio 和 HTTP 字段，但 create save 仍 disabled。页面当前只消费 plugin list，不消费 runtime snapshot。

因此本设计应让现有表单变成真实 API 调用方，而不是另起一个 MCP Settings 页面。

## 对 Codex 的参考判断

Codex 相关设计提供三点可借鉴结论：

1. `codex-mcp::McpConnectionManager` 聚合多个 MCP clients、startup status、tool catalog、resource 和 tool call routing。
2. `codex-rmcp-client::RmcpClient` 封装 rmcp lifecycle；stdio process placement 通过 `StdioServerLauncher` 抽象。
3. `LocalStdioServerLauncher` 和 `ExecutorStdioServerLauncher` 返回同一类 rmcp transport，MCP 协议层不需要知道 server 进程运行在哪里。

Talos 不复制 Codex 的目录和产品对象，但采用同一边界判断：

```text
Plugin lifecycle owns server availability.
MCP runtime owns protocol lifecycle and process placement.
Tool registry owns model-callable projections.
Settings consumes lifecycle and runtime state.
```

## 手动 MCP lifecycle 设计

### API 模型

优先扩展现有 `PluginInstallRequest`，不要新增 `McpServerCreateRequest` 平行管理栈：

```rust
pub enum PluginInstallTarget {
    Plugin {
        plugin_id: PluginId,
        expected_source_commit: Option<String>,
    },
    ManualMcpServer {
        server: McpServerDefinition,
    },
}
```

响应仍使用现有：

```rust
pub struct PluginInstallResponse {
    pub plugin: Plugin,
    pub install: PluginInstall,
}
```

理由：

- 手动 MCP 安装后和 marketplace MCP 没有生命周期差异。
- `PluginInstallSource::Manual` 已存在，正好表达来源。
- 前端继续使用 `plugin.install`、`plugin.enable`、`plugin.disable`、`plugin.uninstall` 一套 SDK surface。
- 不产生 `Plugin` 与 `ManualMcpServer` 两套相似对象。

### Manual plugin object

后端收到 `ManualMcpServer` 后生成一个 synthetic `Plugin`：

```text
Plugin
  id: deterministic from MCP server name
  kind: McpServer
  source_id: None
  source_commit: None
  path_in_source: None
  status: Installed
  definition: McpServer(McpServerPlugin)
```

`plugin_id` 必须确定性生成，建议基于 validated canonical server name：

```text
manual/mcp/<server-name>
```

Manual server name 使用 canonical lowercase form，必须匹配 `[a-z0-9][a-z0-9_-]{0,63}`。后端拒绝 uppercase、whitespace、`.`、path separator、absolute path prefix 和任何 normalization 后发生变化的输入，不替用户静默改名。安装目录只能由 validated name 通过 plugin installer 的 safe path join 派生，并验证最终路径仍位于 managed MCP install root 内。

如果同名 manual MCP 已存在，`PluginInstallConflictPolicy::Reject` 返回 conflict；`Replace` 走现有 replace install 流程。现有 app/service mutation handler 还必须在写入 plugin state 前检查 candidate server name 是否与 configured MCP 或其他 installed MCP 冲突。Plugin Manager 不读取 Talos config，也不承担全局名称校验。

Runtime 不使用 server name 作为内部连接 identity。稳定 key 由 source 决定：

```text
configured MCP: config/<server-name>
plugin MCP:     plugin/<install-id>/<server-name>
```

`server.name` 仍是面向 tool namespace 的全局唯一名称。稳定 key 用于 runtime reconcile 和 Settings 状态关联；全局名称冲突返回显式 conflict，不自动重命名，也不覆盖任意一个已有 server。

Installed MCP descriptor discovery 使用一个 internal result 隔离单个 plugin manifest error：

```rust
pub struct McpServerDiscoveryResult {
    pub descriptors: Vec<McpServerRuntimeDescriptor>,
    pub warnings: Vec<PluginWarning>,
}
```

Plugin manifest 无法解析时，该 install 不进入 desired runtime catalog，并产生带 path 和 error context 的 plugin warning。Settings 显示 plugin discovery error，而不是伪造 runtime snapshot。其他 valid descriptors 继续 reconcile。

Configured MCP 在 config commit 前完成整体验证。Config validation 失败时拒绝 config mutation，不触发 runtime reconcile，也不改变当前 desired catalog。

### 落盘布局

手动 MCP 使用现有 MCP install 目录，不写入 config YAML：

```text
<plugin-state>/
  installs.json
  mcp/
    <server-name>/
      talos-mcp.json
```

`talos-mcp.json` 使用和 marketplace MCP plugin 一致的 manifest schema。这样 `installed_mcp_servers()` 可以继续通过 `server_from_installed_mcp_manifest()` 读取，不需要手动路径专用分支。

### Secret and environment

stdio transport 的 `env` 可以保存非敏感值。敏感 token 不应以明文保存在 `talos-mcp.json`。第一版增加显式环境变量透传列表：

```rust
Stdio {
    command: String,
    args: Vec<String>,
    env: BTreeMap<String, String>,
    inherit_env: Vec<String>,
    cwd: Option<PathBuf>,
}
```

`inherit_env` 只保存允许从 Talos host environment 读取的变量名，不保存变量值。launcher 默认不继承 Talos 主进程的完整 environment，只注入：

1. 运行命令所需的 platform bootstrap allowlist。
2. descriptor 中的固定 `env`。
3. `inherit_env` 明确列出的 host environment values。

缺失的 `inherit_env` variable 在启动前返回带 server context 的 validation failure。

HTTP 第一版支持以下集合：

```rust
pub enum McpTransportDefinition {
    Stdio {
        command: String,
        args: Vec<String>,
        env: BTreeMap<String, String>,
        inherit_env: Vec<String>,
        cwd: Option<PathBuf>,
    },
    StreamableHttp {
        url: String,
        headers: BTreeMap<String, String>,
        env_headers: BTreeMap<String, String>,
        bearer_token_env_var: Option<String>,
    },
}
```

说明：

- `headers` 只为已有 config/marketplace descriptor 保留兼容读取。`ManualMcpServer` request 不接受固定 header value。
- `env_headers` 表示 header name 到环境变量名的映射，是 manual HTTP 自定义 header 的唯一输入方式。
- `bearer_token_env_var` 表示后端在发起 HTTP request 时从 runtime env 读取 bearer token。
- Header name 需要归一化并拒绝冲突；`Authorization` 由 bearer token 字段独占。
- 所有 `env_headers` 和 `bearer_token_env_var` 在 connection startup 前一次性解析。环境变量缺失或 header 非法时，该 server 进入带上下文的 `Failed`，不发起 HTTP request。
- Resolved headers 在当前 server generation 内保持不变；host environment 变化需要显式 disable/enable 或后续 restart 才生效。
- 前端不提供固定 header value 或明文 token 输入框。
- OAuth 和持久化 secret storage 不在本设计范围内。

## Runtime reliability 设计

### Runtime owner

`talos-mcp` 继续是唯一 runtime owner，增加清晰的连接管理职责：

```text
TalosMcpRuntime
  owns configured descriptors
  owns managed server states
  owns rmcp service handles
  owns stdio process launcher
  exposes snapshots
  exposes reconcile
  exposes call/read APIs
```

`talos-plugin` 只输出 enabled MCP runtime descriptors，不持有进程句柄，不读取 `rmcp` 类型。

`talos-tools` 只持有 MCP execution adapter，不直接启动、停止或重连 server。

### Reliability boundary

第一阶段提供以下保证：

- stdio child process crash/exit 不直接终止 Talos main process。
- startup、tool call 和 resource read timeout 不占住主 agent loop。
- 单个 server 的 protocol/startup failure 不改变 Unchanged server 的 runtime state。
- shutdown 能终止该 server 的 process group 并持续 drain stderr。

第一阶段不提供资源 sandbox。stdio child 仍与 Talos 共享 host CPU、memory、filesystem 和 network；极端资源耗尽仍可能影响 Talos 或触发系统 OOM。出现需要运行不可信 server、需要 CPU/memory quota，或 rmcp client failure 必须与 main process 隔离的产品要求时，再启用 executor/sidecar 方案。

### Startup

MCP startup 应并发连接 server：

```text
publish desired catalog
  -> each enabled server enters Starting
  -> Talos app/server becomes available
  -> connect servers concurrently in background
  -> list tools/resources with timeout
  -> publish Ready or Failed per server
  -> atomically refresh MCP tool projection
```

Talos app/server startup 不等待 MCP server 进入 `Ready`。MCP tools 只在对应 server 完成 initialize 和 list tools 后进入 registry。早期 turn 不发现仍为 `Starting` 的 tools；Settings 通过 runtime snapshot 明确展示 `Starting`，而不是把该 server 展示为 disabled 或 missing。

stdio child spawn 成功后，initialize、list tools 或 list resources/templates 任一步失败或超时，startup task 必须关闭 rmcp service 和对应 process tree，最后发布 `Failed`。清理失败附加到 failure context，但不能把 server 发布为 `Ready`，也不能遗留无人管理的 child。

### State model

扩展 runtime status，避免所有失败都折叠成一条 message：

```rust
pub enum McpServerStatus {
    NotStarted,
    Starting,
    Ready,
    Failed { message: String },
    Exited { message: String },
    Disabled,
}
```

`Failed` 表示 validation、initialize/list tools/readiness、operation timeout 或 transport/service failure。stdio process 在 `Starting` 阶段退出也归 `Failed`，message 包含 exit context。

`Exited` 只表示 server 曾经进入 `Ready`，之后发生非 runtime shutdown 发起的自然或异常 process exit。Runtime 主动 shutdown 产生的 expected exit event 只用于资源清理，不得覆盖当前失败状态。HTTP transport 无本地 process，不进入 `Exited`。

`Disabled` 为现有 schema compatibility 保留，但 disabled plugin 不进入 desired runtime catalog，Settings 不应期待 disabled plugin 存在 runtime snapshot。

### Incremental reconcile

Runtime reconcile 触发条件：

- manual MCP install/update/uninstall。
- MCP plugin enable/disable。
- marketplace MCP plugin install/update/uninstall。
- config MCP reload。

app/service 组装层重新读取 configured MCP 和 enabled plugin MCP descriptors，形成 desired catalog。`talos-mcp` 使用稳定 runtime key 和 descriptor digest 计算 diff：

| Diff | Runtime behavior |
| --- | --- |
| Unchanged | 复用现有 state、rmcp service 和 tool catalog，不重连 |
| Added | 发布 `Starting`，后台连接，完成后进入 `Ready` 或 `Failed` |
| Changed | 立即从新 tool projection 移除旧 tools，发布 `Starting`，后台启动新 service |
| Removed | 立即从新 tool projection 移除 tools，不再接受新调用，并关闭旧 service |

Reconcile 顺序：

1. 校验 desired catalog 的 stable key、server name 和 descriptor。
2. 在一个 publish 操作中写入新的 desired runtime states，并原子替换当前 MCP tool projection。Removed/Changed server 的旧 tools 从此不再被新 turn 发现。
3. 为 Added/Changed server 启动后台连接任务；每个任务只更新自己的 runtime state。
4. server 进入 `Ready` 或 `Failed` 时，再次原子发布包含最新 ready tools 的 MCP projection。
5. Unchanged server 不受其他 server 的启动、失败或退出影响。

所有会改变 MCP desired catalog 的 plugin mutation 和 config reload 都通过现有 app/service 组装层的同一个 async critical section 串行执行。该 critical section 只覆盖 validate、prepare、plugin/config commit 和 desired state publish，不等待后台 startup 或旧 server drain。

Runtime 必须有 generation/revision guard。旧 startup task、in-flight call failure、operation timeout、process exit 或 shutdown completion 只能清理自己的旧 generation，不得把当前 generation 标记为 `Failed` 或 `Exited`，也不得修改当前 tool projection。

Removed/Changed server 的新 tool call 在 desired state 发布后立即拒绝。已经捕获旧 service handle 的 in-flight call 可以继续完成；后台 drain 有固定上限，到期后关闭旧 process group。Plugin mutation response 不等待 drain。

不得出现：

- reconcile 后继续展示已删除或已变更 server 的旧工具。
- enable 成功但 runtime 没刷新。
- disable 成功但工具仍可被新 turn 发现。
- 为了保留可用性在后台 silently fallback 到旧 server。
- 一个 server 的 mutation 导致 Unchanged server 被重连或关闭。

### ToolRegistry projection

`ToolRegistry` 需要支持原子替换 MCP namespace 的 tool specs：

```text
replace_mcp_projection(manager)
  build complete next MCP spec map
  publish the complete map in one write critical section
  keep built-in and dynamic tool maps untouched
```

Tool discovery 只能看到旧 MCP projection 或新 MCP projection，不能看到 remove/register 中间态。MCP handler 应引用一个 runtime handle，而不是把每次 snapshot 的工具列表当成永久事实。工具发现使用 registry 中的当前 MCP specs；执行时通过 stable runtime key 获取并捕获当前 server generation 和 service handle。

### Stdio launcher

从 `connect_service()` 中抽出 stdio launcher 边界。launcher 返回 rmcp 可消费的 transport，同时把 process ownership 留给 `talos-mcp` runtime 管理。`rmcp` 不应直接拥有 child lifecycle；Plugin Manager 也不应知道 process handle。

本轮只实现 local launcher，职责包括：

- 解析 command、args、env、cwd。
- 清空默认 inherited environment，并只注入 bootstrap allowlist、固定 `env` 和显式 `inherit_env`。
- 持续 drain stderr 并写入带 server context 的 tracing。
- 监听 child exit，区分 natural exit 和 runtime shutdown。
- shutdown 时终止完整 process tree，并保证 setup 失败或 handle drop 时有 best-effort cleanup。

具体 trait 形状、Unix/Windows 进程组细节和 shutdown grace period 在实现阶段确定；spec 只要求 rmcp transport 与 process lifecycle 解耦，且本地子进程不会留下无人管理的后代进程。

### Tool call failure

第一版保持 startup timeout 30 seconds，并统一 tool call 和 resource read timeout 为 120 seconds。Timeout error 必须包含 server name、operation 和 timeout duration。

MCP tool call 失败分三类：

| 类型 | 行为 |
| --- | --- |
| Validation failure | 返回 tool failure，不改变 server readiness |
| Tool returned `is_error` | 作为 MCP tool result 返回给模型，不改变 server readiness |
| Tool/resource operation timeout | 取消当前 operation，关闭当前 service，标记 `Failed`，并移除该 server 的 tool projection |
| Transport/service failure | 返回 tool failure，把 server 标记为 `Failed`，并移除该 server 的 tool projection |

第一版不做自动重连。operation timeout 和 transport/service failure 必须原子更新状态并从下一次 MCP projection 中移除该 server 的 tools，然后关闭当前 rmcp service；stdio transport 还必须通过 process handle 完成 process-tree shutdown。Expected exit event 不再改变状态。清理在后台执行，不能阻塞主 agent loop。用户可通过 disable/enable 恢复；显式 restart 和 bounded reconnect 留给后续设计。

## Settings 设计

### 页面结构

沿用现有 `SettingsMcpServersSection`：

- server list 显示 plugin lifecycle 状态和 runtime 状态。
- create form 保存后调用 `plugin.install` with `ManualMcpServer` target。
- update mode 首版只允许 enable/disable/uninstall，不允许原地修改 transport。修改 transport 需要 replace install 或 uninstall 后重新 create。
- marketplace source list 保持展示来源统计；manual server 的 source 显示为 `manual`。

### 表单约束

stdio:

- `name` 必填，必须符合 MCP server name 约束。
- `command` 必填。
- `args` 为数组，不用 shell string 拼接。
- `env` key 必须非空且唯一。
- `inherit_env` 中只填写 host environment variable name，必须非空且唯一。
- `cwd` 可空；非空时必须是 absolute path，后端拒绝 relative path。Marketplace manifest 的 relative `cwd` 继续按 installed plugin directory 解析，manual MCP 不使用该规则。

streamable HTTP:

- `url` 必填，必须是 `http://` 或 `https://`。
- header key 必须非空且唯一。
- custom header value 和 bearer token 只能通过 env var 引用，不提供 fixed value 或 plaintext token 保存。

### Runtime status display

Settings 需要一个 runtime snapshot query：

```rust
pub struct McpRuntimeListRequest {
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

pub struct McpRuntimeListResponse {
    pub data: Vec<McpServerSnapshot>,
    pub next_cursor: Option<String>,
}
```

默认 `limit` 为 50，最大为 100；cursor 由 runtime stable key 派生。UI 对 plugin MCP 只通过 snapshot source 中的 `install_id` 与 plugin install 关联，不以可编辑或可归一化的 server name 做 join。Configured MCP 没有 plugin lifecycle row，直接使用 snapshot 中的 server name 展示。

```text
Plugin status:
  installed / disabled / source_missing / conflict

Runtime status:
  not_started / starting / ready / failed / exited
```

Disabled plugin 不进入 desired runtime catalog，因此没有 runtime status。列表从 plugin lifecycle 显示 disabled。plugin enabled 但 runtime failed 时，列表显示失败 message。enable/disable 操作返回时 desired catalog 和 MCP projection 已发布；新增或重新启用的 server 可以仍处于 `Starting`。

Runtime query 加载期间，UI 保持 loading state，不把缺失 snapshot 映射为 disabled。完整查询后 enabled plugin 仍没有对应 snapshot 表示 state inconsistency，UI 显示 runtime unavailable 并允许重新加载；不得伪造 `Ready`、`Failed` 或 `Disabled`。

## API 与 schema 影响

### Rust types

需要更新：

- `PluginInstallTarget::ManualMcpServer`
- internal `McpServerDiscoveryResult` and `McpServerRuntimeKey`
- `McpTransportDefinition::Stdio` explicit `inherit_env`
- `McpTransportDefinition::StreamableHttp` secret reference fields
- `McpServerStatus::Exited`
- `McpRuntimeListRequest`
- `McpRuntimeListResponse`

`McpRuntimeListRequest` 和 `McpRuntimeListResponse` 按 list API 规则使用 cursor pagination。Wire/API 类型必须导出 TS/schema；`McpServerDiscoveryResult` 和 `McpServerRuntimeKey` 只用于 runtime 组装边界，不导出到 wire schema。不要创建 UI 专用 `ManualMcpServer` 对象与 core MCP 对象重复表达 transport。

### Service boundary

`PluginManagerService::install()` 继续是 plugin install 的入口。手动 MCP 的 manifest materialization 放在 `talos-plugin` installer 内。

运行时 reconcile 不能塞进 `talos-plugin`。现有 app/service mutation handler 负责在不新增平行 manager 的前提下编排：

```text
validate candidate and global MCP namespace
  -> prepare next desired catalog
  -> commit plugin mutation
  -> synchronously publish desired runtime states
  -> atomically publish MCP tool projection
  -> start Added/Changed connections in background
  -> return committed plugin/install
```

Candidate validation、manifest materialization 或全局名称冲突必须在 plugin state commit 前失败。Desired state publish 是基于已校验 descriptor 的内存状态替换，不设计为可恢复的部分失败 API。

MCP initialize、list tools、HTTP connection 和 stdio process startup 都发生在 state commit 之后，是 server runtime outcome，不是 plugin mutation failure。它们通过 `Starting`、`Ready`、`Failed` 或 `Exited` snapshot 表达，不能把已提交的 install response 返回为 error，也不能回滚 plugin state。

Descriptor discovery 必须按 install 隔离错误。一个已安装 manifest 损坏时，该 install 被排除并产生 plugin warning，不能阻止其他 descriptors 进入 reconcile。

### Breaking change review

潜在 breaking changes：

- stdio launcher 不再默认继承 Talos 的完整 process environment。依赖隐式环境变量的已有 server 必须把变量加入 `inherit_env`。
- `McpTransportDefinition::StreamableHttp` 新增字段会改变 schema 和 TS 类型。
- `McpServerStatus` 新增 `Exited`，前端 exhaustive switch 必须更新。
- MCP runtime startup 改为后台执行，app-server ready 时 MCP server 可能仍为 `Starting`。
- ToolRegistry 新增原子 MCP replacement 语义，enable/disable 后 tool catalog 会在 desired state publish 时立即变化。
- `McpRuntimeListResponse` 使用 `data` 和 `next_cursor`，这是新增 API，不替换已有 wire response。
- 如果 manual server name 与 config server name 冲突，必须明确返回 conflict，不自动重命名。

兼容原则：

- 已有 marketplace MCP manifest 继续可读。
- 已有 config MCP servers 继续可用。
- 旧 stdio JSON 缺少 `inherit_env` 时按空列表反序列化，并在迁移说明中列出环境继承行为变化。
- 旧 `StreamableHttp { url, headers }` JSON 反序列化时新增字段使用 default。
- 不删除现有 plugin install/read/list API。

## 验证策略

实现计划中需要把测试拆到 unit、integration、E2E 和 frontend 层级。Spec 只要求覆盖以下行为：

- Manual MCP 通过 plugin install lifecycle 落盘、查询、启用、禁用和卸载，不写 config YAML。
- Manual MCP 输入校验覆盖 name、path safety、duplicate、relative `cwd`、HTTP auth references 和 explicit environment inheritance。
- Runtime reconcile 能增量处理 Added/Changed/Removed/Unchanged，并原子更新 MCP tool projection。
- 单个 broken MCP server 的 startup timeout、transport failure 或 process exit 不影响其他 ready server。
- stale generation 不能覆盖当前 runtime status 或 tool projection。
- stdio launcher 能限制 environment、drain stderr、清理 process tree，并正确表达 `Failed`/`Exited`。
- runtime snapshot list 支持分页，并通过 `install_id` 与 plugin row 稳定关联。

Extend existing `talos-e2e/scenarios/09_app_server/mcp_e2e.sh`:

- create a manual stdio MCP server using the test MCP binary.
- run a turn that discovers and calls the manual MCP tool.
- disable it and verify a new turn cannot discover that MCP tool.
- create one intentionally broken MCP server and verify the app-server remains usable.
- keep one healthy MCP server connected while another server is installed and fails startup.

Frontend tests:

- create form calls plugin install with `ManualMcpServer` target.
- list row displays plugin lifecycle state plus runtime status/failure message.
- runtime loading/error states never present an absent snapshot as disabled or ready.
- update mode keeps transport readonly and exposes enable/disable/uninstall.

## Open risks

1. ToolRegistry 当前使用全局 map。实现原子 MCP projection 可能需要调整内部存储，但不能改变 built-in/dynamic tool ownership，也不能把 runtime lifecycle 移入 registry。
2. Existing MCP servers 可能依赖当前隐式继承的 host environment。切换到 explicit `inherit_env` 是有意的安全 breaking change，需要迁移说明和启动错误上下文。
3. HTTP auth shape 当前并不统一。本设计确定 manual MCP 只写 env-based auth references；既有 fixed headers 仅兼容 config/marketplace。实现时必须集中 normalization，不能在各层复制 mapping。
4. Process tree termination 和 bootstrap environment allowlist 是 platform-specific。Unix 和 Windows 行为需要分别验证。
5. Background startup 意味着早期 turn 不会发现仍为 `Starting` 的 MCP tools。这是确定行为；产品风险是首个 turn 的能力集合可能少于后续 turn，Settings 必须准确展示状态。
6. Removed/Changed server 的旧 process 最长可以在后台保留 120 seconds 以 drain in-flight calls。高频 replace 可能短时增加资源占用，runtime 必须按 generation 跟踪并确保每个旧 process 最终被关闭。

## Deferred follow-ups

1. Executor-backed MCP stdio launcher.
2. Full MCP worker/sidecar process for isolating rmcp client runtime from Talos main process.
3. OAuth login and token storage for streamable HTTP MCP.
4. Tool-level approval policy and per-server tool filters.
5. MCP runtime metrics: startup duration, tool call duration, failures, restarts and process exits.
6. Explicit restart API and bounded per-server reconnect policy.
