# MCP 服务管理与可靠性设计

**状态**：本期能力已实现，本地回归完成；外部账号与 Windows 待验收
**创建日期**：2026-08-06
**更新日期**：2026-09-11

[互操作设计](interoperability.md)负责协议、导入和鉴权契约；[根目录实施计划](../../IMPLEMENTATION_PLAN.md)负责阶段、成功标准、测试和状态。

## 目标与范围

本期交付三个结果：

1. 用户在现有设置页面添加 stdio 或 Streamable HTTP MCP，保存为 Plugin manifest，并可启用、禁用、替换和卸载。
2. 单个 MCP 启动慢、挂起、崩溃或协议失败时，Talos 和其他健康 MCP 仍能正常工作，界面能显示真实状态。
3. 升级到已核实的 `rmcp 3.2.0`，适配 MCP `2026-07-28`，完成常见本地命令、HTTP Token/API Key 和本地 OAuth 接入。

本期面向单实例的本地、Desktop 和本地 Web。`mode: cloud` 目前只是部署模式配置，Cloud 多用户、租户、用户凭据隔离、按用户拆分连接和工具目录、托管 OAuth 回调均留到下个版本。本地连接 Notion、GitHub 等第三方托管 MCP 属于本期，与 Talos Cloud 无关。

手动 MCP 保存到 Plugin 托管安装目录中的 `talos-mcp.json`。现有静态配置继续作为另一种定义来源，由应用层统一组装。

## 当前代码事实与具体改动点

代码基线：2026-09-09，`main@ff08502632f9bfdcc37dcb7e5cfb991f7167c6d6`。实施前依赖为 `rmcp 1.7.0`，阶段 2 已升级为 `3.2.0`。下表记录实施前问题与目标改动，完成状态以根目录计划为准。

实施前调用链：

```text
application_runtime::build_application_runtime_with_migration_dir
  -> register_mcp_tools
  -> TalosMcpRuntime::connect_all          [talos-mcp：连接与协议执行]
  -> McpConnectionManager::from_runtime   [talos-tools：工具适配及当前快照缓存]
  -> ToolRegistry::register_mcp_server
  -> McpToolHandler                      [调用返回同一个 Manager -> Runtime]
```

对象定义与调用可见 [连接实现](../../talos-rs/crates/action/mcp/src/lib.rs)、[工具适配器](../../talos-rs/crates/action/tools/src/mcp/mod.rs) 和 [应用注册入口](../../talos-rs/crates/app/talos-base/src/lib.rs)。本文的连接管理职责由当前 `TalosMcpRuntime` 承担；`McpConnectionManager` 当前承担工具适配。

| 现有位置 | 当前问题 | 最小改动 |
| --- | --- | --- |
| `talos-rs/crates/action/mcp/src/lib.rs` | `refresh()` 串行连接并替换、关闭全部服务 | 原运行时按定义差异协调，有界后台启动，只处理新增、变更和删除 |
| 同上 | 服务表读锁跨网络等待；失败只保留消息 | 捕获服务句柄后释放锁；内部保留强类型错误，对外投影 `ApiError` |
| 同上 | stderr 未持续读取，缺少完整进程树清理 | 复用 SDK 进程能力，补齐排空、退出观察和有界清理 |
| 同上 | 启动期间预取资源；列表忽略后续页；部分操作无期限 | 就绪只依赖必要协议流程与工具发现；资源按需分页，操作均有期限 |
| 同上 | HTTP 构造忽略 headers，协议版本固定 | 使用新版 SDK 协商与传输构造，真实应用 headers 和鉴权 |
| `talos-rs/crates/action/tools/src/mcp/mod.rs` | `McpConnectionManager` 永久缓存快照和工具定义 | 保留工具适配职责，删除永久副本；连接与工具目录事实归运行时，注册表按需派生工具定义 |
| 同上及 `handlers/mcp.rs` | 资源接口忽略 cursor；丢弃结构化结果；图片退化成 JSON/base64 | 转发有界分页，保留结构化结果，复用已有强类型内容与产物链路 |
| 同上 | 全部 MCP 工具默认只读、非破坏性、可并行；有占位死代码 | 使用现有保守权限规则，annotations 仅作提示；删除占位与被替代路径 |
| `talos-rs/crates/action/tools/src/registry.rs` | 只保存永久注册结果，定义与 handler 分开查询 | MCP 目录按需派生，dispatch 捕获同代定义与连接绑定，保持内置和动态工具职责 |
| `talos-rs/crates/app/talos-base/src/lib.rs` | 空目录时不创建运行时，启动等待全部连接，静态 HTTP 鉴权转换缺失 | 启用 MCP 时始终创建运行时；应用组装统一定义并发布；补齐鉴权转换 |
| `talos-rs/crates/app/talos-base/src/{application_runtime,application_automation,lib}.rs` | 应用组装仅把连接句柄交给工具 handler，应用关闭未显式调用 MCP shutdown | 应用持有连接管理句柄，接入正常关闭与启动部分失败清理 |
| Plugin 安装器、manifest 扫描及 `talos-rs/crates/orchestration/service/src/plugin/service.rs` | 安装目标仅支持已有插件；Service 同步委托 manager，无运行连接依赖；坏 manifest 可阻断全部发现 | 扩展安装目标；应用注入协调依赖，改造受影响的修改链路；按安装隔离发现错误 |
| `SettingsMcpServersSection`、`McpServerForm` 及共享 SDK | 保存尚不可用，只有安装状态 | 接通现有表单及 JSON 输入，展示运行和鉴权状态，复用错误展示器 |

