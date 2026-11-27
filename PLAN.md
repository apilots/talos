# Talos 产品架构与模块拆分实施计划

## 依据与当前状态

本计划落实 [主 Spec](spec/refactor-046-worker-session-execution/design.md)、
[执行与产品数据子 Spec](spec/refactor-046-worker-session-execution/execution-ownership.md)及
[模块拆分与产品组装子 Spec](spec/refactor-046-worker-session-execution/worker-entrypoints.md)。
业务要求和架构决策以主 Spec 为准，计划跟踪实施依赖与完成条件。
[四层、五产品及存储总图](spec/refactor-046-worker-session-execution/assets/architecture-overview.svg)统一展示既有决策；本计划实施顺序不变。

中文 Spec、架构总览、七模块依赖及执行一致性时序图已归档。
新增[Worker 存储边界图](spec/refactor-046-worker-session-execution/assets/worker-storage-boundary.svg)与[七类服务清单](spec/refactor-046-worker-session-execution/execution-ownership.md#worker-服务与物理存储边界)，区分既有节点能力与已下沉的执行持久化。各阶段代码已落地，最终验收和质量检查进行中；阶段完成以全部成功标准通过为准。
沿用独立 worktree 和 docs/worker-session-execution 分支，实施变更不写入 main。

## 交付边界

- Desktop/TUI 完整本机自管，Talo、Docker、独立 Native 统一由 Server 管理产品 API、产品数据和调度。
- 所有产品复用共享 Worker 执行内核、产品服务、Store 端口、领域类型和 SDK 适配器。
- 个人远程访问使用可裁剪 Relay lib，云产品 API 直接进入 Server 产品服务。
- 抽取 talos-control-plane lib，归拢节点注册、租约、目录、发布和托管生命周期；通过受限端口与产品调度和执行协作。
- 执行历史、上下文、压缩、Memory 形成与恢复归 Worker，准入、锁、资源校验和结算由全部接入路径共享。
- 执行事实由 Worker 单写；产品 Timeline 为最终一致视图。配置、请求受理和业务结算按各自唯一 owner、版本及幂等身份管理。
- 本次把 Sync 接入与 Cargo 依赖改为可选，解除身份/注册/路由/执行耦合；保留协议、开关、绑定、捕获和物化语义，独立服务、合并、加密与密钥不改造。
- 变更/tombstone 捕获是常驻 Store 事务能力。未装配期间的修改和删除后续也必须同步，不能仅保留旧 outbox/ACK 或依靠重扫。
- 未来移动端复用产品 API，本次不实现移动端；QUIC/P2P、跨机器迁移、跨 Agent Goal 拆分和新密码学协议不属于实施内容。

## 七大模块实施映射

对照 main b0c77b34 的现有代码落实[主 Spec 七模块划分](spec/refactor-046-worker-session-execution/design.md#基于实际代码的七大模块)。

| 模块 | 主要代码基础与实施目标 | 关联阶段 |
| --- | --- | --- |
| M1 产品入口/API | tui、SDK、gateway/app-server/http 保持统一领域契约，装配本机与远程路径。 | 1、3、4、5 |
| M2 产品管理 | service/store/coordinator/router 按产品归属装配；常驻变更/tombstone 捕获独立于 Sync。 | 1、2、3、5 |
| M3 Agent 执行 | worker/agent/Provider SPI 承接本地历史、准入、上下文与恢复。 | 1、2、5 |
| M4 Worker 环境 | config/skills/mcp/tools/browser/sandbox 按实际节点装配，模型配置按 nodeId 管理。 | 1、2、3、5 |
| M5 Server 控制面 | 抽取 control-plane lib，从 AgentNodeService、managed_worker 和宿主装配归拢节点管理。 | 1、3、5 |
| M6 个人 Relay | 提取 relay-server lib，依赖控制面路由端口，可独立裁剪。 | 1、4、5 |
| M7 个人 Sync | 导出/复制/导入适配器可选，消费常驻捕获并保持原同步协议。 | 1、4、5 |

## 五种产品的组装与验收

| 产品 | 装配范围 | 核心验收 |
| --- | --- | --- |
| TUI | 终端入口 + Embedded SDK + 本机产品服务/Store + 内嵌 Worker/环境 | 本机离线可用、同进程调用、Sync 可选。 |
| Desktop | 桌面 UI + AppServer/IPC + 本机产品服务/Store + 同一 Worker/环境 | 个人远程发布、nodeId 配置、无 Sync 也可注册和远程访问。 |
| TalosServer | Web/API + 云产品服务/Store + 控制面 lib + Worker 绑定 | Relay 与 Sync 独立装配的四种组合均验证；云产品始终可用。 |
| Docker Worker | 同一 Worker binary + 容器环境 + 主动 gRPC Connect | 单一内核、注册和租约；Talo 托管生命周期由 Server 控制面管理。 |
| Native Worker | 同一 Worker standalone 入口 + 本机环境 + 主动 WSS | Server 产品 API 与存储、目标节点模型配置、本地执行历史与断连恢复。 |

## 四边界的实施对应

| 边界 | 实际落地类型 | 组装与生命周期要求 |
| --- | --- | --- |
| 共享产品运行时 | talos-base 的 TalosApplicationRuntime，TalosService、TimelineEventService、Automation、产品 Store | AppServer/TUI 和 Server 复用；本机也启动产品库与常驻捕获，宿主监督产品调度及关闭。 |
| Worker 运行时 | talos-worker 的 TalosWorker、Harness/Provider、WorkerRpcHost、ConfigService | 内嵌/独立复用执行库及环境能力，受保护 API 绑定生命周期由 Worker/宿主管理。 |
| Server 控制面 | talos-control-plane 持有 AgentNodeService、ManagedWorkers，复用 ManagedWorkerRuntime | 只在 Server 装配多节点权威目录、租约、发布及托管管理，产品调度保留原 owner。 |
| 控制面客户端 | AgentNodeClient、AgentNodeRunner、RegisteredWorker、WorkerEnrollment、现有 SDK/DesktopCloud | 用户端登录接入和节点凭证登记各循原流程，宿主管理身份/连接/重连；客户端不依赖 Server 控制面服务实现。 |

AppServer/TUI = 共享产品运行时 + 内嵌 Worker + 可选接入/Sync。
TalosServer = 共享产品运行时 + Server 控制面 + 可选 Relay/Sync + 内嵌或远端 Worker 绑定。
每个 Worker 自持配置、执行历史/检查点、Memory 进度和适用资源/凭证的服务实例与数据；同一 Worker 的全部接入共享实例和锁。
独立 Worker = Worker 运行时 + 控制面客户端。上述边界对应 M1–M7，实施顺序仍采用以下五阶段。

同机并发纯管理 CLI（Goal/Loop 定义管理、Session 列表/详情/模型选择、Models、Status）复用产品 Store/Service/Gateway，不重复装配 Worker 或启动调度、注册与 Sync；前台 start/resume/exec 持有唯一执行命名空间。用并发 Goal 暂停/Loop 停止场景验证该装配边界，保留原跨进程管理能力，不增加 loopback RPC。

## 阶段 1 — 对齐基础与接口落点

**目标**　核对已合并的内嵌 lib 和主动 Connect 基础，确定数据与模块迁移的实际位置。

**实施内容**

- 在独立分支核对远端最新实现及 PR 208、209 的共享 gate、资源绑定、注册发布和宿主生命周期。
- 按主 Spec 的 B1–B8、M1–M7、A1–A10、T1–T17、C1–C8 和 S1–S6 建立现有调用到目标 owner 的迁移映射。
- 核对四边界的实际类型，区分 TalosApplicationRuntime 的共同产品字段、Server 管理实现和可选接入客户端；复用已有入口与注册协议。
- 确定执行库布局、schema owner、事件分页上限及单个大事件表示。
- 对照子 Spec 七类服务清单区分已本地与待下沉记录，登记每 Worker 命名空间及 ConfigStore/历史/检查点/Memory/资源凭证 owner。
- 核对 ConfigService 混合字段，按 Llm/WebSearch 节点能力和 SessionMessageSendPolicy 产品策略拆分接口、原配置后端与消费者。
- 确定 managed Memory/结算、控制面窄端口、Relay crate 路径、Relay/Sync optional feature 和能力发现字段。
- 核对 AgentNodeService/managed_worker、ApplicationSyncHost、desktop_worker、节点网络适配器和三层 Cargo 强依赖；细化稳定宿主身份迁移与原账号绑定保留。
- 确定 resource_revisions 常驻捕获类型、删除前身份/基线、消费与安全回收水位，先明确无 Sync 期间写入和恢复契约。
- 明确个人 Desktop/TUI 和独立 Native 的注册角色，复用现有唯一领域类型。

**成功标准**　接口、依赖、数据写入者和迁移位置可直接用于下一阶段实施；上述实现前细化项都有明确落点。

**验证**　核对现有正常入口、依赖图、错误契约和相关测试夹具，记录基线与实际 owner。

**状态**　已完成：既有 owner、接口落点、数据库边界和产品装配已对齐。

## 阶段 2 — 常驻变更捕获与 Worker 执行下沉

**目标**　先使产品变更捕获独立于 Sync，再让 Worker 从本地执行状态完成已准入工作。

**实施内容**

- 先在既有 Store 内抽出常驻事务捕获，修改、clear/delete、代数和 tombstone 在同事务记录，移除无适配器时跳过捕获的依赖。
- 保留原 owner/资源/执行节点身份、必要前像和安全恢复基线，业务回滚同步回滚记录，捕获失败按原事务错误处理。
- 从 AgentExecutionStore/原 Store 实现拆出完整执行历史、准入输入、有序事件/终态及执行检查点，绑定 Worker 执行库；产品通知/派生 Timeline 与账本保留产品归属。
- 将 KernelMemoryProgressStore 绑定 Worker 本地进度，替换产品 SessionMemoryProgress 和 standalone RemoteMemoryProgress/AgentSessionDataClient 日常回调。
- 已本地节点配置、Memory 文件及 memory-index.sqlite、插件/MCP/凭证和 Provider 私有产物沿用原实现/scope/路径；不强制搬迁文件或塞入执行数据库。
- 内嵌/独立复用 ConfigService 及类型化持久化实现，每 Worker 自持服务实例和节点数据；同一 Worker 的 lib/WSS/gRPC 复用原实例、状态、Store 与锁。
- 对历史查询先做窗口、字段、字节和记录数限制，再应用 prompt 字符/token 预算。
- 订正 Turn、续跑、压缩与 Memory 形成的依赖，移除日常完整历史反向回调。
- 在受保护执行路径提交输入、事件与终态，复用命令/occurrence 身份处理结果未知的重试。
- 持久关联命令身份、准入输入和已接受版本，支持准入响应丢失后返回原受理结果；冲突载荷/版本明确报错。
- 提供已提交事件的有界游标读取，支持有效授权下断连继续执行、重连关联原调用及补读。
- 保持 lib、WSS、gRPC 的共享准入、SessionOwner、gate、适用的 OS 锁、资源 guard 和终态提交。

**成功标准**　常驻捕获在无 Sync 构建中完整可用；Worker 本地有界读取历史，掉线后原 Turn 可完成且不会重复准入或结算。

**验证**　无 Sync 的修改/删除记录、事务回滚、前像/基线保留；大历史、准入前后断连、等待输入、中断、撤销、重启恢复、跨进程锁与资源权限。

**状态**　已完成：常驻捕获、独立执行库、Worker 历史/进度及游标投影已接线。

## 阶段 3 — 控制面抽取与产品管理组装

**目标**　独立控制面统一节点管理；产品服务按归属在本机或 Server 装配，产品、控制面和执行边界清晰。

**实施内容**

- 在现有 Service/Store 中消除具体 Server 宿主依赖，注入产品 Store、受保护执行端口和宿主生命周期。
- 抽取 talos-control-plane lib，归拢注册、凭证、租约、目录、发布/撤销和托管生命周期，复用 auth、节点 Store 和 talos-worker-runtime 驱动。
- 细化 Actor/SessionRecord/NodeAdmissionGates 等现有依赖的窄端口，防止控制面反向依赖产品服务；宿主注入唯一活动保护和资格接口。
- Desktop/TUI 装配完整本机 Session、Goal、Loop、Memory 管理、Automation 和调度器。
- 明确 AppServer/TUI 也启动共享产品运行时及产品库；从共同启动中分离控制面服务和可选接入，独立 Worker 保持只装配 Worker 与客户端。
- 在同一 Base 组装库内裁剪 Server 生命周期/监听/托管依赖，产品入口逐项转发可选 feature；个人发布客户端不依赖 Server Relay。发行物逐入口构建和归档，防止 Cargo 联合构建合并 Server feature；CI 同时保护独立依赖图、个人无可选组件和完整 Server 测试。
- Server 为 Native、Docker、Talo 装配对应产品服务、产品库、统一 API 和调度器。
- 配置字段按节点/产品职责暴露窄端口，Server 产品配置、任务与结算保持原归属；独立 Worker 不装配完整产品管理，RegisteredWorker 接线不重建 Worker 服务。
- 统一配置入口按类型分派，Llm/WebSearch 由目标 Worker ConfigStore/env 落盘并 reload；SessionMessageSendPolicy 由产品宿主原 ConfigStore 保存，移除 WorkerEnvironment 中产品策略旧分支，同步契约和消费者。
- 保留 Worker 模型/Skills/MCP 配置 owner，页面及本机入口复用目标 nodeId 的既有配置接口和 reload/gate。
- 产品服务幂等物化事件，Timeline 与连续游标原子提交；用量和任务结算由对应已有 owner 处理。
- 区分待处理请求与已准入事实、配置请求与执行端确认、最后已知进度与终态，避免落后视图产生重复派发。
- 支持从 Worker 保留事件重建产品派生视图，保持独立配置、结算账本和幂等记录；覆盖越过保留范围时的明确错误。
- 保留单 Session/Harness Goal、Goal/Loop 互斥、用户队列、预算、动态 Loop 决策及内置维护行为。
- 接通托管 Worker 的 Goal、Loop、Dream/run-once 唤醒、就绪复核和活动保护。

**成功标准**　每个 Session 和任务只有一个产品写入者及调度者；本机可独立工作，云产品通过 Server 提供完整管理和多客户端访问。

**验证**　控制面无环依赖、唯一节点状态/锁、注册/租约/撤销、托管生命周期；云端及 Sync 关闭时的本机全功能、双客户端云产品、配置节点隔离、幂等物化结算和全部任务。

**状态**　已完成：个人产品通过原产品服务的窄端口装配，不创建 Server 控制面；共享产品服务可独立构建，唯一准入保护保持共享。talos-base 的 server/relay 仅由 Server 入口显式装配，个人端的 relay-client/sync 独立可选；依赖图与独立产品构建验收编译期隔离。

## 阶段 4 — 可选 Relay、Sync 解耦与统一远程 API

**目标**　个人产品远程访问和云产品直达在模块、依赖和组装上各有明确路径。

**实施内容**

- 从现有 gateway/talos-base 逻辑抽取 talos-relay-server lib，迁移个人连接、路由、关联、有界转发、背压和撤销。
- Relay 消费账号/目录和传输契约；云产品服务、Worker 内核与注册能力独立于 Relay。
- Relay 消费控制面鉴权后的目标与发布端口，不依赖产品库、任务调度或 Sync。
- 将 ApplicationSyncHost 中身份、账号接入、目录、注册和个人连接迁出；Sync 仅通过可选适配器消费原有提交/注册生命周期。
- 客户端接入沿用 AgentNodeClient/AgentNodeRunner/RegisteredWorker、SDK 和 WorkerEnrollment，把凭证保存与重连生命周期交给对应宿主；Server 侧管理实现只装配在控制面 lib。
- 稳定宿主/管理者身份保持原持久值，Sync 使用同一身份，连接与准入不再依赖 TalosSync.peer_id()。
- 审查 talos-base/service/store 的传递 Sync 依赖，实施独立 optional feature；关闭 Sync 不影响注册、目录、Relay、执行及游标补读。
- feature 仅裁剪导出转换、复制网络与导入适配器；常驻捕获不依赖 Sync SDK/Loro/运行中账号传输。
- 保留现有协议、绑定、outbox、checkpoint 和 ACK；重新装配/开启 Sync 沿原机制恢复，不重建身份或删除已有同步数据。
- 恢复装配后先按原水位处理未装配期间的本地修改/删除，再通过原导入缺口栅栏允许相关远端事实覆盖；保留原账号认领和导入无回声行为。
- 把个人远程操作接到所属宿主产品 API，保留源端权限校验及现有文件/媒体能力。
- 以 optional Cargo dependency 支持完整 Server 与云服务专用 Server，按已有产品能力契约公开可用能力。
- 同账号 WebUI 和 SDK 复用产品 API，按持久 Session/Project 绑定选路，保留个人选择器与独立 Talo 入口。
- 为交互、控制、文件和媒体保留有界队列、背压、取消与适用的分通道处理。

**成功标准**　Relay 与 Sync 独立装配；云产品与控制面完整可用，个人转发只依赖 Relay，身份与同步协议语义稳定。

**验证**　四种 Server 构建；已同步新架构数据→无 Sync 构建→修改/清空/删除/新代→重启→恢复装配，确认授权数据传播、无旧内容复活与原协议/ACK 保持。另验登录/注册/路由独立、导入栅栏、重复恢复和文件背压。

**状态**　已完成：可选 Relay/Sync、远端产品 API、OAuth 与预览已接线。

## 阶段 5 — 新架构切换、旧路径清理与验收

**目标**　所有新执行统一使用新架构，完成产品入口和故障边界验收。

**实施内容**

- 切换前 drain 活动工作，新工作创建新 Session，旧会话保持已有电脑中的本地只读历史。
- 在 Talos Session 及已有导出/导入适配器落实历史只读资格，不改造 Sync 服务或协议。
- 删除旧调用端 Coordinator 持有远程执行权的 handler、路由、缓存、帮助代码和配置。
- 清理完整历史回调、Shared-history 准入屏障、无用 loopback 及迁移后的重复实现。
- 清理控制面/Relay 的原重复 owner、Sync 宿主残留职责与强依赖，核对七模块、五产品组装和四种 Server 能力组合。
- 确认清理与裁剪未移除常驻变更/tombstone 捕获，未消费记录及恢复需要的基线不会丢失。
- 覆盖 clear/delete、代数/tombstone、晚到事件、管理者空闲切换和宿主关闭。
- 覆盖准入响应丢失、配置版本变更、Server 视图落后、摄取中断、视图重建和删除确认期间的晚到旧事件。
- 验证节点配置/产品发送策略接口与落盘隔离、原 scope 权限、Provider 文件沿用、Memory 进度本地恢复和执行 Store 拆分；不遗留完整产品库或历史回调依赖。
- 验证同一 Worker 多入口状态/锁一致，两个 Worker 配置、历史/检查点、Memory 及凭证隔离；覆盖配置 reload 冲突、重连不重建及重启恢复。
- 验证四边界实际启动/关闭和依赖，覆盖本机产品库、可选接入断线、独立 Worker 登记、Server 多节点管理及各自作用域撤销。
- 按两个子 Spec 从正常产品入口完成验收，更新同一份 Review 材料与实际证据。

**成功标准**　主 Spec 的业务及架构要求可逐项验证，旧历史保持本地只读，执行路径无旧兼容分支和无用代码。

**验证**　相关 Rust 包使用 just test 执行测试，运行对应格式/Lint/编译检查和 pnpm --dir talos-app check:repo；根据变更选择相关本地 E2E 与 CI 等价门禁。
真实多机、独立外部服务和 GUI 拓扑分别记录实际本地验收，文档检查不替代运行时验收。

**状态**　已完成：清空/删除前完成原业务结算，事务拒绝丢失待结算记录；整棵 Session 树退休，连续失败重试及永久退休保护已覆盖；个人产品装配隔离和无用公开 loopback API 清理完成。受影响回归、真实 TUI/Web/Automation 主流程、独立复审及仓库质量门禁完成；真实多机与生产外部服务拓扑的发布验收限制单独保留在 output Review 材料中。

## 跟踪约定

每阶段依据成功标准更新状态。运行中发现的失败、验证证据、截图与自审记录写入根目录忽略的 output 目录。
本计划保留实施顺序与可复用验收要求，实际结果在 Review 记录及提交说明中引用。
