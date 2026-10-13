# 阶段 1：节点注册全流程

状态：**Complete**。用户已授权按阶段实施；本阶段交付真实节点管理/注册闭环，
Harness 远程调用在阶段 2。每小阶段结束核对实现与 [Spec](../design.md) 后再推进。

## 1.1 契约、包与配置

状态：Complete。依赖：已完成实施前计划审查。

工作：

- 创建最终使用的 agent-pb、agent-protocol 包及构建声明；生成全部既定 PB，
  本步只使用 auth/node 控制与管理契约。PB 生成使用唯一构建入口，库内无注册或数据库逻辑。
- protocol 定义 NodeAuth/节点归属/注册结果；将独立的 AgentCapabilities 及相关枚举
  从 talos-agent 迁入共享位置，避免 protocol 反向依赖 Harness 实现。原 agent-api 消息/事件不迁移。
- 沿原配置系统增加节点身份、控制面 TLS 地址、注册凭据文件、节点监听地址和 TLS 信任配置；
  node_id 由所属用户在控制面预置并稳定保存，凭据不写 Workspace。租约周期使用 Spec 默认值。
- 独立节点入口跟随 1.3 的真实注册 Runner 交付；不增加仅加载配置的空运行模式。
- 更新 Cargo workspace、仓库实际使用的生成/构建声明；`Cargo.lock` 在本仓库被忽略。
  当前没有配置 schema 生成器，配置契约以原 serde 配置和运行时校验为准。

成功标准：原配置入口能够读取节点配置；控制 RPC 的 Rust 类型可生成；没有重复领域定义或循环依赖。
验证：`just check -p agent-pb`、`just check -p agent-protocol`、`just check -p talos-agent`、
`just check -p talos-config`。
阶段出口（2026-09-21）：PB 唯一来源、共享能力迁移、节点配置和现有 Harness 兼容检查通过；
`just check -p agent-pb -p agent-protocol -p talos-agent`、`just check -p talos-config -p agent-pb -p agent-protocol -p talos-agent`、
`just test -p talos-config -p talos-agent`（136/136）、新增安全配置校验后 `just test -p talos-config`（100/100）、
`just fmt-check` 通过。对照 Spec 未发现执行业务漂移；Runner 入口仍按 1.3 交付。

## 1.2 Store、节点管理入口与凭据

状态：Complete。依赖：1.1 已通过。

工作：

- 在原 talos-store 增加 agent_nodes migration 和 NodeStore，远程节点记录 owner_user_id；本阶段不添加
  Session 亲和列。SQL 草案的最后两条 Session 语句在阶段 2 单独迁移。
- 实现 create/get/list/register/renew/heartbeat/unregister/update/rotate 的必要事务，
  当前 hash 条件更新防止旧请求覆盖新租约；数据库只保存 hash。
- AgentNodeService 归原 Service；控制面 TLS listener 暴露 NodeControl/NodeManagement。
  管理 metadata 的用户 Bearer token 经原 auth_service.verify_access_token 产生 Actor，
  Service 检查节点归属；普通用户可管理自己的节点，管理员可代管。NodeControl 使用
  NodeAuth，节点 token 不能当作用户管理 token。
- 节点所有者或管理员可用生成的 AgentNodeManagementClient 创建节点、查看状态、启停和轮换凭据；
  提供可执行的调用说明。无需新增 UI、管理框架或另一套管理员身份系统。
- 对应新增材料遵循原 TalosError/ApiError contract，token/hash 不进入查询结果或诊断输出。

成功标准：用户能管理自己的节点、不能管理他人节点；管理员可代管；节点 token 不能冒充
用户管理身份；重启后节点归属与记录保留。
有意义的局部验证：在真实 Store/Service 边界走创建→登记→续租→轮换/停用，确认旧 token 不能
再修改当前租约；不做单独字段/枚举快照。复用现有 Store fixture 和认证支持。
验证：`just test -p talos-store`、`just test -p talos-service`，对改动的 listener 所属包执行 scoped check/test。

阶段出口（2026-09-21）：单表 migration、Store/Service、TLS gRPC 控制与管理入口已接通；
`just test -p talos-store -p talos-service` 303/303、`just test -p talos-base` 28/28、
相关 clippy 与 fmt-check 通过。Store 测试验证 DB 重开与旧 token 不能覆盖新租约；RPC adapter
验证用户归属、节点 token 不能冒充用户与轮换撤销。对照设计修正同秒续租幂等和阶段 1 空部署登记；
TLS 跨进程 smoke 留在 1.3。

## 1.3 独立注册 Runner 与租约生命周期

状态：Complete。依赖：1.2 已通过。

工作：

- 创建 agent-client 并完成 Register/RenewLease/Heartbeat/Unregister；Runner 保存当前 Registration，
  按单调截止时间续租、心跳和失效。它不依赖 RemoteAgentClient 或 Harness RPC adapter。
