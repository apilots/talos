# Agent 节点注册与统一 Harness RPC

Status: implemented — 基线验收见[阶段 8](plans/08-execution-boundaries.md)；本轮五项消融已完成，定向回归及真实 E2E 8/8 验收见[阶段 9](plans/09-review-ablation.md)。

## 1. 核心设计

一个控制面管理多个执行节点。上层始终调用同一个 `AgentHarness`：内嵌模式直接调用
进程内实现，远程模式调用 `agent-client` 的实现。两者提供相同的 Params、事件、Result
和错误契约。Harness 负责执行与本地 Workspace，Service 负责 Session/Turn 和业务持久化。

远程 Session 的 Workspace 文件全部由对应 Worker 读写，包括 transcript、summary、
continuation、`MEMORY.md` 和 Dream Memory。自动反思和维护流程也必须遵守此归属；
控制面保存节点 metadata、Session/Turn 和事件等业务记录。内嵌 Worker 的文件仍属于
其执行 Workspace，不能作为远程 Session 的替代存储位置。

**兼容原则：现有 Harness 是唯一业务契约。RPC 不改变已有功能、业务对象或返回语义。**
PB 只是原类型的传输投影，额外信息限于鉴权和必要的标准 RPC 封装。agent-client 内的
RemoteAgentClient 封装执行调用，AgentNodeClient（node client）封装注册/租约/心跳。
协调层统一使用 AgentHarness，不判断传输模式；执行事件实时反馈，不等待任务结束后批量返回。

```text
talos-server -> Service / Coordinator -> AgentHarness
                                      ├─ talos-worker library -> Provider
                                      └─ RemoteAgentClient -> gRPC
                                                              -> talos-worker library -> Provider

节点 -> 注册 / 续租 / 心跳 RPC -> AgentNodeService -> AgentNodeStore
                                           └─ 更新现有 Registry 的可调用句柄
```

`talos-worker` 是唯一执行运行时组装边界，负责 Provider、Harness、工具、MCP 和节点本地
Workspace 能力。它只通过现有 typed ports 使用上层资源，不调用 Coordinator、NodeStore、
SessionStore，也不打开控制面数据库。
`talos-server` 在内嵌模式注入本地实现并直接安装 Harness；独立 `talos-worker` binary 注入
RPC Client 实现，启动 Harness gRPC listener 后注册到控制面。两种形态不产生两套业务实现。

远程节点是一名用户独占的电脑或 Pod；一个用户可以有多个节点，一个节点只属于一个用户。
注册只管理节点身份、所属用户、地址、租约和在线情况，不参与 Turn 结算。
首版要求控制面可通过 TLS 访问节点 gRPC 地址；节点也可访问控制面注册地址。
部署通过集群网络或已有代理满足可达性，Harness 协议直接使用标准 gRPC 方法和流。

## 2. 分层与包

| 包 / 组件 | 职责 |
| --- | --- |
| `agent-pb` | 独立 lib：PB 文件、自动生成的对象和 gRPC Client/Server 接口；无业务逻辑 |
| `agent-protocol` | 共享 Harness trait、执行 Params/句柄、NodeAuth 和 PB 转换；引用既有规范消息/结果 |
| `agent-client` | `RemoteAgentClient: AgentHarness`、节点注册 Client；封装认证、连接、流及错误转换 |
| `talos-agent` | 现有 Harness/Provider 实现；节点侧 RPC adapter 验证请求后调用同一 Harness |
| `talos-worker` library | 统一构造 Provider、Harness、工具、MCP 与节点本地 Workspace 运行时 |
| `talos-worker` binary | 独立执行 Host：为同一 Worker 增加 gRPC listener、注册、租约和心跳 |
| `talos-server` | 控制面；内嵌部署时注入本地 ports 并直接注册 Worker Harness |
| Coordinator / 现有 Registry | 按 Session 绑定选择节点，并获取 `Arc<dyn AgentHarness>` |
| Service / 现有 Store | 节点管理，以及原有 Session、Turn、事件、结果持久化 |