实施前图片能力只覆盖用户附件；`ArtifactRef` 尚无文件解析和图片展示链路。工具图片需复用 FileService 存储、AttachmentReader 和 Provider 图片编码，补齐工具结果引用、模型输入及 UI 授权读取；不能将 JSON/base64 或标签渲染视为图片支持。

连接生命周期、本地进程处理和协议结果转换按职责拆分到现有 crate 内。`McpConnectionManager` 保留工具适配职责，连接管理提供唯一目录事实，注册表按需适配。

序列化或协议转换中的 `unwrap_or`、`.ok()` 等静默丢失行为，在 MCP 结果链路内改为有来源的显式失败。

## 职责与核心对象

| 边界 | 唯一职责 | 禁止承担的职责 |
| --- | --- | --- |
| Plugin 生命周期 | 定义、安装状态、启用、替换、卸载及持久化事务 | 持有进程、SDK 客户端或 OAuth 会话 |
| 现有应用/Service 组装层 | 合并静态与插件定义、校验冲突、解析服务凭据、触发协调 | 实现 MCP 协议或另建管理体系 |
| `TalosMcpRuntime` | 当前连接、运行状态、工具发现、本地进程、调用和资源访问 | Plugin 安装和 UI 展示 |
| 现有 MCP 工具适配模块 | 将协议结果和工具定义接入现有工具系统 | 再缓存一套永久连接状态或管理进程 |
| `ToolRegistry` | 当前模型可见的工具定义和处理器 | 连接、重连、关闭 |
| 设置页面和 SDK | 输入及真实状态展示 | 直接改文件、推测连接状态、重做后端校验 |

继续复用 `McpServerDefinition`、传输定义和内部运行时描述符；对外查询使用安全摘要。输入格式在入口转换为同一套规范对象，SDK 类型保持在 `talos-mcp` 内。

当前 `McpServerSource::Config` 没有独立 ID，静态服务使用配置中的唯一服务名称作为身份；Plugin 服务使用安装 ID。服务名称仍用于工具命名空间，实例内必须唯一。内部身份使用强类型表达。变更检测比较规范化后的服务定义与 Plugin 内容摘要，脚本更新也触发连接替换。

配置、来源、安装方式、鉴权与连接状态分别建模，并由上表对应的职责持有。

## 目标架构与调用流程

应用持有同一连接管理句柄，Service 负责定义变更与发布，工具适配器负责调用。图中的箭头表示调用或数据流；注册表通过工具适配器读取连接层的当前目录，保持 `talos-mcp` 与 `talos-tools` 的依赖边界。

```text
设置页面 / SDK -> Gateway -> Service 协调
                              |-- PluginManager -> talos-mcp.json
                              |-- 服务凭据 / OAuth
                              `-- 连接管理（TalosMcpRuntime）
                                    `-- rmcp -> stdio 子进程 / HTTP MCP

连接管理当前视图 -> 工具适配器按需派生 -> ToolRegistry
                                        ^
                                        |
Agent Kernel -> 当前工具定义与权限检查 --+
             -> McpToolHandler -> 工具适配器 -> 同一连接管理 -> rmcp
```

