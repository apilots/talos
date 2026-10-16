# Computer注册与工作区

Status: implemented — 阶段 1–5 及本轮 Review 修复/当前源码真实 Electron/Web 验收完成；原阶段 DMG 安装验收单独保留。结果与未验证部署范围见 [验收记录](verification.md)。
更新边界：管理宿主与统一Worker RPC目标由[联合feat-044](../feat-044-talos-sync/design.md)修订，实现及实际验收见044根PLAN；043历史验证只证明原版本，不证明新目标已实现。
基础：[feat-042](../feat-042-distributed-agent-execution/design.md)。协议：[接口与 Protobuf](protocol.md)。执行：[注册](plans/01-registration.md)、[执行](plans/02-outbound-execution.md)、[环境](plans/03-workspace-environment.md)、[UI](plans/04-computer-ui.md)、[验收](plans/05-e2e-delivery.md)。

## 1. 最终目标

Desktop和公网talos-server都有管理职责、自己的业务DB和一个内嵌本机Worker；账号共享注册默认使用`https://agent.talos.love`，地址可配置。
同账号经独立Sync共享资源；当前管理节点选择Computer并协调执行，结果实时回当前协调者。Web使用其连接管理节点的产品API。
Desktop 内嵌 Worker 也是其中一台 Computer：首次默认本机，随时可切远端；目录和算力始终在被选中的节点。
这个地址是拟定产品默认值，不表示公网部署已完成。

本期包含主动连接、Desktop 内嵌 Worker 云注册、统一业务 API、Computer UI 和 Worker 环境管理。
保留不登录/关闭Sync的独立本机使用；共享历史及跨控制面执行由044定义，已有Session固定Computer，不做运行Turn接管或原生移动客户端。
复用 agent_nodes、AgentNodeService、Registry、AgentHarness 和 Client；不新增 Computer 表、授权表或 RPC 执行账本。

## 2. 管理宿主与薄客户端

| 方式 | 产品 API / 业务持久化 | Worker 与节点身份 |
| --- | --- | --- |
| Desktop app-server | 完整Service/Store/Coordinator与自己的业务DB；云登录启注册/共享，不裁剪本地管理能力 | 一个内嵌Worker，独立loopback RPC listener；本机经agent-client SDK调用，共享时另启Connect |
| 公网 talos-server | 同一管理runtime；协调本机/远端Computer，拥有当前协调任务的业务事实 | 一个内嵌Worker，同SDK/adapter/对象；管理API与Worker listener分离 |
| Web/移动端薄客户端 | 所连接管理节点的原API，不建立管理业务DB | 不因此启动Worker或协调任务 |
| 纯 talos-worker | 无Service/Coordinator/产品DB | 同Worker host/adapter，仅执行/环境；按配置listener与Connect |

登录或断网不改变Desktop管理runtime；无agent/sync也可loopback执行和恢复本机历史。共享/匿名历史认领沿044，不隐式改账号/Computer。
退出仍遵守原drain/取消；云租约与本机入口分离，缓存/草稿按账号和Computer隔离。

```text
Desktop / talos-server: Service -> Coordinator -> AgentHarness / agent-client SDK
                                                     |
                               loopback / direct / Node Connect RPC
                                                     |
                         同一个 Worker adapter -> Workspace / Provider
                                                     |
                         progress / settlement -> 当前协调者DB / UI
各管理节点已提交事实 -> talos-sync-sdk / WSS + Loro -> Sync -> 其他管理节点
```

本机由当前本机Coordinator经同一RPC SDK分派，文件I/O由本机Worker完成；远端使用同接口。UI走原Service准入/鉴权/持久化，不直接调用Harness或启动第二Worker。

## 3. 职责与代码位置

