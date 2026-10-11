# 阶段 3：节点归属与 Worker 环境

Status: Complete。依赖：阶段 2 已完成。3.1 已验收，3.2 已验收，3.3.1 已验收；3.3.2 已验收，3.3.3 已验收；进入 UI 与交付。
目标：Project、Session 和所有环境操作一致定位目标 Worker；拆成三个可独立验收的切片。
方案：[design](../design.md)，[contracts](../protocol.md)。

| 切片 | 工作与完成条件 | 状态 |
| --- | --- | --- |
| 3.1 目录/Project/Turn | Project node affinity 与迁移；Session/Project 输出和筛选 nodeId；目标部署/模型/默认Workspace查询；目录读取 -> Project -> Turn -> 实际文件 -> Timeline 闭环 | Complete |
| 3.2 文档/Memory | WorkerEnvironment 的 Workspace、Memory、Skills；复用 DTO/版本检查/索引；两节点同路径独立效果，离线不落到控制面 | Complete |
| 3.3 插件/MCP/配置 | 安装与 sources、MCP 公网回调递交与 Worker 凭据、模型和执行配置；owner 权限、节点任务准入/重载互斥；节点间互不阻塞 | Complete |

步骤：

1. Preflight 检查旧 Project 历史归属；单节点沿用、无历史 embedded，多节点冲突解决后再迁移。
   不改 Session 历史绑定，不把纯本地历史上传到云端；按 user/node/path 检查唯一约束。
2. 更新 CreateSession/StartTurn/ProjectCreate 和列表契约；UI 明确 nodeId，已有调用保留约定缺省。
   先确定 Project/Computer，再验证 Router 的 agent_id 在目标 deployments 中；不匹配明确报错。
   新 Session 通过目标 GetWorkspaceInfo 取得默认 ID 并保存，后续沿用原绑定；首次上下文不预填控制面 Workspace。
3. 补单一 WorkerEnvironment typed port 和具体 PB 方法，direct/outbound 共用；本地仍直接访问同一 port。
   Registry 只增加实际需要的环境句柄，不创建环境 Registry。PB 转换逐能力使用既有对象，生成物与 owner 同步。
4. 依次完成 3.1、3.2、3.3，每个切片跑自己的真实节点效果验收与职责 Review，通过后再继续。
5. 控制面 admission gate 按节点保护任务/队列/输入/恢复和配置 RPC；Store 按节点检查非终态。
   Worker 本地 gate 保护真实执行和配置提交，不依赖 SessionStore；不能锁住资源读取或其他节点。
6. 上传附件/图片仍用 FileService，通过 Project/Session 展示归属；移除被替代的控制面文件 I/O 和 local/remote 业务分支。
7. 在 3.3 扩展现有 OAuth attempt：Worker 支持可信 relay redirect URI 与 CompleteMcpAuthentication；
   控制面固定 callback 只持有有界 state-hash 路由元数据，登记完成才返回授权 URL，不复制 PKCE/token。
   校验 Actor/node/server/attempt/state/issuer/expiry；无效回调不消耗有效 attempt，定义替换/停用/断连/超时清理。
8. 共享 embedded 沿用既有 Project/Session 与用户 Workspace 权限，实例配置管理员权限独立检查；不新建历史入口。

成功标准：三切片分别通过；同名路径不串机；Session 固定节点；个人 Worker owner 可管理环境；
共享实例权限不扩大；配置失败不重放，同节点修改/任务无竞态，其他节点仍可执行。

拟执行检查：

- just test -p talos-types -p talos-store -p talos-service -p talos-coordinator -p talos-gateway
- just test -p agent-protocol -p agent-client -p talos-agent -p talos-worker
- just gen-app-protocol；各能力 DTO/PB/SDK scoped checks；Rust 编辑后 fmt/clippy。
- 真实两 Worker 的目录、文档、插件/MCP/模型配置效果；并发重载/准入和不同 agent_id 负场景。
- 控制面 default、Worker pc 的真实创建/执行；Worker 更改默认后旧 Session 仍绑定 pc，旧 ID 删除时明确失败。
- 浏览器与 Worker 在不同宿主的 OAuth 授权、错误 state/issuer、重复回调/断连/超时；仅目标 Worker 保存凭据。
- 普通用户已有 embedded Project/Session 的列表、读写/继续任务权限保持，共享配置修改仍被拒绝。