添加服务的完成点分为两步：Service 校验并提交安装、发布期望状态后返回安装结果；后台连接完成协议协商与必要工具发现后，另行发布 `Ready` 或失败状态以及当前工具目录。调用方通过状态查询观察连接结果。

模型调用从当前注册表解析工具并执行权限检查。连接层捕获服务身份、代次和句柄后释放共享锁，再发起外部请求。结果经工具适配、内容处理和裁剪返回模型；资源列表和读取沿同一连接按需执行。

替换或删除服务时先撤下旧工具并拒绝新调用，在途调用按既定期限收尾。应用关闭时停止接入、处理在途操作并清理连接与子进程。鉴权断开按凭据失效规则取消关联调用。

## 手动 MCP 管理

### 安装与更新

扩展现有 `PluginInstallTarget`，增加携带 `McpServerDefinition` 的 `ManualMcpServer` 目标。继续使用现有 `PluginInstallRequest`、`PluginInstallResponse` 及插件列表、读取、启用、禁用、卸载接口。

手动安装的 wire 目标为 `{ "kind": "manual_mcp_server", "server": McpServerDefinition }`，沿用既有 `enable` 与 `conflict_policy`。请求总定义不超过 256 KiB，未知行为字段拒绝；可附带只写凭据整包，输入和引用契约见下文阶段 4 接口表。托管手动安装不伪造 MarketplaceSource；相同服务名的替换保持安装身份。

安装器生成 `PluginKind::McpServer`、`PluginInstallSource::Manual` 的托管安装，在现有目录保存 `talos-mcp.json`，复用市场 MCP 的 manifest 读取路径。定义持久化与安装状态写入复用已有锁、临时文件、原子替换和恢复机制。

服务名称由共用校验器检查：1–256 个 ASCII 字母、数字、下划线或连字符，保留大小写，不静默改名；连续双下划线是工具路由分隔符，服务名中明确拒绝。安装路径由安装器从已校验身份派生并检查不越过安装根目录，不能直接拼接未经校验的输入。手动输入的非空 `cwd` 必须为绝对路径；市场 manifest 的相对路径继续按安装目录解析。

同名手动安装遵循既有 Reject/Replace 语义。修改传输定义走已有替换安装流程；前端应提供重新提交替换的可达路径。市场安装继续遵循原有更新方式。

现有 Service 层在提交前检查候选定义是否与静态配置或其他安装冲突。Plugin 管理器不反向读取应用配置。安装成功表示定义已经保存，不保证依赖已安装或连接已就绪。

### 发现与故障隔离

已安装 manifest 按安装逐项解析。一个损坏安装产生结构化插件警告并排除该项，不能阻断其他安装。优先复用已有发现结果和 `PluginWarning`；只有现有返回类型无法承载部分成功时才增加内部结果对象，不公开新的发现 API。

静态配置仍在配置提交前整体验证。非法修改不替换当前生效定义，也不触发协调。两者不能通过吞错或伪造运行时快照实现。

### 凭据与启动环境

凭据规则见[互操作设计](interoperability.md)。新 Token/API Key 输入只写，manifest 保存引用；敏感数据不进入快照、日志、导出或模型上下文。

stdio 保留 command、参数数组、env 和 cwd。复用 `env_clear()`、现有 `filter_inherited_tool_environment()` 及显式变量注入。新增秘密引用必须显式解析并只注入对应服务，引用缺失明确失败。命令不通过 shell 字符串拼接，记录执行主机及启动配置来源。

`npx`、`uvx`、Docker 和直接可执行文件都按 command/args 在后端主机启动，所需可执行文件由部署环境提供。现有环境过滤不等于资源沙箱，系统环境中仍可能存在无法通过名称识别的敏感值。

## 连接管理与执行行为

### 后台启动与状态

启用 MCP 功能时，即使初始服务目录为空也创建运行时。应用发布期望服务目录后即可对外可用；最多同时启动四个连接，其余任务可在创建进程前取消。连接管理内部控制并发和任务取消。

需要授权但没有可用凭据的服务保持 `NotStarted`。其余服务进入 `Starting`，在总启动期限内完成 SDK 协商/发现，并在服务声明工具能力时完成必要的工具列表读取后进入 `Ready`，失败则进入 `Failed`。未声明工具能力的服务可以带空工具目录就绪，不能因此拒绝仅提供资源的 MCP。资源、模板不在启动路径上预取，不作为工具就绪条件。