| Owner / existing location | Responsibility (normative) |
| --- | --- |
| agent-pb | Own only .proto sources, generated service APIs and PB objects. Add Connect transport envelopes; reuse existing Harness/business messages. |
| agent-protocol | Own AgentHarness, typed node contracts, conversions and the sessionless WorkerEnvironment port. No network lifecycle or Stores. |
| agent-client | AgentNodeClient/AgentNodeRunner own outbound registration, lease and connection lifecycle; RemoteAgentClient hides direct RPC or reverse-stream transport behind AgentHarness. |
| talos-worker lib / binary | Compose one execution/environment runtime and RPC adapter. Resource callbacks use the current invocation's client; loopback/remote transport is hidden. No product DB ownership. |
| talos-base / Talos Server | Host node ingress, the existing publisher and SDK-facing APIs; compose local or remote handles into the existing Registry. |
| Service / Stores | Authorize targets; own Project/Session/Turn/Timeline and uploaded attachments; perform business admission and configuration policy. |
| Desktop main / shared UI | Own local process lifecycle, enrollment and safe identity exposure; UI uses product SDK, selected Computer and existing account menu. |

Desktop 继续启动已打包的 app-server，由它组装现有 TalosWorker lib，不额外启动或打包独立 worker 二进制。
联网模式在这个内嵌 Worker 上安装云资源客户端、注册 Runner 和主动连接；app-server 负责宿主生命周期与既有登录引导，
产品业务经本机Service/Stores/Coordinator管理，云登录不切成无DB宿主。内嵌和远端Worker调用内聚在同agent-client SDK，同一宿主只有一个Worker。
独立 talos-worker 二进制保留给独立部署，复用同一 lib/Runner/adapter；它不是 Desktop 的额外执行进程。

### Desktop 宿主入口（阶段 1.3）

Electron main 给现有 app-server 传入宿主设备名，仍只启动一个已打包的进程。
Desktop/TUI 登录后使用统一个人 Worker 注册流程；不再提供连接模式或部署模式配置。普通 stdio/Server 保留完整管理组装，direct/outbound 与 endpoint 继续只决定传输。
cloud 宿主复用 AccountService/CloudSessionCache，登录完成和重启恢复后调用受信任的 `desktop/workerEnroll`；
app-server保存身份，以同一Worker+Runner注册，同时保留本地Project/Session/Timeline数据库。共享启停不换DB或销毁管理宿主；导入走原Store事务，不迁移Computer。

配置沿用 `TALOS_HOME/conf/talos.yaml`，显式外部 config 路径不能用于 Desktop 自动注册。
`cloud.agent_trusted_ca_file` 是首次创建节点时可选的私有 CA，随后写入 runner.trusted_ca_file；缺省使用系统根。
创建前将恢复标记原子写入 `$TALOS_HOME/.worker-state.json`，通过 `.worker-state.lock` 和现有配置锁串行更新。明确拒绝的创建可清除标记；未知结果保留标记，要求选已有 UUID 恢复。身份和凭据落盘完成后删除两个内部文件；已有身份重启自动注册，成功注册也会清理中断遗留的文件。旧配置字段由配置升级直接移除，不迁移其值。
保存顺序为内部 pending 标记 -> 配置中的 UUID/地址 -> conf 下原子写凭据（Unix 0600、文件/目录 fsync）-> 删除内部状态文件。
每次保存都有配置锁并保留其他 YAML 节点；并发创建检查原身份，避免重复创建。
注册凭据首次有效期为 365 天，过期由当前 owner 显式恢复；租约仍使用原有 90 秒/心跳/续期规则。
重启先读取已保存 UUID，向控制面确认归属，不覆盖用户改名；账户/控制面不匹配明确拒绝。

登录成功与 Worker 注册失败独立：保留登录结果和缓存，注册错误通过现有 Desktop runtime-state 的 ApiError 返回。
退出先让 stdio EOF 触发宿主 drain/注销，再退出进程；注销失败仍由既有租约过期规则收敛。
首次 Register 已发出时在既有 RPC 超时范围内等响应，退出再注销；不丢弃已签发租约，不新增持久化状态。
Register 建立身份与租约；direct 使用可达 TLS RPC 发布执行能力，outbound 则在同一 RegisteredWorker 的主动 Connect 确认后安装远程 Harness 并发布执行能力。

## 4. 注册、身份与可用性