实际结果：3.1/3.2/3.3.1/3.3.2 Complete；3.3.3 Complete。真实 E2E 已恢复并通过本切片验收。
边界核查：控制面保存业务记录，Worker 保存环境文件；不新增执行 Store、配置镜像、Secret 副本或伪造 Session。
下一步：每切片都记录实际证据；完成后进入阶段 4。

## 3.1 实施顺序

1. WorkerEnvironment typed port、PB、direct/outbound client 与同一 Registry 句柄；默认 Workspace、目录及模型目录归 Worker。
2. Project 节点字段/唯一约束/冲突迁移，创建与状态检查通过目标 Worker。
3. Session/Turn 显式节点、Project 一致性、目标默认 Workspace 与模型校验；沿用旧绑定，列表可筛选。
4. 相关 Rust/协议/SDK 检查，真实两个 Worker 的目录与执行闭环；修复问题并同步 Spec。

历史切片记录：3.1/3.2 验收后依序完成 3.3，最终证据在下节。阶段 2 恢复 E2E 与消费者检查已完成。

- 当前实际证据：520 个 Rust 回归全部通过、0 skipped；六个新真实边界场景覆盖迁移/唯一约束/Session Worker 输入与 TLS 三模式对等。SDK 54 个测试、全前端类型检查、三个宿主入口检查、scoped Clippy 与 check:repo 通过。后续源码修订再作定向复验。
- 实施过程：新增 18/06_workspace 场景；首次脚本字段拼写已修复，后续完整验收通过，证据如下。

## 3.1 实际验收与职责核查

- `just e2e-case 18_computer_workspace/06_workspace`：6 个场景全部通过，真实 Auth/Server/三个 outbound Worker/DeepSeek，无监听端口、假模型或 DB 写入替代。报告 `talos-e2e/reports/e2e_report_20261011_112837.md`；场景日志在 `reports/18_computer_workspace/06_workspace`。
- 验证目录/模型/默认 Workspace 来自选中 Worker；目录创建 -> Project -> Turn -> Worker 文件 -> 实时 Timeline；Project/Session 按 nodeId 输出/筛选；错误 owner、跨节点 Project/Session 请求被拒绝。
- 验证 Worker 停止后 Project 元数据可读且显示 nodeUnavailable；同 UUID 重启、更改默认后旧 Session 保留原 Workspace，新 Session 用新默认；删除旧定义后明确失败，已有元数据仍可读。
- 最终定向复验 15 个 protocol/coordinator 场景通过，包含模型投影与目标部署不匹配；520 个全相关 Rust 回归已通过，SDK 54 个场景通过，全前端类型检查通过。新 schema/TS 由原生成器生成。
- 删除控制面 FsService 组装和 Session 的控制面 Workspace/模型解析；现有 FsService 在 WorkerEnvironmentService 内复用，TalosWorker 唯一组装该 owner。direct/outbound 共用认证 adapter，embedded 直接调用同一 port。
- Project 只增加 execution_node_id 与 owner/node/directory 唯一索引；Session 复用已有节点字段。迁移事务拒绝多节点历史冲突并保持旧 Session；无新业务表、授权表、执行账本、配置镜像或资源同步。
- 上层只保存元数据/业务记录和沿用 FileService 附件；Worker 执行路径、配置与 Workspace 文件留在节点。没有 transport 分支向上泄漏，没有改动 Harness 业务接口/PB。
- 3.1 功能与 Spec 一致；3.2/3.3 的验收见下节，产品 UI 见阶段 4。

- 补充消费者兼容验证：talos-sdk-embedded、talos-tui、app-server-test-client 的 all-targets check 和 225 个回归全部通过；SDK/Web lint、fmt-check、最终 check:repo、8 份文档/26 个引用与 PB 方法检查通过。阶段 3.1 对相关旧入口的验证共 745 个 Rust 测试，另有 15 个定向复验（不重复累计）。