现有状态枚举已包含 `NotStarted`、`Starting`、`Ready`、`Failed` 和 `Disabled`，连接实现当前只生成 `Ready`/`Failed`。目标补齐未启动和启动中行为，将失败消息改为结构化错误，并增加 `Exited`。内部失败保存 `TalosError` 与强类型来源，对外快照仅投影 `ApiError`，遵循[统一错误模型](../../docs/architecture/ERROR_MODEL.md)。

`Exited` 仅表示就绪后本地进程非主动关闭的退出；启动期间退出属于 `Failed`。HTTP 无本地进程，不产生 `Exited`。主动清理不覆盖原始失败状态。已禁用安装通过 Plugin 状态展示，不另外维护一套运行时禁用状态机；移除未用于连接查询的 `Disabled` 值，查询以安装状态为准。

### 增量协调与目录一致性

应用组装层创建并持有连接管理句柄，并把定义组装、连接协调所需依赖接入现有 `PluginManagerService` 修改链路。该 Service 当前只有同步委托，需要同步调整安装、更新、启用、禁用和卸载的调用方，使一次修改在持久化与期望状态发布后完成。底层 `PluginManager` 继续只负责插件状态。

安装、替换、启用、禁用、卸载和鉴权变化都触发协调。静态配置沿用 `ConfigReloadHook` / `ConfigReloadPlan` 的准备与发布边界，在应用组装的 reload hook 中加入 MCP 定义的校验及发布。当前 hook 未更新 MCP 连接；实施后显式配置重载才会协调生效，配置文件写入本身不等于已经生效。

| 差异 | 行为 |
| --- | --- |
| 未变化 | 复用现有连接和工具目录，不重连 |
| 新增 | 按鉴权状态发布 `Starting` 或 `NotStarted`，后台启动 |
| 变更 | 立即撤下旧工具，按新定义启动；只处理该服务 |
| 删除 | 拒绝新调用并撤下工具，后台清理旧连接 |

在现有 Service 修改路径中加入共享的协调临界区，使所有期望目录修改有序完成校验、准备、持久化提交和内存发布；不在其中等待外部 MCP。提交前完成可失败的定义校验。提交后协议启动失败只改变运行状态，不能把已经成功的安装伪装成失败或回滚安装。安装记录写入错误时，在文件锁内重读确认原记录、候选记录或无法确认三种结果；只有确认原记录仍生效才回滚目录。候选记录已生效时立即发布，保留备份并明确返回耐久性未确认错误；无法确认时保留目录和备份并报错。提交后的备份清理错误单独报告。

运行时原子更新当前连接视图；注册表在一次短锁快照后按需派生 MCP 工具目录，不保留另一套永久投影，也不新增发布任务。dispatch 从同一视图捕获定义和连接绑定，权限判断及执行沿用该绑定。替换期间旧处理器不能按名称重新查找并误调用新实例；内置和动态工具保持既有路径。

`KernelAgentProvider` 已共享同一个 `Arc<ToolRegistry>`，Kernel 在每次模型迭代重新读取工具定义；保留这一链路。`ToolContext` 每次调用构建的操作快照也保留。需要删除的永久副本仅位于 `McpConnectionManager`。

每个后台启动、调用、退出或清理任务只能影响捕获的服务代次，旧任务不能回写新状态。插件内容摘要参与连接复用判断，因此 manifest 不变而脚本内容改变时也会替换连接。服务表锁只用于捕获句柄和代次，网络等待在锁外执行。注册表按需派生当前目录，适配器不再保存永久快照。

变更或删除时，已捕获旧句柄的普通在途调用允许在有界期限内结束，新调用立即拒绝。鉴权断开需阻止新请求并取消关联在途请求，不能继续使用已移除凭据；其他服务不受影响。SDK 成功刷新 token 不改变工具身份，也不无故重置连接。

工具目录变化使用协商后的 SDK 通知/订阅，只刷新关联服务。沿用现有工具可用性检查和增量上下文链路，不重写模型历史。

### 本地进程与关闭

应用组装把同一连接管理句柄交给工具适配器、Service 修改路径和应用生命周期持有者。`TalosApplicationRuntime::shutdown` 的嵌入式关闭路径、`TalosServerRuntime` 对应的服务端关闭路径均显式停止 MCP：先停止接收新调用，再等待或取消在途操作，最后有界清理连接与进程。任一其他组件关闭失败时，仍执行 MCP 清理并分别报告错误。