1. 登录用户通过现有 CreateNode 流程创建个人节点，控制面生成 UUID 与一次性注册凭据。
   默认设备名称取宿主名称，允许改名；不以 hostname/地址匹配或重建节点。
2. Desktop 宿主或部署流程先保存 UUID，再启动 Worker：沿用 `TALOS_HOME/conf/talos.yaml` 的
   `agent_node.runner.node_id` 和控制面地址；凭据保存在现有受保护 credential_file，原子写入，不进入 Workspace/renderer。
   保存失败不执行；创建结果丢失时通过管理入口确认 UUID 并轮换凭据，不按设备名称猜测后重复创建。
3. Worker 读取保存身份，使用 TLS 主动调用 Register；控制面签发现有 90 秒租约。
   沿用 10 秒心跳、30 秒续租与单调时钟有效期判断；每个 RPC Request 使用独立 NodeAuth。
4. Worker 主动打开 Connect 双向流，控制面验证当前租约并安装客户端句柄后才置为 available。
   本机 Desktop 云注册也遵守同一网络租约；它在云端是 remote，绝不能把所有设备注册成 embedded。
5. 重启、改名与重连仍携带原 UUID；同节点新有效连接替换旧连接，旧连接不能撤销或回写新连接的状态。
   控制面重启后节点记录保留，Worker 重新注册/连接；旧 DB 心跳不是当前可执行连接的证据。
6. 停用/凭据轮换撤销旧租约、句柄和流；退出登录断开当前账户的 Worker 注册并按既有退出语义释放任务。
   已失效注册任务的鉴权/过期错误保留日志；确认执行清理成功后不阻塞重新注册或退出。执行清理、任务 join 或其他注册错误仍明确失败。
   owner 不可改绑；账户与保存身份不一致时拒绝复用旧凭据，通过连接流程建立正确归属。

Worker 只需出站访问控制面，不要求个人电脑暴露公网监听端口、固定地址或自行准备服务端证书。
Runner 默认使用系统 TLS 信任；私有部署可配置 CA。保留现有 direct RPC 部署的显式配置，默认使用 outbound。
控制面仍需自己的 TLS 身份；trusted_node_ca 仅在拨入 direct Worker 时需要，outbound-only 不要求虚构节点 CA 文件。
单一逻辑控制面本期部署在一个承载 Registry/连接的 Server 实例；公网 HTTP 与 gRPC 入口必须落到该实例。
多副本跨实例转发不在本期引入，不能靠共享 DB 假装共享内存连接。

### 现有表的最小变化

| agent_nodes field group | Contract |
| --- | --- |
| node_id / owner_user_id / display_name / mode | 原身份与归属不变；云注册 Worker 为 remote，宿主内嵌共享节点仍为 embedded |
| endpoint | 改为可选：outbound 节点为空；已有 direct 部署使用真实 HTTPS 地址。不使用虚构地址占位 |
| status / health / heartbeat / credential and lease hashes / expiries | 沿用原字段与校验；不存明文 token、流 ID、RPC 请求或结果 |
| created_at / updated_at | 沿用 |

补新增迁移放宽 remote 必须有 endpoint 的 CHECK，同时保留 owner、凭据与租约约束；不修改既有迁移。
available 由现有 Service 的租约、健康和当前已发布句柄共同推导，不新增在线字段或传输状态表。
注册 deployments 继续保存在现有运行时注册信息；离线仍能读节点列表与业务历史，Worker 目录操作不可用，执行时使用当前节点部署校验。

## 5. API、权限和统一归属

NodeInfo/NodePage/枚举保持唯一规范 owner，供产品使用时迁入 talos-types；不复制 ComputerInfo。
产品 DTO 使用 camelCase、String ID 与 Unix 秒时间；Actor 从认证推导，token/hash 不进入列表/详情响应。