- 旧 direct 实际路由回归 `17_multi_node/02_routing` 7 项全部通过（e2e_report_20261011_113559.md）；18/02–05 17 项断言全部通过，阶段 2 恢复验收已完成。3.1、旧 direct 与 embedded 行为均已实测，无代码/Spec 偏离。

## 3.2 实施顺序

1. Workspace 文档：扩展既有 typed port/PB/Client/adapter，复用版本检查与 UserWorkspaceMemory；产品请求补 nodeId，控制面仅路由。
2. Memory/Skills：复用 Worker 的 MemoryStore、PluginManager 与配置，不创建索引/运行时副本；补具体 PB 投影和产品目标选择。
3. 真实两个 Worker 的同路径独立读写、版本冲突、索引检索、Skills 和离线拒绝验收；direct/embedded 回归、Spec 核查后完成。

历史切片记录：3.2 Complete，随后完成下节 3.3。

## 3.2 实际验收与职责核查

- 13 个文档/Memory/Skills 方法接入同一 WorkerEnvironment；19 个环境 gRPC 方法/typed carrier 与端口一致。RPC、HTTP、JSON-RPC 和 SDK 均支持目标 nodeId，消费后不传入业务参数；内嵌直接调用同一 owner。
- TalosWorker 使用已有 UserWorkspaceMemory、MemoryStore、PluginManager；控制面删除 Fs/WorkspaceDoc/Memory/Skills 服务组装及 Memory 构造参数。删除被替代的 WorkspaceInfo 实现/重复测试和 Skills 的冗余管理器构造路径。
- 文档版本检查、MEMORY.md 格式校验、Memory 文件/索引更新、Skills metadata/errors/warnings 和原业务 DTO 字段保持；默认配置解析发生在 Worker。没有新表、索引副本、业务状态模型、伪造 Session 或上层传输分支。
- 新 E2E 发现普通模型突发输出也会触发 8 帧接收队列过载。Connect 增加八条消息的接收窗口与累计消费确认，按调用施加背压；停滞五秒只失败该调用。原 Harness PB 不变，确认/窗口仅是 Client 内的实时传输状态，重连不重放。
- `just test -p talos-service -p talos-gateway -p talos-gateway-http -p talos-base -p agent-protocol -p agent-client -p talos-agent -p talos-worker -p talos-types -p talos-tools -p talos-memory -p talos-sdk-embedded -p talos-tui -p talos-app-server-test-client`：799 PASS；10 个已有 sandbox_e2e_tests 由独立 Sandbox E2E group 运行，本轮未运行。最终传输/环境定向复验 5 PASS，不重复累计。
- `just e2e-case 18_computer_workspace/07_environment`：6 PASS，报告 `talos-e2e/reports/e2e_report_20261011_125313.md`。真实 Auth、Server、三台 outbound Worker、DeepSeek；同路径/日期/Memory ID 隔离、版本冲突、索引替换、五项 Skills 操作、Harness 文档上下文、owner/offline 拒绝和同 UUID 重启均通过。
- Control-plane 主机上的内嵌 Worker 有自己的启动文档；验收比较其文件基线，确认远端操作不新增或修改那些文件。没有把“与控制面同机”误判为控制面文件 I/O。
- `18_computer_workspace/03_reliability`：4 PASS（报告 `e2e_report_20261011_125559.md`），验证实时进度、其他节点并发与真实长工具跨续租。`17_multi_node/02_routing`：7 PASS（`e2e_report_20261011_125737.md`），验证原 direct 路由/文件/历史/故障/重启。
- SDK 56 PASS；全前端 typecheck、SDK lint、三个宿主和三个消费者 all-targets check、scoped Clippy、fmt-check、生成物、仓库/引用与 diff-check 通过。新增 Clippy 参数数量问题已通过具名调用上下文修复，无新 lint suppression。
- 首轮测试的日期/插件状态路径及 HTTP helper 夹具问题已修复；断连回归等待 Worker 观察 EOF 后再撤销租约，不放宽鉴权断言。真实 E2E 和最终回归通过；不把早期失败/部分通过算成验收。
- 对照 3.2 Spec 无职责偏离。配置/插件/MCP 的目标权限、重载互斥与远端 OAuth 由下节 3.3 验收；产品 Computer UI 由阶段 4 验收。