应用将首次连接发布放在组装末尾，所有可失败步骤完成后才启动外部 MCP，避免初始化失败时产生已启动连接。连接管理的存活期由应用负责，即使工具目录为空也保持同样的所有权。

stdio 已经运行在独立子进程中。先核对升级后 SDK 的子进程句柄、退出通知和清理能力，能满足目标就直接复用。SDK 缺少的进程树控制由 `talos-mcp` 内的私有本地实现补齐。

进程处理必须满足：

- stderr 持续排空，读取缓冲有界；第三方原始文本默认不转发到普通日志、UI 或模型，退出及 I/O 元数据使用结构化错误边界。
- 观察异常退出，区分主动清理与自然退出。
- 启动失败、取消、禁用、卸载和主程序正常关闭均有进程树清理路径；初始化部分失败也不能遗留已启动进程。
- 有明确的关闭期限和平台行为。Unix 进程组、Windows 对应进程树机制分别验证；清理次级失败单独报告。

不承诺隔离 SDK 客户端自身崩溃、CPU/内存耗尽、文件系统或网络权限。本期可靠性保证覆盖外部 MCP 进程退出、协议失败和有界操作。

### 期限与错误范围

沿用默认启动 30 秒、操作 120 秒，旧连接收尾最多 120 秒。期限集中在连接管理内部，测试可显式覆盖。启动从出队开始计时，覆盖进程创建、协议发现和全部必要工具分页。一次工具目录刷新共享总操作期限；资源单页读取和内容读取分别受操作期限限制。

请求取消的通知发送和本地收尾也必须有界，不能在原请求超时后无限等待取消。禁止自动重试有副作用的工具调用，连接失败后由用户显式恢复。服务失败可通过禁用/启用恢复。

| 结果 | 影响 |
| --- | --- |
| 参数错误、远程拒绝、`is_error`、不支持的可选能力 | 单次操作失败，保留可用连接 |
| 调用取消或超时 | 按 SDK 语义取消，不能仅凭超时认定整台服务失效 |
| 终止性传输失败 | 移除该代次工具，发布 `Failed` 并清理 |
| 本地就绪进程退出 | 发布 `Exited` 并移除该代次工具 |
| 鉴权失效或断开 | 阻止该服务未授权调用，展示鉴权状态 |

`is_error` 属于服务返回的工具结果：保留完整强类型内容、结构化数据与错误标志交给工具适配器，不能降成只有 `ApiError` 的调用失败。错误分类依赖类型与错误码，不匹配远程消息文本，不用空目录或空成功结果掩盖失败。

## API 与前端改动

沿用现有设置页面。表单与 JSON 输入共用后端安装校验和规范对象；替换、启用、禁用、卸载复用插件 API。字段和不支持的配置必须有明确错误。

### 对外接口契约

| 接口或对象 | 变更性质 | 目标契约 |
| --- | --- | --- |
| `plugin/install` / `PluginInstallTarget` | 扩展现有接口 | 增加 `ManualMcpServer { server: McpServerDefinition, credentials?: McpCredentialValues }`；响应继续使用 `PluginInstallResponse` |
| Plugin 替换、启用、禁用、卸载 | 调整现有行为 | 提交安装状态并发布期望连接状态后返回；后台连接结果由状态查询表达 |
| 连接状态查询 | 新增只读接口 | 请求使用 cursor/limit，响应使用 data/nextCursor；返回服务身份、来源、状态与安全错误 |
| 服务凭据设置与清除 | 新增管理能力 | 秘密只写，查询返回存在性和安全状态；绑定到明确服务身份 |
| OAuth 启动、状态、断开 | 新增管理能力 | 授权尝试可查询且有期限；断开使旧授权任务及凭据失效 |
| `McpServerStatus` | 扩展现有对象和行为 | 补齐未启动/启动中行为，失败携带 `ApiError`，增加 `Exited` |
| 资源列表 | 调整现有输出契约 | 保留已有查询入口，返回强类型有界页面及继续查询游标 |
| 工具调用结果 | 修复端到端映射 | 保留结构化数据、完整 `is_error` 结果及已支持内容类型 |