| Product API | Request / response | Policy |
| --- | --- | --- |
| computer/list | ComputerListRequest { query?, cursor?, limit? } -> ComputerListResponse { data: NodeInfo[], nextCursor, total } | 服务端名称搜索与游标分页；默认 50、最大 200；total 与查询/权限范围一致 |
| computer/read | existing GetNodeRequest -> NodeInfo | 与选择器相同可见范围，包括离线/停用节点 |
| computer/create | existing CreateNodeRequest，endpoint 改为可选 -> CreatedNode | 缺省 owner 是当前用户；一次性交付凭据，Desktop main 保存，renderer 不接收节点密钥 |
| computer/rename | RenameComputerRequest { nodeId, displayName } -> NodeInfo | 只更新名称，不覆盖并发状态/地址；共享 embedded 不通过个人改名接口修改 |
| computer/setStatus | SetComputerStatusRequest { nodeId, status } -> NodeInfo | 个人节点启用/停用及统一撤销；不等于关闭电脑或退出 Desktop |
| computer/rotateCredential | existing RotateCredentialRequest -> NodeCredential | 已有凭据恢复和撤销路径；个人节点 owner / 管理员可用 |

HTTP `/v1/computers`、JSON-RPC `computer/*`、SDK `computers.*` 与管理 gRPC 共用 Service 和发布/撤销逻辑。
HTTP 路径：GET/POST `/v1/computers`、GET `/v1/computers/{nodeId}`、PATCH `/{nodeId}/name`、
PUT `/{nodeId}/status`、POST `/{nodeId}/credential`（后三者同属 `/v1/computers`）。
创建/轮换凭据响应使用 `Cache-Control: no-store`；Desktop 通用 renderer IPC 不开放这两个方法，由受信任宿主调用。
NodeService 统一持有原注册生命周期锁；传输发布时绑定撤销回调，停用、轮换、注销和重新注册释放原发布。
不新增发布管理器或状态表；改名使用独立 SQL，只改名称和更新时间。

联网 Desktop 与 Web 通过 SDK 访问云端产品 API；本地 stdio 保持同一业务契约。节点密钥只交付受信任宿主/部署端，
Web 手工连接允许一次性受控展示凭据，禁止加入通用查询缓存、日志或可重复读取 API。

| Target | Select / execute | Environment management |
| --- | --- | --- |
| 个人云注册 Computer，包含 Desktop 内嵌 Worker | 当前 owner；管理员跨用户管理目录不混入自己的任务选择器 | owner 管理完整 Worker 环境；Pod 限制由节点本地能力与沙箱执行 |
| Server 内嵌 Worker | 首位管理员自动认领并绑定个人 owner，签发 UUID；本机调用 loopback，对外使用中心 TLS 入口的 direct RPC | 其他管理员不隐式获得该 Computer 的执行或环境权限 |
| 显式本地模式 embedded Computer | 现有可信本地 Actor，包括 auth_disabled 的本地连接 | 现有本地权限；此规则不能放宽公网节点注册/管理的认证 |

配置/插件/MCP 权限按目标计算，不能直接复用控制面的管理员目录权限限制个人节点。
Computer 可见/业务执行权限与实例环境管理权限分别校验。computer/list/read 必须能返回普通用户有权访问的 embedded，
旧会话链接可以恢复其节点上下文；不因无管理权限隐藏自己的历史，也不因此允许修改共享实例配置。
无需新增历史入口或 grants 表；使用相同选择器与既有 Actor/业务资源权限，停用时仍可读取有权访问的历史。

| Existing contract | Change |
| --- | --- |
| SessionInfo / ProjectInfo | 返回 nodeId；Session 映射既有 execution_node_id，Project 增加 execution_node_id |
| SessionListRequest / ProjectListRequest | 增加可选 nodeId；Computer UI 总是传选中节点 |
| CreateSessionRequest / StartTurnRequest / ProjectCreateRequest | 增加可选 nodeId；新调用明确指定，旧调用保留现有缺省路由 |
| Existing Session/Project read, patch, queue, retry, compact and execution | 解析已存节点，拒绝冲突目标；无普通改绑或静默回退 |
| fs / Workspace / Memory / Skills / Plugins / MCP / execution config | 增加可选 nodeId；UI 显式指定；缺省本地旧调用保持既有语义 |
| Account, appearance, language, Cloud Auth / Server configuration | 保持账户、客户端或实例范围，不随 Computer 切换 |