## 3.3 配置、插件与 OAuth

| 切片 | 具体目标与验证 | 状态 |
| --- | --- | --- |
| 3.3.1 配置与准入 | 先分离 ConfigService 的产品非终态检查与 Worker 文件提交/发布；控制面按节点授权/准入，Worker gate 覆盖实际执行与重载；配置 PB 投影既有 DTO。验证同节点冲突、其他节点执行、失败不发布、不复制 secrets | Complete |
| 3.3.2 插件与 MCP | sources/install/enable/credentials/status 等通过原 PluginManager/MCP owner 与环境 RPC；移除控制面同能力组装。验证目标真实安装/启停/凭据落点，个人 owner 与共享实例权限 | Complete |
| 3.3.3 OAuth 与闭环 | 原 attempt 支持受信任 relay callback；控制面仅有界 state-hash 路由，原 Worker 验证/换取/保存。验证跨宿主回调、错误/重复/断连/超时清理，完成后进入阶段 4 | Complete |

依赖：3.3.1 -> 3.3.2 -> 3.3.3；每切片按具体业务对象扩展 PB，不使用通用 JSON Execute，不引入新的配置/OAuth 管理器或执行账本。
最终代码事实：ConfigService 已归 Worker，控制面按目标授权并检查该节点非终态；插件/MCP 和远端 OAuth 回调已复用同一环境端口。

## 3.3.1 实施记录（Complete）

- ConfigService 不再依赖 SessionStore，也不维护重复的模型目录；Worker 的配置 Store/原子提交/候选发布和 Provider 由 TalosWorker 组装。移除控制面旧重载 wrapper 和重复模型测试。
- 新增四个具体配置 RPC，投影既有 Config DTO/ModelInfo；HTTP/JSON-RPC/SDK 支持目标 nodeId。新增 HTTP explicit reload 与 SDK 对等入口。
- 控制面节点 gate 覆盖开始/输入/审批恢复/队列分派/恢复/会话修改；现有 SessionStore 按节点检查非终态。Worker 实际 invocation 从 paused admission 到 settlement 持锁，配置写入冲突显式拒绝。
- RPC adapter 改为共享实时配置；Harness 在执行 gate 内重新验证 Workspace/模型。已有 Kernel Provider 配置改为共享发布，使存量 Session 后续调用也使用重载后的工具配置；不重建 Session 或历史。
- 417 项 Rust scoped 回归、56 项 SDK 和全前端 typecheck 已通过。新增真实 18/08_configuration 的初版七项 PASS（报告 e2e_report_20261011_141005.md）；最终增加实际 Web Search 后八项 PASS，报告 e2e_report_20261011_141303.md。
- 最终 Kernel/Harness 65 项、Worker gate 两项通过；Clippy、SDK lint、生成物、仓库检查和 diff-check 通过。实际执行从 paused admission 到 settlement 排斥配置提交，结算后释放；故障原因仍沿用原结果。没有新增业务状态表、配置镜像或传输分支。
- 对照 3.3.1 Spec 无偏离。启用部署 ID 集变更须重启 Worker 再注册，重载会显式拒绝；配置接口保持既有 LLM/Web Search 管理范围。下一切片 3.3.2，当前任务已授权连续完成 3.3、4、5。

## 3.3.2 实施顺序（Complete）

1. 按已有 Plugin/MCP DTO 投影 PB；先拆共享定义/来源/安装与 MCP 凭据对象，机械映射独立模块，保持枚举/nullable/版本/错误。
2. 将既有凭据和 OAuth owner 从 TalosService 移到已有 PluginManagerService；Worker 组装 ConfigService hook，控制面只通过环境端口路由。清理控制面同能力字段/构造参数和同机生命周期引用。
3. 具体环境 RPC、HTTP/JSON-RPC/SDK 目标选择和 owner/shared-instance 权限；变更经过节点准入，Worker 保持提交/执行互斥。
4. 真实两 Worker 安装/启停/源刷新/凭据保存与工具效果，故障/重启和内嵌/direct 回归；通过后继续 3.3.3。