具体 wire 方法和输入输出如下，规范 Request/Response 跨 HTTP、JSON-RPC、SDK 和前端复用。

连接状态采用 JSON-RPC `mcp/server/list` 与 HTTP `GET /v1/mcp/servers`。两端使用同一 `McpServerListRequest/Response`：`cursor` 可省略或为 null，从头开始；`limit` 可省略或为 null，默认 50，允许 1–200。响应为 `data`、`nextCursor`；按服务名排序，游标表示上一页最后的服务名，服务删除后仍继续读取后续名称。非法游标返回 `InvalidCursor`，非法 limit 返回 `InvalidRequest`。

`McpServerInfo` 仅返回 `name`、`displayName`、`source`、`transport`、`status`、`toolCount`。来源为静态配置或 Plugin/安装身份，运行状态携带安全 `ApiError`。不返回命令、环境、URL、头部、安装路径或内容摘要；不再沿用包含完整工具目录和启动定义的旧快照对象。安装启用与损坏 manifest 仍从 Plugin 接口获取，连接查询不复制安装状态。该只读接口沿用现有会话鉴权和单实例管理权限。

MCP 管理接口采用 camelCase；既有 Plugin 安装 payload 沿用其 manifest 配置字段的 snake_case。`ManualMcpServer` 可附带只写 `credentials: McpCredentialValues`，省略表示未提供新值；服务定义中的 `credentials` 是 `{ reference, authentication }` 绑定，不含秘密。Token 设置支持 `none`、`bearer`、`header { name }`；凭据值按服务整包替换，含 `token` 和可选秘密 `env/headers`，省略的映射表示无秘密项。

| 方法 | HTTP | 输入和响应 |
| --- | --- | --- |
| `mcp/server/import` | `POST /v1/mcp/servers/import` | `McpServerImportRequest { json, name?, enable?, conflictPolicy }` → `PluginInstallResponse`；bare server 无名时必须提供 name |
| `mcp/server/auth/read` | `POST /v1/mcp/servers/auth/read` | `McpAuthenticationReadRequest { server }` → `McpAuthenticationResponse` |
| `mcp/server/credentials/update` | `PUT /v1/mcp/servers/credentials` | `{ server, change: replace { credentials } / clear }` → `McpAuthenticationResponse` |

服务引用为 `plugin { installId }` 或 `config { name }`。状态区分未配置、凭据已保存、引用缺失和安全失败；“已保存”不声称供应方已接受 token，连接结果另看运行状态。`clear` 先停止该服务的关联调用，再移除本地秘密并保留缺失引用，防止重启后自行恢复。私有记录按一个 UUID 引用存储整包，绑定安装身份及完整传输目标；更新端点、命令或其他影响目标的配置须重新绑定。显式清除后，替换定义时移除已缺失的旧引用，允许用户改完目标再授权。恢复同时核对禁用安装与当前生效/持久配置，未知提交结果保留秘密。

OAuth 复用认证读取与凭据清除，通过 `mcp/server/auth/start`（HTTP `POST /v1/mcp/servers/auth/start`）。请求为 `{ server, config: McpOAuthConfig, clientSecret? }`，秘密只写；配置包含可选预注册 `clientId`、`clientMetadataUrl`、`callbackPort` 和 `scopes`。不配置客户端时按 SDK 支持的注册能力处理。响应仅向发起客户端返回 `{ server, attemptId, authorizationUrl, expiresAt }`；状态读取不重复返回登录 URL。`expiresAt` 使用 Unix 秒时间戳。

认证尝试有 10 分钟期限，每个定义至多一个有效尝试；新的开始、凭据替换/清除、目标改变、卸载和关闭使旧尝试失效。回调监听仅绑定 `127.0.0.1`，路径固定 `/mcp/oauth/callback`；预注册客户端需匹配该回调地址和指定端口。SDK 负责 PKCE、state、issuer/resource、token 交换与刷新。回调期间的凭据留在 SDK 内存存储中，完成后在现有提交锁内重新核对定义和尝试身份，再持久化新引用。OAuth 认证状态增加授权中、已认证和已过期。

SDK 刷新使用其 CredentialStore 的刷新 guard；私有文件写回在现有凭据文件锁下核对记录仍存在且归属匹配，禁止刷新重新创建已清除文件。普通刷新不改变服务引用或重建连接。不新增独立刷新调度器。OAuth 设置与 Token 设置共用现有定义发布、只写私有存储和断开路径。