新 Session 先解析 Project/显式 nodeId，再在目标节点的既有 deployments 中匹配 Router 所选 agent_id；
不匹配返回明确 InvalidRequest，不改选其他 Computer，也不新建调度器。模型列表、默认模型与选择校验来自目标 Worker。
新 Session 在目标节点确定后，通过现有 WorkspaceInfo 投影读取该 Worker 的默认 workspaceId 并持久绑定；
不使用控制面默认值。首次查询只需可信 Actor，由 Worker 解析默认 Workspace，不先要求调用方知道 workspaceId。
后续 Turn、恢复和旧 Session 沿用已保存 ID；Worker 更改默认值不改已有绑定，缺失旧 Workspace 时明确失败。
旧项目仅有单一历史节点则沿用，无历史则保留原 embedded 归属；多节点历史先明确解决归属再迁移。
目录唯一约束按 user/node/path 更新；路径解析、canonicalize、存在和权限检查由目标 Worker 完成。
本地旧历史不上传云端；云模式不会把本地 embedded Session 自动绑定到新 UUID。
上传附件与工具图片仍归控制面 FileService；Worker 文件不作为附件同步副本。上传列表按已有 Project/Session 关联展示。

## 6. 执行与环境 RPC

Connect 只增加传输信封，按 [协议附录](protocol.md) 复用 AgentHarnessRpc 的全部 Request/Response，
包括 admission -> commit -> accepted -> progress -> settlement、用户输入、审批、compact、Memory、Dream callback。
单调用队列满时明确以 ServiceUnavailable 结束该调用并按既有语义清理，其他调用继续；控制帧优先但不打乱同调用顺序。
不丢弃业务事件后报告成功；具体读写调度、取消和失联条件见协议附录。
AgentSessionDataRpc、AgentSubagentControlRpc、AgentToolImageRpc 继续由 Worker 主动调用云端，不绕路 Desktop。
执行通过 AgentHarness，环境通过 WorkerEnvironment；直连/反向流差异内聚在 agent-client 和既有 RPC adapter。
现有 RPC adapter 共用 Start/SubmitInput/Compact/Dream 流处理和全部 unary 方法；Harness 业务 PB 不变。
阶段 3.1/3.2 新增既有 Workspace/Fs/Model/Project、文档、Memory 和 Skills 的 PB 投影，direct/Connect 共用同一 Worker owner。
控制面的 TalosService 只做目标授权和路由，不组装 Fs/WorkspaceDoc/Memory/Skills 服务。
文档沿用版本检查和 Worker 的 UserWorkspaceMemory；Memory 沿用同一 MemoryStore，Skills 沿用同一 PluginManager。
断连后等待本地执行清理，未确认清理明确阻止自动重注册；Server 优雅退出先关闭长连接。

无 Session 的管理复用一个 WorkerEnvironment typed port 与 WorkerEnvironmentRpc；PB 是既有 DTO 的投影。
环境请求包含 NodeAuth、WorkerEnvironmentContext { userId, username?, workspaceId? } 和原业务对象。
workspaceId 缺省（JSON null / PB absent）由 Worker 解析自身默认，明确提供时须非空且存在；Session 执行上下文仍使用已保存的必填 ID。
控制面构造可信上下文，个人节点 Worker 验证 lease owner；内嵌本地调用使用可信 Actor 与原权限。
不传控制面文件根、不伪造 Session、不新增环境 Registry。