- 独立节点入口最终由阶段 4 收敛为 `talos-worker`；该进程只运行 Worker 与注册生命周期，
  不启动控制面业务数据库。
- 实现注册响应丢失后的重新登记、控制面重启后 registration_required 的处理；复用 node_id，
  不因重启新增行。重新登记只更新节点身份和租约，不触及 Session/Turn。
- 最终运行时只注册已构造的真实 Harness，deployments 必须非空；开发期空部署入口已在
  [阶段 7](07-node-lifecycle.md) 收敛。不得填入猜测的 Provider 能力或占位 Harness。
- 失鲜节点停止新分派；租约到期/明确拒绝时停止节点 admission 并清理已组合的本地资源。
  阶段 2 将真实 Harness 接到这条原有进程生命周期中，不新增远程恢复管理器。
- 同一 token 正常续租不打断工作；撤销传播窗口明确不超过剩余租约。旧 token 的异步回复
  不覆盖新 Registration，旧句柄回调不摘掉新句柄。

成功标准：独立进程真实执行注册→心跳→续租→注销；控制面重启后以原身份登记；失效实际停止可用性。
验证场景：可控时钟的过期/续租边界测试，加一个独立进程生命周期 smoke；不靠固定 sleep 断言。
验证：`just test -p agent-client`，以及 `just check -p talos-server`；smoke 复用 E2E 启停 helper，
只验证注册链路，不把它称作远程 Agent 执行验收。

阶段出口（2026-09-21）：`agent-client` 登记/续租/心跳/注销与单调时间截止生效，
节点命令独立运行。`talos-server` 集成测试启动真实节点子进程，验证 TLS 登记、控制面
重启后重登记与信号注销（1/1）。Runner 不填假 deployments、健康状态仍 unknown、执行
available=false。核对设计后修正：节点监听 TLS 文件只在阶段 2 构造 Harness 时要求；
本地租约到期重新 Register；Client 延迟连接使控制面初始不可达也进入 Runner 重试。
`just check -p talos-coordinator -p talos-service -p talos-base -p talos-server` 已通过，
综合测试 448/448 通过。

## 1.4 embedded 接入与阶段出口

状态：Complete。依赖：1.3。

工作：

- composition 维护稳定 `embedded` 节点，现有 Agent definition 构造 Harness 后通过
  AgentNodeService → CoordinatorImpl.register_agent 安装。embedded 网络字段为空。
- Registry 保持 Coordinator 私有，以 node_id/agent_id 定位，保留逻辑 Agent 存在性查询；
  阶段 1 的原有 embedded 调用统一映射到 embedded，阶段 2 再接入 Session 节点绑定。
- 原 embedded Service 权限保持；不改变现有用户和 anonymous 本地模式。远程电脑/Pod
  按用户独占，只校验节点归属，不新增 Agent/Workspace 授权清单。
- 节点登记/健康与执行 available 分开推导：尚无 RemoteAgentClient 时 remote available=false，
  已有 embedded 真正可执行；不安装测试 Harness 充数、不新增阶段功能开关。

成功标准：注册管理可以独立验收，原有 embedded Session/Turn 不回归；没有先依赖阶段 2 才能通过的检查。
验证：`just test -p talos-coordinator`，运行最小既有 embedded Session/Turn 场景；
检查阶段 1 的管理结果及注册日志，不输出秘密。

阶段出口（2026-09-21）：Coordinator 私有 Registry 使用 (node_id, agent_id) 键；
原 embedded Agent 仍由既有 composition 构造，随后持久化稳定 embedded 目录行。
旧句柄的异步撤销通过指针身份匹配，不能删除替换句柄。对照 Spec 修正 Service 不持有
Harness/Registry，管理端管理员可读取 embedded 节点。综合 scoped 回归 448/448，
后续 Service 测试 208/208；相关 clippy、`just fmt-check`、`check:repo` 通过。

阶段 1 完成记录（实施后填写）：

| 证据 | 当前结果 |
| --- | --- |
| 用户管理自己的节点、拒绝跨用户管理、管理员代管 | Service/RPC 场景测试通过；节点 token 不能冒充用户 token |
| 独立节点注册、续租、心跳、注销及控制面重启 | 独立进程 TLS smoke 通过；原 node_id 重登记、SIGINT 注销 |
| embedded 原有 Session/Turn | 跨包回归 448/448，后续 Service 变更 208/208；稳定 embedded 行重复登记不复制 |
| 对照设计与修正 | Service 只持久化 embedded 目录，Coordinator 继续拥有私有 Registry；远程节点不假装可执行；无新执行账本 |
| 前置阻塞 / 下一步 | 无；进入阶段 2.1 原 Harness/RPC 类型和实例失效边界核对 |