现有 `talos-agent` 的上游端口、执行 Params/句柄及能力描述迁入 `agent-protocol`，原位置
不留重复定义。`talos-agent-api` 中已有的消息、事件、产品结果和 Timeline/History DTO
保留唯一 owner，由 protocol 引用。Provider SPI、reducer、执行控制仍在实现层。
PB 是领域对象的编码形式；本地调用不经过 PB，远程转换集中在 protocol 中。
Worker 核心不包含 embedded/remote 分支；差异由 Host 注入的本地实现或 `agent-client`
实现决定。远程网络状态由 Client、RPC stream 和租约生命周期管理，不进入业务状态表。

## 3. 节点注册与认证

用户创建自己的节点，或管理员代为创建；控制面自动生成 UUID v4 作为不可变 node_id，
记录 owner_user_id、注册凭据和 TLS endpoint。部署时把返回的 UUID 写入 Worker 的
`TALOS_HOME/conf/talos.yaml` 的 `agent_node.runner.node_id`；Worker 读取已有配置，每次启动/注册均携带它。显示名和地址不是身份；改名、改地址、凭据轮换不新建节点行。
节点启动后主动 Register；
控制面按稳定 node_id 更新原记录，返回短期 token 和有效期。控制面在 Register 处理期间向现有 Registry 发布 RemoteAgentClient，再返回租约；
节点安装租约并报告健康后才允许分派。控制面 listener 必须一次组装完整的注册/管理、
publisher 和资源 RPC；available 同时要求有效租约、健康心跳和已发布的调用句柄。
重复启动使用原 node_id，不新增节点行。

所有节点控制 RPC 和 Harness RPC 使用具体方法名命名请求与参数，共享 NodeAuth：

```proto
message NodeAuth { string node_id = 1; string token = 2; }
message RegisterRequest { NodeAuth auth = 1; RegisterParams params = 2; }
message HeartbeatRequest { NodeAuth auth = 1; HeartbeatParams params = 2; }
message StartRequest { NodeAuth auth = 1; StartParams params = 2; }
```

Register 使用长期注册凭据；其他节点请求和控制面调用 Harness 使用当前短期 token。
RenewLease 验证当前 token 后延长期限，token 保持不变。Register 更换 token。
控制面数据库只保存 hash；节点与 RemoteAgentClient 在受保护内存中持有当前 token。
重启后的控制面等待节点重新注册以重建 Client，不从数据库还原秘密。

默认租约 90 秒、30 秒续租、10 秒心跳，30 秒未收到心跳暂停新分派。租约到期由节点
本地计时器停止接收调用并清理执行；控制面撤下在线句柄。到期或离线不代表业务执行完成。
同一 token 续租只更新 deadline；重新注册替换 token 并撤销旧调用。Server 退出只关闭本进程拥有的内嵌 Harness，独立 Worker 保持运行。详细认证、管理接口见[节点 API](contracts/node-api.md)。

## 4. 最小持久化

| 对象 | 内容 |
| --- | --- |
| 新增 `agent_nodes` | 稳定身份、所属用户、部署方式、endpoint、凭据 hash、当前租约、启停及最新健康/心跳 |
| 原 `sessions.execution_node_id` | 唯一新增 Session 字段；让后续调用回到持有本地文件的同一节点 |
| 原 Session/Turn/事件/continuation 表 | 保持既有业务职责，不增加远程调用状态字段 |

Registry、Client、RPC 流、未消费 permit 均为运行时对象。内嵌节点使用稳定 node_id
`embedded` 和相同的部署描述、路由方式，由可信进程内入口安装 Harness；不申请网络 token。
内嵌用户/Workspace 权限继续由原 Service 决定。远程节点只检查调用用户与所属用户一致，
不维护 Agent/Workspace 权限清单。个人电脑按用户完整使用环境，Pod 的运行限制由沙箱策略
负责；RPC 不新增文件访问策略，原 Harness 的 permission_mode 和工具行为保持。业务持久化继续使用已有
SessionStore / AgentExecutionStore。表和事务接口见[SQL](contracts/agent_nodes.sql)及[Store API](contracts/store-api.md)。