| Capability | Existing objects / owner |
| --- | --- |
| Directory read/create + Project directory status | FsDirectoryReadRequest/Response、FsDirectoryCreateRequest/Response、ProjectStatus；Worker filesystem |
| Workspace info/docs | WorkspaceInfo、WorkspaceDocList/Read/Write Request/Response；Worker resolver 与版本检查 |
| Memory list/read/write/search/get | MemoryFileList/Read/Write、MemorySearch、MemoryGet Request/Response；Worker 文件与可重建索引 |
| Skills / Plugins / plugin sources | 既有 Skills*、Plugin* 请求/结果；节点安装、启停和来源管理 |
| MCP list/import/authentication/credentials | McpServerList、McpServerImport、McpAuthentication*、McpCredentialsUpdate 请求/结果；节点认证和连接生命周期 |
| Models / execution config read/update/reload | ModelList、ConfigGet/Update/Reload 请求/结果；仅 Worker 执行设置，不含 Server/Cloud Auth 配置 |

配置重载责任明确分开：将现有 admission gate 按 node 细化，控制面持有它、检查该节点的非终态业务记录，并在整个 RPC 修改期间
阻止同节点新任务；其他节点不受影响。Worker 用本地执行 gate 检查真实运行，先预构建，再按原子写入/发布顺序更新运行时。
排队/审批/后续输入与配置修改都要遵守同一节点准入；Worker 不依赖 SessionStore，控制面不写 Worker 配置文件。
RPC 失败不自动重放修改；云端模型/配置缓存按节点失效，不生成第二份配置持久化。

### MCP OAuth 回调

纯本地保留现有 loopback 回调；联网授权使用控制面固定 HTTPS `/v1/mcp/oauth/callback`，
Worker 使用这个 redirect URI 创建既有 OAuth attempt，保留 SDK state、PKCE、client secret 和 token 交换。
控制面在向客户端返回 authorizationUrl 前，将原 state 的 hash 与 Actor/node/server/attempt/expiry 绑定为有界短期路由，
只转发授权回调到该 Worker，不另建 OAuth 管理器或凭据 DB。SDK state 不被改写，路由不代表授权完成。
Callback 经 CompleteMcpAuthentication RPC 交给原 attempt，Worker 验证 state/issuer、一次性完成交换并按现有提交规则保存凭据。
客户端通过原 ReadMcpAuthentication 读取最终状态；断连、重启丢失路由或超时明确要求重新授权，不自动重放回调。
具体接口与失败/清理规则见协议附录；联网本机和远端使用同一回调路径，不猜浏览器与 Worker 是否同机。

## 7. Computer UI 与 Desktop 接入

沿用已确认三列布局：56px 独立 Computer 栏、原项目/会话栏、主内容。
快捷项最多三台并含当前项，`…` + 隐藏数量打开完整列表，支持服务端搜索、分页、滚动与键盘。
管理按钮固定于左栏底部、已有登录用户按钮上方；账户菜单保持原行为。

设备名称旁用轻量“本机”标识，窄栏用 tooltip；本机标识、在线状态和选中标记相互独立，参考图不固定样式。
Desktop 的安全 IPC `desktop/workerIdentity` 返回 { nodeId, controlPlaneEndpoint }，不含密钥；
UI 用它与当前云控制面及 NodeInfo.nodeId 精确匹配本机，不能根据 remote/embedded、同名或网络地址猜测。
Web 不知道访问者的本机身份，不冒充客户端本机；同一设备在不同 Desktop 上的标识不同。

登录后的首次选择优先本机 UUID（注册中显示接入进度），已有会话链接或最近选择优先；本机接入失败不自动改选远端。
Web 没有最近选择时，一台自动选，多台明确选择；没有节点显示连接指引。
选择 Computer 同步切换项目、会话、环境设置和新任务目标；跨节点 Project 选择先同步目标。
旧 Session/运行任务不迁移，草稿保留；离线可读云历史，依赖 Worker 的操作明确不可用。

| Frontend location | Responsibility |
| --- | --- |
| features/computers/ | SDK queries, selected-node context and node-scoped cache; reuse app store |
| features/computers/ComputerRail.tsx / ComputerOverflowMenu.tsx / ComputerItem.tsx | Independent rail, full list, shared device/local/status/selection presentation |
| pages/computers/ComputersPage.tsx | Node list, connection guide, details, rename, enable/disable and credential recovery |
| Existing AppLayout / Sidebar / settings / account menu | Compose layout and selected-node views; preserve existing account behavior |
| Desktop main + existing SDK/IPC composition | Compose one management runtime and Worker; enroll/share after login; retain local business services and safe identity |