rmcp 的存储错误接口只接收字符串；私有适配器通过连接内的一次性通道把原始 `TalosError` 交给现有连接任务。首次存储失败终止该 OAuth 连接并统一记录来源链、捕获位置和服务名，其他 MCP 连接继续工作；修复存储后可禁用再启用恢复。连接取消只阻止旧状态发布，不丢弃已发生的诊断；退出先关闭接收端再读取剩余错误，避免取消竞争吞错。不引入全局错误缓存或独立错误管理器。

取消后的记录只保留到旧异步操作完成。重授权等待该定义所有未完成尝试释放监听；应用关闭也等待仍处于发现阶段的操作，不能只等回调任务。最多保留 32 个未完成操作或可淘汰的状态摘要，已结束的失败不阻塞新授权。

私有 OAuth 记录补充保存 SDK 未持久化的客户端密钥、回调地址及实际授权 resource。连接恢复时复核 issuer/resource；rmcp 3.2 未公开 resource getter，使用 SDK 生成授权 URL 提取该参数作比较，产生的单个未使用 PKCE state 随连接释放，不复制发现协议。SDK 自身 auth 日志可能含授权码及供应方响应，因此既有日志输出层对该 target 应用不可覆盖的静态过滤，错误由 Talos 边界脱敏投影。自行设置 tracing subscriber 的嵌入式宿主也必须保持这一秘密日志边界。


### 内部接口契约

- 连接管理的创建与连接启动分离：创建得到应用持有的空连接集合；提交期望定义触发后台增量协调，返回不等待外部服务就绪。
- 现有 `connect_all/refresh` 的全量连接路径由增量协调路径替代；定义未变化的连接保持复用。
- 连接层提供同步短锁读取的当前视图；注册表按需派生目录。dispatch 同时绑定工具定义与对应连接，替换或删除后的新准入由连接管理拒绝。
- 工具适配器读取当前定义并转发调用、资源请求，不持有永久快照；关闭接口由应用生命周期显式调用。

### 客户端接入


连接摘要列表使用 cursor/limit 和 data/nextCursor，客户端原样续传游标。Plugin 行按安装 ID 关联摘要，静态服务保留配置来源，不伪造安装记录。摘要不携带解析后的秘密值。

界面分别展示安装/启用、运行、鉴权状态。加载中、尚需授权、损坏 manifest 和运行失败必须有真实依据；缺失快照不能直接解释为就绪或禁用。后台启动意味着首次 turn 可能只看到已经就绪的服务，界面明确显示启动中。

影响公共对象时同步 HTTP/JSON-RPC 路由和 Desktop IPC 参数校验，由生成命令更新当前主版本 schema、Rust/TS SDK、示例和调用方；不手写生成产物，不给旧协议增加新入口。

macOS 可能对只含未回收退出进程的进程组返回 EPERM。清理只在确认根进程已经退出并回收后重试组级信号；重试仍失败则保留错误，不把权限错误等同于组已消失。

## 兼容影响与验收约束

- 保留现有名称大小写规则、静态配置来源、环境过滤默认行为及 Plugin 生命周期。
- 新凭据字段与状态对象变化需要 schema 和调用方同步。旧静态 bearer 配置经显式适配实际生效，不继续忽略；不将旧秘密复制到新 manifest 或公开输出。
- 现有凭据输入按实际字段进行必要转换。新导入的秘密必须安全落盘，旧定义的读取与展示也必须继续脱敏。
- 后台启动及目录即时更新属于行为变化，必须验证早期 turn、在途调用、启用/禁用、错误事件和会话回放。
- 资源列表从旧裸数组改为带游标的强类型页面属于工具输出契约变化，必须同步现有工具说明、调用方和回放测试，不能只修改后端返回值。
- 升级依赖同时更新工作区与锁文件，并检查所有直接使用 `rmcp` 的模块和测试服务。
- 验证复用现有 MCP 测试二进制、工具集成 fixture、前端测试和 `talos-e2e/scenarios/09_app_server/mcp_e2e.sh`。测试覆盖可观察行为。
- 主流服务兼容性以[互操作验收矩阵](interoperability.md#验收证据)为准。文档完成、依赖升级、编译成功均不等于实际接入完成。

实施阶段、依赖与检查命令见[根目录 PLAN](../../IMPLEMENTATION_PLAN.md)。