`MEMORY.md` 与 Dream Memory Markdown 属于节点 Workspace；Memory SQLite 仅是可从
这些文件重建的本地搜索索引。本功能不新增控制面到节点的用户 Memory 管理代理；
以后若需要浏览节点文件，需显式选择节点，不能在一个用户有多个节点时隐式猜测。
Session/Turn 事实仍由控制面数据库持久化。

## 5. 调用与结果

Harness 保留原七个方法：start、submit_input、interrupt、compact_session、clear_session、
close_session、shutdown，保持原执行语义。Workspace 维护补读/更新 Memory 和形成 Dream 三个端口，复用原类型及算法；不增加执行状态。详见[Memory 接口](contracts/memory-api.md)。输入、进度、结算及产品结果继续使用既有领域类型。

Start/Compact 的 gRPC 流先返回 admission；Client 重建原 `AgentRunExecution`。
Service 按原顺序持久化 admission，再 consume permit。Client 随后在该调用流内发送
commit，节点执行真实 permit.consume 并回传其结果，再推送进度与最终 settlement。
SubmitInput 使用同一规则。Client 持续消费流并即时投递原 receiver，协调层/Service 在
任务完成前即可消费事件；Service 保持原有落库后向产品客户端发布的顺序。
流本身标识这次调用，不另设跨调用的请求总线或执行编号。

Unary 方法直接返回原结果。gRPC deadline、取消和流控由 Client/adapter 处理；Service
继续消费现有 receiver 并落库。完整输入、Result、PB 定义见[Harness API](contracts/harness-api.md)。

## 6. 必要的失败边界

网络失败转换为统一 TalosError。超时、断流只说明调用结果未知，Client 不自动重试有
副作用的方法。节点尽力取消对应执行，清理结果通过原 interrupt/settlement 契约表达。
上层只根据已确认的业务结果更新终态；重新注册、租约过期和 Registry 缺席都不是终态证据。

Client 将传输不可达转换为已有 ServiceUnavailable/Timeout，绝不能伪造 Harness 的
ResourceNotFound 或 Completed；真实 Harness 业务结果原样转换。上层沿用统一错误契约，
不增加 remote/local 执行分支。原 embedded 恢复、WaitingForInput 续接与取消功能保持。
跨进程自动恢复不在本期范围；不能以网络适配为由改写既有恢复语义或新增业务状态机。

默认 Workspace 在执行 Worker 按配置拼接：相对 root_prefix 使用
`Worker TALOS_HOME / root_prefix / user_id`，绝对 root_prefix 使用 `root_prefix / user_id`；
默认值为 `Worker TALOS_HOME/workspace-talos/user_id`。控制面仅传逻辑 user/workspace，
不因控制面旧目录存在而设置 working_directory。显式目录/项目保留原 Context 的
路径语义，RPC 不额外强制限制在 Workspace 内。Session 保持节点
亲和性；有活动 Turn 或私有 continuation 时不更换节点。独立节点的依赖组合边界和完整
主链路见[执行流程](contracts/execution-flow.md)，未接通必要依赖的节点不能报告健康。

## 7. 契约附件

- [Harness API 与结果](contracts/harness-api.md)
- [执行节点 Memory 文件接口](contracts/memory-api.md)
- [注册、认证与节点管理](contracts/node-api.md)
- [节点 Store 与 Session 绑定](contracts/store-api.md)
- [端到端主链路](contracts/execution-flow.md)
- [独立 Kernel 的控制面依赖](contracts/control-plane-dependencies.md)
- [auth.proto](../../talos-rs/crates/orchestration/agent-pb/proto/auth.proto)、[node.proto](../../talos-rs/crates/orchestration/agent-pb/proto/node.proto)、[harness.proto](../../talos-rs/crates/orchestration/agent-pb/proto/harness.proto)、[types.proto](../../talos-rs/crates/orchestration/agent-pb/proto/types.proto)

Protobuf 文件位于独立 `agent-pb` crate，作为唯一 wire 定义；内嵌与独立执行均由
`talos-worker` library 组装，并已通过真实跨进程执行回归。