账户/控制面/nodeId/资源 ID 隔离缓存、草稿和最近选择；快速切换的旧响应不能覆盖当前节点。
每台 Desktop 的当前选择独立，云历史共享；关闭窗口不等于注销 Worker，退出 Desktop 才按宿主退出流程释放。
默认节点改名不改变 UUID 或本机标识。账户切换时清理前一账户缓存与连接。

## 8. 验收

- 实际云控制面 + 两台同用户 Worker（含 Desktop 自带 Worker）+ 另一用户 Worker；普通电脑只发起出站连接。
- 同账号 Web 和第二个 Desktop 都能看到并操作第一台 Computer；Cloud Auth、产品 API 和 Worker 租约身份一致。
- Desktop 首次本机、轻量标识、切远端、最近选择恢复；真实文件/目录效果与实时 Timeline 验证目标节点。
- UUID 重启/改名不变，CP 重启后重连；旧连接不会撤销新连接；离线/续租失败/停用无静默回退或重复执行。
- 本机和远端覆盖输入、审批/等待、compact、子任务、附件/图片、Memory callback；环境修改只影响目标 Worker。
- 配置修改与任务准入竞态、不同节点互不阻塞；不同部署 agent_id 的匹配失败明确可见。
- 控制面与 Worker 的默认 workspaceId 不同，新 Session 正确绑定目标；更改 Worker 默认后旧 Session 仍用原 ID。
- 另一台 Desktop/Web 完成目标 Worker MCP OAuth；校验错误 state/issuer、重复回调、超时与断连，密钥仅留目标 Worker。
- 普通用户自己的旧 embedded 项目/会话可见、链接可达且操作权限不变；共享实例配置仍不可越权。
- 一条慢调用过载明确失败，另一调用的 Interrupt/commit/回调仍可推进；同调用事件顺序与结算语义不变。
- 新 18_computer_workspace group 同时覆盖 API 和实际 Electron/Web UI；搜索/分页使用显式小 limit。
- 既有 17_multi_node direct 场景与 09_app_server、纯本地模式回归；生产默认域名需独立验证 TLS/HTTP/2/长连接部署。

阶段 1 注册闭环已通过真实 Auth/Server/Desktop controller 验收，证据与范围见[阶段记录](plans/01-registration.md)。
完整执行、环境、产品 UI 与安装产物的实际结果见 [验收记录](verification.md)。公网部署和跨物理机器验证另列，不由本地注册结果推断。

阶段 3.1 的实现接口和缺省语义见协议附录第 4 节。离线 Project 返回既有元数据与 nodeUnavailable 状态；网络失败不读取控制面同路径。

## Review 修复后的边界

- 首次注册只保存 pending 时，由用户在管理页选择账号拥有的 outbound UUID 恢复；已有 UUID 不可重绑。后台 Runner 失败沿宿主通知上报 ApiError，不改变登录身份。
- 有期限 RPC 在 Client 与 Worker 两侧执行 deadline；仅取消对应调用。Project 状态读超时保留 DB 元数据并显示 unavailable。
- Memory 工具、Dream/compact 和环境 API 共用 Worker 实时配置与同一 MemoryStore；单次操作使用配置快照。
- Project 记录只属于 query，客户端 Store 只保留选择/草稿等 UI 状态；LLM/WebSearch/Profile 与其他 query 共用 talos 前缀，账号切换在 AppProviders 的 Store 变更边界统一清理，再挂载新 observer。移除未消费 MemorySlice/假数据与无调用 Worker getter。
- 窄屏同一 Sidebar 通过原生 dialog 展示，支持关闭、Escape 和焦点恢复；不引入第二个 Sidebar。

修复验收包含实际登录按钮/轮询、首次 pending UUID 恢复、第二 Desktop/Web 派活、旧 embedded Project/Session 续跑、暂停 Worker 超时，以及配置 reload 后 Memory 工具/compact。证据见 [verification](verification.md)。
