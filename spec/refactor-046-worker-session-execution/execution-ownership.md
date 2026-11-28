# 执行与产品数据落地

[主 Spec](design.md) · [模块拆分与产品组装](worker-entrypoints.md)

[四层与产品存储合成总图](assets/architecture-overview.svg)汇总本方案边界，详细契约以主 Spec 和本子 Spec 为准。

## 目标与约束

本子 Spec 落实主 Spec 的 [业务要求](design.md#业务要求)、[数据归属](design.md#产品数据归属)和[技术决策](design.md#技术决策)。
产品归属统一引用主 Spec，本文件细化持久化、状态流转、上下文、任务管理和验收。
本次包含 Talos 侧 Sync 接入解耦与可选装配；Sync 独立服务、协议、加密及密钥改造不在范围内。

原故障中的完整历史响应为 4,558,084 字节，超过 4 MiB RPC 接收限制。
内嵌 lib 已消除本机序列化边界，本次通过 Worker 本地历史、查询投影和上下文预算处理历史增长。

## 状态与持久化接口

| 状态 | 落地 owner 与约束 |
| --- | --- |
| 产品 Session/Project 配置、Memory 管理配置、任务定义和计划 | 现有产品 Service/Store 唯一写入，按主 Spec 归属装配在本机或 Server。 |
| 执行派生 Timeline、生成 Memory 结果和执行进度 | 由产品 Service/Store 消费 Worker 已提交事实构建物化视图，允许落后并按游标补读。 |
| 执行历史、准入输入 | Worker 持久化 owner，携带稳定 Session/Turn/命令身份与历史代数，在启动 Provider 和发布事件前提交。 |
| 检查点、摘要、原生续跑状态、Memory 形成进度 | Worker 执行资源及 Provider 私有状态；日常读取在执行边界内完成。 |
| 产品摄取游标、幂等结算状态 | 对应产品 Store，连续游标与 Timeline 在同一事务提交，未完成业务结算进入其已有持久状态。 |
| 可同步产品变更及 tombstone | 产品 Store 的常驻事务捕获，独立于 Sync 装配/开关/网络，按 S1–S6 保持后续可导出。 |
| 目录、凭证、租约和可调用发布 | Server 现有账号与注册模块，Relay 通过类型化权限接口使用。 |

目录与节点管理通过新控制面 lib 统一调度既有 Store 和授权接口。
产品服务通过窄端口请求节点就绪、活动保护及托管唤醒，保留任务资格、Session 队列和账单 owner。
Worker 与产品服务的调用上下文使用宿主/管理者稳定身份，不从 Sync 初始化获取身份，也不等待 Sync ACK。
Sync 适配器按可选能力接入产品提交与注册生命周期，原有捕获与物化语义保持。

### 常驻捕获与恢复

产品写入通过原 Store 事务装饰器记录变更和 tombstone，复用已有 resource_revisions。
捕获只使用同步无关的本地资源、owner、代数、身份、前后差异与顺序，不依赖运行中的 StoreSyncAdapter。
业务写入与变更记录一起提交或回滚；捕获失败按原事务错误处理。
删除前保留必要身份与状态，即使实体行已移除仍能生成原协议的删除事实。
引用事件水位或外置元数据时，要保留可重建差异的必要基线，避免源数据先被清理而记录无法导出。

Sync 恢复装配和授权后，已有导出器读取已提交本地记录，按原资源/账号映射、版本和水位构建原 outbox。
以原 has_source_gap/import 栅栏保护尚未处理的本地缺口，先完成该资源的本地事实转换，再允许相关远端事实覆盖。
导入物化继续跳过回声捕获和执行副作用；保留原 Loro、wire、ACK、账号认领与绑定规则。
记录安全回收须依据既有消费语义，未消费修改和 tombstone 不能因关闭功能或登出而丢失。
这属于产品事实记录，与执行事件交付的“无逐事件 ACK/Worker outbox”契约分别适用。

产品库和执行库分别管理 schema、连接和事务。Worker 执行库固定为本节点 data_dir 下的 worker/execution.sqlite，由 WorkerExecutionStore 独立打开；不复用产品库连接。
跨边界通过 Worker API 和已提交事件交互，禁止直接读取或写入对方表。
AppServer/TUI 和 TalosServer 复用共享产品运行时及产品持久化能力，内嵌或远端 Worker 提供执行端口。
控制面客户端只处理登录/接入、凭证和连接恢复，不代替本机产品管理者，也不持有 Server 多节点权威状态。

内嵌和独立 Worker 使用相同服务实现与类型化持久化接口，各自持有节点 ConfigService、执行历史/检查点、Memory 形成进度和适用的节点资源/凭证实例。
同一 Worker 的 lib/WSS/gRPC 入口共享服务、已提交数据和锁；产品 Service/Store 仍按本机或 Server 产品归属装配，不因代码复用装入独立 Worker。
节点注册凭证保存在接入宿主，Server 注册权威记录保留在控制面；二者各循原凭证生命周期。

Worker 通过本地历史查询完成窗口选择和字段投影。
移除日常 Turn、续跑、压缩与 Memory 形成对 Server 或查看端 ReadSessionHistory 的完整历史回调。
新输入附件及确属产品服务所有的资源沿用受权限约束的资源端口。
Memory 形成进度保存在 Worker，生成的产品管理记录通过结果交给对应产品服务物化。

## Worker 服务与物理存储边界

共享同一套服务、类型化持久化接口及适用的 Store 实现，每个 Worker 独立持有实例、命名空间与数据。
产品库与执行库分别管理 schema、连接和事务；不跨库直接改表，不把完整产品 Session 库搬入 Worker。
Store 实现复用按能力和记录范围进行，不能据 DatabaseSessionStore 当前实现多个契约，就将全部产品能力作为 Worker 依赖。

| Worker 能力 | 实际代码现状（main b0c77b34） | 设计处理与物理边界 |
| --- | --- | --- |
| 模型、搜索配置及 reload | ConfigService 定义在 talos-service，由 talos-worker::config::service 组合 ConfigStore/YamlConfigStore、gate 和 reload_hook。 | 直接复用节点配置文件、env 秘密、校验与原子发布；产品发送策略另归产品配置端口。 |
| Memory 内容与索引 | runtime.rs 已组合 MemoryService、MemoryStore、UserWorkspaceMemoryService，并打开 memory-index.sqlite。 | 保留实际 workspace/user scope、文件内容和节点索引，不重建平行 Memory 系统；产品管理记录仍属产品服务。 |
| 插件、Skills、MCP 状态与凭证 | PluginManagerService、PluginStateStore、McpCredentialStore 已在 Worker 组合。 | 保留既有节点路径、权限及凭证生命周期，按 Worker 命名空间隔离。 |
| Provider 续跑与私有产物 | KernelSessionArtifactStore/FileKernelSessionArtifactStore 已在 Worker 组合并按 Provider 路径写入。 | 保留 Provider 私有文件/产物 owner；逻辑边界下沉不要求搬迁这些已在本地的文件，也不强制塞入执行数据库。 |
| 完整执行历史、准入输入、有序事件和终态 | 已有 SessionHistoryReader、AgentExecutionStore；当前 AgentExecutionStore 由 DatabaseSessionStore 实现，并混合执行事实、产品通知和 Timeline 查询。 | LocalExecutionHarness 将执行记录绑定 WorkerExecutionStore；产品通知、派生 Timeline 和业务账本留产品 owner。复用原身份、代数和提交规则。 |
| Memory 形成进度 | SessionMemoryProgress 借用产品 SessionStore 字段，standalone 的 RemoteMemoryProgress 经 AgentSessionDataClient 回调。 | WorkerExecutionHistory 将 KernelMemoryProgressStore 接口绑定 Worker 本地进度，随执行身份/代数持久恢复；移除这两类日常执行反向依赖及仅供旧路径使用的代码。 |
| 文件、浏览器状态及工具产物 | BrowserRuntime、既有文件/工具资源接口由实际宿主提供。 | 在实际资源宿主持久化和鉴权；新附件经授权端口交付，不以执行下沉为由复制全部产品文件。 |

实例隔离使用已有 Worker/node 身份、Session/历史代数及授权 workspace/user scope；不建立另一套标识系统。
配置文件、env/凭证、Memory 文件及索引、执行数据库和 Provider 产物可使用不同后端，均沿原 owner 的路径/锁管理。
内嵌/独立 Worker 复用相同实现；同 Worker 的 lib/WSS/gRPC 共享服务、配置状态、execution gate、资源绑定及生命周期，重连只恢复交付。

产品侧保留 Session/Project 配置、Goal/Loop 定义与计划、Automation 管理状态、派生 Timeline、用量及业务结算。
云产品由 Server 持有，Desktop/TUI 由所属本机持有；双方只通过受保护 API、已提交事件与游标交互。
产品物化保留来源执行事件身份和 Worker 游标，产品自生通知不能写回执行日志或推进 Worker 来源游标。
Worker 启动、续跑、压缩和 Memory 形成从自身执行库及原 Provider 私有存储读取，不向 Server 获取历史或进度。

### 执行事实与产品投影的结构

WorkerExecutionStore 与 DatabaseSessionStore 分别服务执行层和产品管理层，不是两套对等执行 owner。
Worker 的权威事件与检查点供本地执行使用；产品服务消费同一 AgentEvent，保留事件副本并构建查询和管理视图。

| 数据 | Worker 执行库 | 产品库及消费用途 |
| --- | --- | --- |
| 执行 Session | 绑定、历史代数、活动 Turn、Memory 处理进度 | 产品 Session 身份、标题、归属、项目、模型选择及统计；不是完整执行 Session 的镜像。 |
| 执行事件 | 原 AgentEvent、来源游标、执行结算事实 | 原 identity/sequence/occurred_at/payload 保持一致，展开为产品行，增加产品 event_seq 和 persisted_at；产品序号不替代 Worker 来源游标。 |
| 活跃状态 | Worker 原子提交的执行状态 | 由来源事件派生的查询索引，允许落后，不能替代 Worker 的最终执行校验。 |
| 准入记录、检查点及 Provider 续跑状态 | Worker 持久化并用于幂等、上下文、续跑和中断处理 | 不做对应完整复制，也不作为产品侧管理的执行状态。产品自己的请求幂等和排队记录不等同于 Worker 准入。 |
| 历史补充和轨迹载荷 | 完整载荷留在 Worker | 已封存的展示元数据按原契约物化，大载荷通过授权、有界 Worker 读取提供。 |
| 摄取与产品结算 | 不新增交付 outbox 或逐事件 ACK | 产品库保留来源游标、事件映射及业务消费进度，用于去重、补读、用量归账和后续调度。 |

这是同一事件模型的持久化展开及产品投影，不引入第二套业务事件类型，也不要求两边表结构相同。
事件副本用于产品历史查询；Worker 检查点用于本地执行，两者的重复数据有明确消费者。
产品执行事件写入口只保留 Worker 事实摄取；已被替代的 commit_initial_admission、commit_progress_event(s)、commit_running_input_admission 及同类执行直写接口和仅供其使用的实现须删除。
产品命令记录、管理通知、来源校验及业务消费事务仍由原 owner 保留，不因清理执行直写接口一并删除。

目录与数据库文件名保持不变：执行库为 `<data_dir>/worker/execution.sqlite`，Memory 索引为 `<data_dir>/memory-index.sqlite`。
独立执行 schema 内不使用部署角色前缀；以下是本次须完成的表名调整，不表示现有代码已经改名：

| 迁移前实现名 | 目标领域表名 |
| --- | --- |
| worker_sessions | sessions |
| worker_admissions | turn_admissions |
| worker_events | agent_events |
| worker_checkpoints | turn_checkpoints |
| worker_supplements | session_history_supplements |

同步调整执行 Store 查询、外键、索引及其验证，不新增旧名兼容写入路径。
已有 Worker 执行数据须通过所属 Store 的 schema 迁移保留，不能以删库重建替代表名调整。
Memory 既有表名保持不变；产品库 worker_projection_cursors / worker_event_projection 表达来源与用途，保留此命名。
跨数据库同名不表示跨库共享表、连接或事务，也不要求字段对齐。

参见 [Worker 执行 Store](../../talos-rs/crates/orchestration/store/src/execution/mod.rs)、
[本机 Harness 执行装饰器](../../talos-rs/crates/orchestration/agent/src/local_execution.rs)、
[Worker 历史与 Memory 进度](../../talos-rs/crates/app/talos-worker/src/execution_history.rs)、
[产品事件投影](../../talos-rs/crates/orchestration/store/src/session/worker_projection.rs)、
[独立 Worker 接线](../../talos-rs/crates/app/talos-worker/src/registration.rs)和
[Provider 文件产物](../../talos-rs/crates/orchestration/agent-provider-kernel/src/artifacts.rs)。

## 一致性与命令受理

落实主 Spec 的 [C1–C8 一致性要求](design.md#数据一致性决策)。
产品服务可持久保存待处理输入和管理请求，但在 Worker 确认准入前只能展示等待受理状态。
已准入输入及其执行事件的权威记录在 Worker，产品层不能提前伪造执行进度或终态。

派发携带既有稳定命令身份、Session 绑定及所需配置/定义版本。
Worker 将受理身份、输入和接受的版本与准入状态持久关联，在返回结果前完成提交。
相同命令重试返回原受理结果或已有执行状态，身份与原载荷/版本冲突则沿用类型化冲突错误。
准入响应丢失不构成创建新 Turn 的依据，恢复仍通过既有命令/调用关联和事件读取完成。

Goal/Loop 及管理配置变更由产品服务提交新版本，下次派发携带对应版本。
Worker 只按已接受版本推进执行。暂停、停止或中断当前执行通过已有控制契约受理，
产品层分别记录请求状态和执行端已确认状态，不通过产品库写入覆盖 Worker 的活动检查点。
取消请求使用独立管理 token，对产品 API 公开 cancellationRequested；state、事件游标和观察时间只由 Worker 已提交事实投影更新。拒绝取消只清理对应请求，不回滚或覆盖新的执行状态，终态删除活动投影时一并清理请求。
本次不扩展 Sync SDK 或协议。Sync 只复制已确认执行状态，不同步临时取消请求或 token；多端通过所属产品 API/Relay 查看实时请求状态。

产品 Timeline 在 Worker 保留的代数内可从已提交事件重建。
重建只更新派生视图及摄取位置，保留 Server 自己的配置、账本及幂等结算记录。
管理入口为 `session/rebuildView`，HTTP 为 `POST /v1/sessions/{sessionId}/rebuild-view`；Rust/TypeScript SDK 与内嵌入口复用同一 Service。调用者须有该 Session 的写权限和原 Worker 产品绑定。复用 Session lease、节点准入 gate 和维护 reservation，拒绝与本进程活动 writer/准入并发；已有 WaitingForInput 事实可以重建，继续使用原 Turn/action 身份续跑，不启动新 Turn。

先按 Worker 原代数从零游标有界分页，将事实暂存到原 SessionStore 的私有 staging 表；暂存期间原视图继续可读。收齐后在单个产品事务内校验来源指纹及已有映射覆盖，再替换派生事件、活动状态及统计，保留原产品事件游标、系统通知、配置、Goal/Loop 当前业务状态、命令身份、已结算账本及 Dream 消费游标。未见过的新来源后缀沿原幂等结算路径处理，不派发后续任务。中断或失败不清空旧视图，重试重新暂存；不增加 ACK、补传或恢复服务。

已映射的 `TurnStarted` 产品行还保存自动化累计记账水位、Loop 决策等业务元数据，属于产品账本。若该行丢失，Worker 源事件不能证明原始业务水位；重建须在替换任何派生行前以类型化 `Conflict` 拒绝，不得用零用量或空决策补造。原起始业务元数据仍在时，等待事件、活动索引及其他派生事件仍可按原规则修复。

Worker 新增后缀可在重建后沿返回游标继续摄取；若暂存期间产品摄取已越过该快照，则明确冲突并重试。保留范围之外、权限失败、映射账本不完整或过代游标明确报错，不能凭空补造已消费记录，也不能通过清空 Worker、变更代数或重建命令来绕过错误。

产品视图落后时展示最后已确认进度和补读状态。调度器不能因旧视图或断线假定 Turn 已失败或完成，
须关联原 occurrence、结算事实和 Worker gate 检查后再派发后续工作。

## 正常执行与上下文

1. 产品服务验证账号、Computer、Workspace、Session 和 Agent 的持久绑定。个人远程请求先通过 Server 目标授权，再由所属宿主验证产品权限。
2. 共享 Worker gate 保留并准入调用，在启动 Provider 前提交输入和执行身份。命令与调度 occurrence 的稳定身份用于识别结果未知的重试。
3. Worker 从本地执行历史、摘要、检查点与已校验资源准备上下文。查询先限制字节、记录数和字段，再应用字符/token 预算。压缩按有界批次读取历史，并保留产品 Timeline 事实。
4. Worker 持久提交有序事件与终态事实后再交付。产品服务物化 Timeline 并处理产品副作用，个人产品在本机提交，Server 管理产品在 Server 提交。

AgentHarness 通过配置解析 AgentProvider，保留 Provider 原生续跑和私有产物。
未来 Provider 复用同一 Harness 契约。Goal 绑定一个 Harness 和一个 Session。
跨 Agent 协作保留 Coordinator 已有职责，跨 Agent Goal 拆分不属于本次实现。

### 等待输入与续跑

产品服务保存展示所需的待处理问题及稳定操作身份，提交用户回复时携带既有 Session、Turn、操作身份和命令身份。
Worker 在原执行 gate 内校验活动状态、历史代数、待处理操作及重复命令，从本地执行记录和 Provider 私有存储恢复 continuation/checkpoint，再继续同一 Turn。
产品服务及 SDK 不负责查找、保存或回传 Provider 私有续跑引用或执行检查点；Worker 对外的执行事实、结算和产品投影只包含交付及业务消费需要的领域信息。
产品侧可复用同一事件 reducer 生成只读历史或展示快照，但不把这些派生快照作为 Worker 的续跑检查点或执行恢复来源。
不能通过复制 Provider 引用到产品库，再由 Server 传回来满足 Worker 的续跑前置条件。
产品视图落后时先沿原游标刷新或返回原类型化状态错误，不能以旧副本覆盖执行状态；重试继续使用原命令身份。
本机内嵌和注册 Worker 复用同一 Worker 命令契约及校验路径，不新增另一套远程续跑实现。

## 断连与结算

准入提交前丢失调用者，释放未消费的 reservation 与 permit。
准入持久提交后，查看端、个人 Relay 或 Worker 执行连接断开只分离交付；授权仍有效时，同一 Turn 继续运行和提交事件。
明确中断、授权撤销、租约到期与真实宿主关闭沿用其作用域内的取消和 drain 策略。

注册 Worker 的临时断连不得在授权有效时销毁已准入调用。
需要订正现有“连接断开即清理调用”的分支。重连关联已有 invocation，不能再次消费准入许可。

在现有认证 Worker API 上提供已提交事件的有界读取。

| 契约 | 行为 |
| --- | --- |
| 权限 | 每次校验账号、Worker、Agent、Session 和历史代数。 |
| 游标 | Worker 签发的独占 afterCursor，表示最后连续提交事件之后的位置；实时流和补读使用同一游标空间。 |
| 分页 | 稳定事件身份、完整产品事件及必要结算事实、最后返回位置和是否仍有已提交数据；限制序列化字节与记录数。 |
| 空页 | 保持游标，活动 Turn 后续仍可产生事件，空页不表示终态。 |
| 清空与删除 | 按原 Session gate、代数、tombstone 和授权规则处理，拒绝跨 Session 与过时代数游标。 |

clear/delete 由产品服务接收授权请求，经同一受保护 Worker gate 应用新的代数或删除事实。
执行端确认结果后物化相应产品状态；请求处理中与已完成状态明确区分。
双方按已确认代数过滤晚到旧事件，产品视图重建不得越过 tombstone。

清空或退休执行来源前，产品服务在原准入保护下补齐 Worker 事实，并由原业务 owner 完成待结算效果；此阶段不派发后续任务。任一结算失败就保留来源、产品历史及待结算记录，返回可见错误。产品 Store 在清空、单 Session 删除和整树删除的事务中再次拒绝未结算记录，不能通过切换代数或外键级联绕过结算。
删除主 Session 同时删除其子 Agent 树：先验证整树空闲并完成全部结算，再从深层子节点到根节点逐个通过同一受保护 Worker API 退休，最后删除产品身份。中途失败保留产品整树身份供原请求重试；已退休来源沿现有幂等契约处理。来源恰为产品下一代、空闲且无完成事件时可确认前次 clear/retire 已应用，其余代数不一致仍显式拒绝，不以空历史回退。
已清空且没有执行载荷的下一代来源，退休时保留当前代数并设置永久退休标记，避免连续产品事务失败将重试推到无法确认的更后代数。永久退休标记与清空校验处于同一写事务，后续清空不能重新打开来源，也不能恢复准入。


产品服务以稳定事件身份去重，补齐缺口后再推进游标，并将事件和连续游标原子写入产品库。
个人远程查看端通过所属宿主的产品 API 经 Relay 补读。Relay 可转发查看端提供的授权位置，不持久保存个人事件游标或重放队列。

终态可能首次经游标读取到达。产品结算按原 Turn、dispatch/occurrence 身份和定义版本匹配用量、Goal 完成与动态 Loop 决策。
此处产品结算只消费 Worker 已提交的执行结算事实；执行的等待、取消、完成和失败及其持久化均由 Worker 完成，产品服务不能再次驱动执行状态机或补造终态。
缺少 Goal/Loop 注册这类不可重试的业务结果，不伪装为存储故障。其产品通知与来源回执的结算确认在同一产品事务提交；重复恢复不新增通知，通知写入失败仍保留原回执待重试。
结算保留原执行模型身份，不读取结算时的新 Session 模型配置来归账。Worker 本地 Memory 反射产生的辅助模型用量随同终态持久化，沿原幂等用量 owner 结算。
Worker 已持久接受后，即使产品视图事务暂时失败，也不释放原命令或队列身份、不重新执行；返回原受理身份，随后沿既有游标补读完成物化与结算。
执行轨迹的完整 payload 留在 Worker，通过原权限范围内的有界读取提供；时钟与调用索引等已封存元数据随终态事实提交，在产品游标事务中物化，不能因为执行下沉丢失原轨迹能力。
已有账单与调度 owner 在推进游标前幂等提交副作用，或记录在其已有待结算持久状态中。
重复读取、事件重叠与设备副本导入不得重复工具执行、计费或任务触发。
恢复只需游标补读，不增加逐事件 ACK、Worker 投递 outbox 或新的恢复服务。

切换查看端保持产品管理者不变。授权管理者切换必须在 Worker 检查空闲并原子准入下一 Turn 时完成。
未消费 reservation、运行 Turn、等待输入和未确认清理都阻止切换。
Sync ACK 不作为准入条件，Session 不自动迁移到其他 Worker。

## Goal、Loop、Memory 与 Automation

复用产品服务中的定义、控制、调度与展示逻辑，按主 Spec 装配在本机或 Server。
调度要求所属运行时常驻。关闭查看端不影响常驻管理者，前台 TUI 退出则遵循既有宿主关闭策略。
本机计划不因断网或退出而自动转交云端。

| 任务 | 产品管理者负责 | Worker 负责 |
| --- | --- | --- |
| Session Goal | 目标、版本、预算、暂停、进度展示与后续 Turn 触发；校验 Session 空闲和用户队列。 | 在单个 Harness/Session 内执行已准入目标，持久化进度、检查点与完成证据。 |
| Session Loop | 周期、到期时间、版本、停止状态与结果；幂等应用动态延时/停止决策。 | 执行已准入 occurrence 并返回类型化结果。 |
| Dream Memory | 计划、暂停/恢复、run-once、派发和结果管理。 | 有界处理执行历史，形成 Memory，持久化进度并返回产品结果。 |
| Profile Usage Retention | 在所属产品 Profile Store 执行启动、定时或授权 run-once 清理。 | 无 Worker 执行步骤。 |
| session-loop-scan | 扫描所属产品的待执行 Goal/Loop，通过既有队列选择任务。 | 只执行已准入的任务。 |

保留 Goal/Loop 互斥、预算校验、用户队列顺序和取消规则。
模型/工具内循环、按需压缩和 Session Memory 注入归执行层，User/Workspace Memory 保留资源权限。
Dream 维护使用已有空闲 Session 维护边界，不创建聊天 Session 或 Goal/Loop Turn。
Dream 的工作区必须在原维护锁和节点活动保护内，复用正常 Session 的目标 WorkerEnvironment 解析已保存 workspace_id 与持久用户 namespace，不按 Server 工作区配置拼接根路径。

标题生成同样由目标 Worker 通过原 Harness 语义端口读取本地有界终态历史并执行模型；保留原调用身份、结果及用量。产品只写生成标题并通过既有 Profile 幂等归账，不保留可直接调用 provider 的 InnerModel 实例。模型选择随目标 Worker 配置更新生效；辅助失败仍沿原产品 notice／错误处理边界，不改变主 Turn 的执行结果。

Office 预览的转换进程和临时 PDF 在目标 Worker。SessionPath 按已绑定 Session 的节点和资源根授权；产品 File 原件继续归产品 FileStore，转换节点由请求 nodeId 明确指定，省略仅指原本机入口，缺少本机能力时明确拒绝。产品只保存有时限预览票据、授权与描述，读取产物继续经原 Worker handle，关闭／到期复用原清理机制。

Dream 的模型选择、凭证、调用和 Memory 形成均在目标 Worker，复用其 ConfigService、provider router 与既有内置模型策略，不再把 prompt 返回 Server 执行。Worker 保留同代 Dream 调用事实（原 request ID、模型、用量、形成结果/类型化失败和已覆盖来源位置），每次按产品消费游标最多返回一条；产品先以原 request ID 幂等结算，再持久化自身消费游标。结果交付或产品事务失败后，按原游标补读，不重新调用模型、不增加 ACK 或出站投递队列。只有已读尽该游标后的形成事实时，才可将返回的 Worker 处理进度视为已覆盖，避免丢失崩溃前已产生用量的调用。该产品消费游标与派生 Timeline 独立，视图重建保留，Session clear 才随代数清理。


Server 产品服务对符合条件 Goal、Loop、Dream 和 Dream run-once，通过控制面 lib 请求唤醒自动 IdleSleep 中托管 Worker。
并发工作合并唤醒，等待就绪后重新校验资格，每个 occurrence 至多准入一次，活动许可覆盖执行到结算。
手动停机保持明确意图，普通 Docker/Native 不可达沿用现有可用性错误与重试策略。
个人 Desktop/TUI 的物理机器唤醒不属于本次范围。

Automation 页面复用 builtinJob/list、builtinJob/control 和 BuiltinJobInfo。
本机控制进入本机 owner；个人远程控制经 Relay 到所属宿主；云产品控制进入 Server。
只在执行 owner 接受后展示 active，被动列表和刷新不得唤醒 Worker。

参考 [Goal/Loop](../feat-053-session-goal-loop/design.md)、[Memory](../../docs/architecture/MEMORY_ARCHITECTURE.md)和[托管生命周期](../feat-054-managed-talo/design.md)。

## 切换与旧历史

切换前 drain 活动工作。新执行只进入主 Spec 定义的产品归属与 Worker 契约。
原会话保留已有电脑中的本地只读历史，新工作创建新 Session；旧会话不能续跑、恢复等待调用或派发 Goal/Loop/Memory 工作。
本次不进行云端回填、远程旧历史转发或执行状态迁移。

历史只读资格在 Talos 的 Session 服务及已有导出/导入适配器内落实，防止旧记录进入新执行和远程发布路径。
沿用已有历史记录与权限，不删除磁盘内容。此处不修改 Sync 服务、协议或密钥生命周期。

移除调用者持有远端执行权的旧路径、完整历史反向回调、Shared-history 准入屏障及仅服务于它们的代码与配置。
个人产品远程访问按新 Relay 产品接口实现，保留实际仍需的权限化文件和媒体能力。

## 验收

- 在云端和 Sync 关闭时，从正常 Desktop/TUI 入口执行、管理全部任务、重启读取历史，并验证前台退出策略。
- 对照七类能力验收已本地能力直接复用，Provider 私有文件不强制搬库；执行 Store 拆分不携带完整产品 Session/Timeline/账本，Memory 进度无产品 SessionStore 或远端回调依赖。
- 验证同一 Worker 多入口共享节点配置与执行持久化、不同 Worker 实例/数据隔离，重连不重建、重启可恢复，产品账本和节点凭证各守其 owner。
- 从 AppServer/TUI 验证共享产品运行时与产品库确实启动，接入关闭或重连不影响本机产品持久化；独立 Worker 无完整产品服务或 Server 控制面依赖。
- 从两个客户端操作 Native/Docker/Talo，确认 Server 持有产品记录、计划和结果，Worker 独立执行。
- 在 Sync 关闭时远程操作个人宿主，验证所属宿主提交产品事实、Relay 有界转发与无个人内容落盘；离线目标明确不可用。
- 构造超过 4 MiB 的历史，验证续跑、压缩、Memory 形成的查询和 prompt 均受预算约束，单个大事件也符合传输上限。
- 分别在准入前后断开本机观察者、Relay WSS 和 Worker 连接，验证释放或持续执行、游标补齐、重叠去重、一次结算和过期游标拒绝。
- 验证租约到期、撤销、明确中断、等待输入和宿主关闭；并发管理者切换只能在空闲时准入。
- 中断产品摄取事务后恢复，确认连续游标与 Timeline 一致；检查产品库和执行库所有权及事务边界。
- 清空并重建执行派生视图后继续等待输入的 Turn，验证 Worker 使用自身 continuation/checkpoint，产品库和命令不持有 Provider 私有续跑引用；已有产品配置和幂等账本保持不变。
- 核对两边同一事件的身份、序列、发生时间和 payload 一致，只有产品序号/入库时间及派生字段不同；执行表改名后的查询、外键与索引不保留旧名写入分支。
- 验证产品服务无法通过旧直接写入接口创建准入、执行进度或终态；断连及旧副本均不能替代 Worker 的执行决定。
- 丢失准入响应后以相同命令重试，覆盖载荷/版本冲突，确认原输入和 Turn 只准入一次。
- 在活动 Turn 中变更定义或暂停，确认已接受版本与管理请求状态分离，控制操作须有执行端结果。
- 延迟 Server 摄取并重建派生视图，确认最后已知进度可展示，不能自动判断失败、重复派发或重复结算；独立配置与账本保持其 owner。
- 覆盖补读超出保留范围、过代游标、删除确认与晚到事件，确认无空历史回退或旧内容复活。
- 覆盖托管 Worker 的 Goal/Loop/Dream 唤醒、手动停机、run-once、活动保护与被动读取。
- 分别在不装配 Sync 与已装配但关闭开关时验证执行、注册、游标恢复和副本无执行权；重新开启使用原协议与 outbox/ACK 恢复语义，不增加密码学验收要求。
- 以已同步的新架构 Session/Project/File 元数据为基线，在无 Sync 构建内修改、清空、删除并重启，再恢复同步；确认其他设备收到原协议差异/删除，远端旧内容不覆盖或复活。
- 验证捕获与业务写入原子回滚、必要前像/基线保留、新代重建、未授权数据不上传，以及重复恢复不产生回声或结算副作用。
- 验证旧历史本地只读、clear/delete 与晚到事件，不出现旧执行兼容路径。

确定性状态与所有权行为进入现有单元/集成测试门禁，真实多机、独立外部服务与 GUI 拓扑单独记录本地验收。

## 实现落点与验收边界

- WorkerExecutionStore 使用独立执行 schema、连接和事务；LocalExecutionHarness 在发布前提交准入及执行事实。产品只能读取已提交来源，不提供写入 Worker 终态或补造游标的管理 RPC。
- AgentCommittedEvent 保留原执行用途（Turn/Compaction）、模型、历史补充、事件身份及结算，产品投影在同一产品事务提交来源游标与视图；结算继续使用原业务 owner 的幂等账本。
- 执行事件捕获预算为 256 KiB，单事件传输上限 512 KiB，每页最多 256 条且不超过 2 MiB。大产物经有界、授权的来源载荷接口读取；不能恢复完整历史 RPC 或静默截断。
- WorkerExecutionHistory 在 16 MiB 本地读取预算内，由 SQL 先限制记录数、字段、单项大小及 UTF-8 总字节；大 Turn 可跨窗口读取，不截断落盘历史。窗口和 Worker 本地 Memory／压缩进度按消息的真实创建位置推进：使用 MessageStarted，无开始事件的已准入输入使用其完成事实；最终内容来自对应的 MessageCompleted，canonical Turn 的终态序号保持原值，读完最后一条消息后才推进到该终态。必需的既有输入／工具调用上下文可重复附带，但不重复声明覆盖。各类 Prompt 独立上限（含关闭某类）必须保留实际消费的连续前缀，不能越过未纳入的已启用内容。单条 canonical message 或其必需上下文超出字节、记录或已启用 Prompt 投影预算时，返回类型化上下文超限错误，保留历史且不推进进度；块级分页不在本合同范围内。本地读取预算与模型 Prompt 预算分别生效。 Dream 和压缩复用既有上下文源按实际转录预算选择连续消息前缀；若按 Turn 合并会删空可读历史，则缩小读取窗口并由源重新计算覆盖，不把裁剪后的空输入当作已处理。手动压缩与自动压缩使用同一模型信息预算推导；未知上下文窗口仍保留原无显式预算策略，单消息超出已知预算时显式失败。
- 常驻 capture 模块复用 resource_revisions，Sync feature 仅决定复制适配器。账号授权、前像、基线及导入缺口栅栏遵守 S1–S6，不改变 wire、Loro、ACK 或密钥协议。
- 节点资格、发布与托管唤醒经 talos-control-plane 的窄端口；稳定宿主身份由现有本地持久化维护，不依赖 Sync 初始化。最终全组及真实拓扑验收按上述清单执行，实际结果单独记录。