## 3.3.2 实际验收

- 按既有 Plugin/MCP 81 个可达业务类型投影 Protobuf；17 个方法复用既有业务请求/结果，产品请求仅补 nodeId。共 40 个环境 RPC，内嵌/direct/Connect 同一 owner、认证与客户端。
- PluginManagerService 复用现有凭据/OAuth 生命周期和 ConfigService publisher；删除 TalosService 配置/插件字段与构造参数。宿主持有 Worker 负责 shutdown，不新增管理器或持久化镜像。
- Worker 插件/凭据提交在真实执行期间返回 Conflict；控制面按目标 owner/shared admin 授权并按节点准入。其他节点仍可修改和执行。
- 真实 18/09_plugins 七项 PASS（e2e_report_20261011_145301.md）：来源/搜索/读取/安装/刷新/更新/启停/卸载，私有凭据替换、DeepSeek 实际 MCP 工具文件效果，节点隔离、并发准入、重启和离线。
- 320 项 scoped Rust 回归初轮 319 PASS、1 个并发 OAuth 取消错误竞态；增加过期 attempt 取消优先检查，修复后该实际授权/刷新/撤销/重授权回归 PASS。SDK 56 PASS、全前端 typecheck、三个消费者 all-targets check、scoped Clippy/生成物/fmt 通过。后续 OAuth 改动再定向复验。
- 对照 Spec 无职责偏离；SDK 复用 canonical request，HTTP 请求和查询自动携带新增字段，业务 PB 不携带上层 nodeId。浏览器远端 OAuth 仍未验收，进入 3.3.3。

## 3.3.3 实施记录（Complete）

1. 原 SDK/Worker attempt 增加可信 relay URI 和标准回调递交；PKCE/换取/凭据仍归 Worker。
2. 控制面仅保留有界 state-hash 路由和当前环境句柄弱引用；固定 HTTP callback，原 Actor/node/attempt 绑定。
3. 清理替换/停用/断连/过期 attempt，无效回调不消耗，调用失败不重放代码；真实异宿主客户端/Worker 回调与原 loopback 回归。

- 当前实际证据：41 个环境 RPC（新增 Complete 与可信 callback target）；原 SDK、单一 Worker attempt、标准回调通道与当前 lease 取消 hook。控制面短期 state-hash 路由、容量预留、Weak 环境句柄和单回调互斥；无凭据、PKCE 或授权账本副本。
- 107 个执行/协议/MCP scoped 回归 PASS，scoped Clippy PASS；真实 TLS/HTTP2 ingress、Auth、Server、outbound Worker 的主要五项 OAuth PASS（e2e_report_20261011_151200.md）。最终扩展停用/真实换取中断连/过期/logout，正在等待原十分钟 deadline；不缩短生产期限，也不通过 DB 注入模拟验收。

- 最终真实 18/10_oauth 十项 PASS（e2e_report_20261011_151446.md，621.75 秒）：TLS 公共 callback，无 Worker listener；错误 state/issuer 不消耗 PKCE；仅原 Worker 一次提交凭据；真实 DeepSeek MCP 工具与重启；连接替换/断连/停用清理；实际 code exchange 中断连不重放；原十分钟过期与持续心跳/续租；logout 路由失效。
- 停用安装时通过原 recover_credentials 生命周期取消 pending attempt，保留已保存凭据。SDK 无效回调保持原状态；实际提交 gate 覆盖 exchange 到发布，取消路径释放它。控制面持原 Actor 和弱环境句柄，不复制 PKCE/token，无新的 OAuth manager/Store。
- Scoped Clippy、fmt、SDK/Web typecheck、执行/协议/MCP 107 项回归通过；新签名同包编译和真实 E2E 均通过。对照 3.3 Spec 无职责偏离。物理异宿主与公网部署不由同机隔离进程的结果代替，最终交付明确列验证范围。


Review 修复：Memory tools/Dream 共用实时 Worker 配置与 MemoryStore；Project 状态读取 timeout 保留 metadata，原权限错误保持。真实 Workspace/reload/Memory 工具/compact 验收通过；无调用 Worker getter 已删除。 [实际结果](../verification.md)。
