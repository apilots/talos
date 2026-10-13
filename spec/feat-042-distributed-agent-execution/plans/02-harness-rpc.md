# 阶段 2：按现有 Harness 实现远程调用

状态：**Complete**。独立审查发现的问题已在[阶段 6](06-runtime-repairs.md)修复，真实七场景 E2E 与定向回归通过；下文保留阶段实施证据。
硬约束：原 Harness 是业务契约来源。新增 RPC 不改变已有方法、参数、结果、事件、
permit、取消、continuation 或 Session 生命周期语义，不破坏 embedded 功能。

## 2.1 共享端口与 PB 对照

状态：Complete。依赖：阶段 1，已通过。

工作：

- 逐项对照现有 port.rs/run.rs/context.rs/capability.rs 和规范 agent-api 类型，建立
  “原 Rust 类型/方法 → PB → Client 返回值”映射；该映射留在 [Harness API](../contracts/harness-api.md)。
- 只移动 Client 必须依赖的上游端口、执行 Params、permit/receiver 和 checkpoint 数据结构。
  reducer 算法及 Provider SPI 保留原 owner；agent-api 规范消息/事件/产品结果直接引用。
- 按原字段、enum、可空语义和错误分类定义 PB 投影；复用已有语义，不新增远程业务状态或
  第二份手写领域模型。PB 生成 Client/Server 接口；网络请求外层携带 NodeAuth。
- 使用标准 gRPC 的 method、deadline、cancel、stream 和 status；原句柄无法序列化的部分
  由 Client/adapter 在内存中适配。流内 admission/commit 仅实现原 permit，不新增上层业务动作。
- 类型迁移与调用方 import 调整在同一可编译增量完成，避免循环依赖和长期兼容别名。
- 核对一个已知边界：节点重启后旧 Turn 的 interrupt 可能从原 Harness 得到 ResourceNotFound，
  现有 Service 会将它视作清理确认；这不能仅靠网络错误映射解决。必须先在 Client/adapter
  边界明确调用实例失效与真实业务缺席的区分，并验证旧 continuation 不被误重放。
  已决定用 Client 当前实例内存中确实接纳的 Turn 身份作为确认边界：新实例对旧 Turn
  返回现有 RunCleanupUnconfirmed，绝不把 ResourceNotFound 当作清理证明；不加 DB 账本。
  行为验证必须随 2.2 的真实 Client/adapter 完成；未通过时不能进入 2.3。

成功标准：七个方法都有无损映射；上述实例失效边界明确；迁移前后的 embedded 行为相同；
业务类型仍只有一个 owner。涉及现有契约无法表达的差异时记录具体阻塞，回到方案评审，
不自行扩展业务协议。
验证：`just test -p talos-agent`、`just test -p talos-agent-api`；
`just check -p agent-protocol`、`just check -p agent-pb`。
PB 编译只能证明类型合法；用现有真实消息/事件 fixture 验证代表性富输入、等待与失败数据的往返，
不为每个字段机械新增测试或把序列化快照当作功能验收。

阶段出口（2026-09-21）：唯一 Harness 端口、Context、执行参数、permit、receiver、checkpoint
已从 talos-agent 移到 agent-protocol；原 crate 只 re-export，不保留第二份业务定义。
七方法的原字段与现有规范事件、富输入、Waiting/Finished、内层失败和 interrupt outcome
均有 typed PB 投影，未知 enum 拒绝。代表性富消息、64 位 JSON 整数、UserAction Waiting、
失败结算和节点本地 Workspace 解析场景通过。`just test -p agent-protocol` 6/6，
`just test -p agent-protocol -p talos-agent -p talos-agent-api` 45/45（搬迁后），
`just clippy -p agent-protocol -p talos-agent -p talos-coordinator -p agent-client` 通过。
对照设计修正：非 UTF-8 PathBuf 在 PB 边界明确失败，不能 lossy 转换；实例更替的
ResourceNotFound 判断留在 Client 内存，2.2 必须用真实 RPC 验证后才能路由业务。

## 2.2 标准 RPC 与 Client/adapter

状态：Complete。依赖：2.1，已通过。

工作：

- RemoteAgentClient 实现原 AgentHarness；节点 adapter 调用原本地 Harness。
  Start/Compact/Input 使用逐调用流，Interrupt/Clear/Close/Shutdown 使用标准 unary。
- 在原 admission 持久化后消费真实 permit。Start/Compact 返回原 event/settlement receiver；
  三个流式方法的 accepted/Failure 都返回真实 permit.consume 的 Ok/Err，不弱化为传输入队。
  SubmitInput 不新增 settlement。Client 收到事件即实时反馈协调层，不累计到任务结束。
- 用阶段 1 已签发的租约验证每个请求；续租/撤销由原 Runner 提供，adapter 不管理节点数据库。
- 保持事件顺序、完整结算、错误分类和本地清理语义；正常 half-close 不误当取消。
  网络失败由 Client 转为现有传输错误，不能冒充 Completed；真实 Harness 结果原样映射。
- shutdown 成功撤下相应实例；重建走现有工厂。Workspace 路径转换仅做节点本地解析，
  不删减原 Context 的模型、权限、项目和目录语义。

成功标准：所有七个方法都可经真实本地 TLS/gRPC 调用；Client 暴露的接口与原 Harness 相同。
最小行为验证：复用原 Harness fixture，分别 direct 和 RPC 调用同一实现；验证 admission
前无副作用、commit 后有且只有一次；动态输入成功/失败按原返回契约完成；取消和结算一致。
仅在网络边界补断流/half-close/错误投影的缺口，不复制完整业务测试套件。
验证：`just test -p agent-client`、`just test -p talos-agent`，对新增 adapter 所属包运行 scoped check/clippy。

阶段出口（2026-09-21）：RemoteAgentClient 实现同一 AgentHarness；节点 adapter 调原
Harness 七个方法。真实本地 TLS/gRPC 场景复用原 FakeProvider，验证 direct/RPC admission、
commit 前无执行、动态输入成功与拒绝、实时事件早于结算、interrupt/compact/clear/close/
shutdown，以及失效租约不伪造 Completed；`just test -p talos-agent` 42/42，
`just test -p talos-service` 208/208，相关 clippy 通过。控制面仅在当前注册内存保留
token 原文及部署，数据库只存 hash，重启后等待节点重登记。对照设计修正：Client 构造
显式接收 CA，adapter 构造显式接收节点 Config；新 Client 不能对旧 Turn 缺席报告清理
已确认，真实 RPC 用例通过。Runner 与节点 TLS listener 的生产组合仍按 2.4 接入，
本小阶段的直接 TLS 测试不作为独立 Kernel 节点的验收。

## 2.3 Session 路由与原 Service 衔接

状态：Complete。依赖：2.2，已通过。

工作：

- 单独迁移原 sessions.execution_node_id，旧 Session 绑定原 embedded；新增 Session 优先
  选择属于该用户且可执行的远程节点，多个远程节点按 node_id 稳定排序，没有时回退
  embedded。初次绑定之后不隐式迁移 Workspace。
- Coordinator 保留逻辑 Agent 路由；ensure_agent/resolve_agent 使用逻辑存在性查询，
  实际执行内部复用 SessionStore.get_session_record 读取绑定后查原 Registry，不向上层
  调用签名添加 node_id 或网络参数，不新增 resolver/Store。注册/撤销仅通过 CoordinatorImpl owner。
- 检查所有调用方：普通 Session、subagent、动态输入、interrupt、compact、clear、close、
  全局 shutdown。NodeAuth 和 gRPC 类型不进入这些业务调用参数。
- 原 Service 的 admission/event/waiting/terminal 事务与返回对象保持。
  网络细节由 RemoteAgentClient/AgentNodeClient 内聚处理，Service 不新增 remote/local 分支。
  验证传输错误不会伪装成 ResourceNotFound→已完成；原 embedded 恢复和 Waiting 行为保持。
- Waiting 取消仍基于持久业务事实和原 Session 互斥，不能依赖存活的 RPC request_id。

成功标准：同一用户流程经 direct/remote 使用同一 Service/Store；后续 Turn 回到原节点，
已有 embedded 用户、Session、输入和恢复功能保持；未知网络结果不制造业务成功或重复副作用。
验证：扩展既有 service/turn、user_action、session、subagent 和 Coordinator 场景，
优先使用现有 fixture；`just test -p talos-service`、`just test -p talos-coordinator`、`just test -p talos-store`。

阶段出口（2026-09-21）：Session 节点绑定与 Session 一起创建，旧记录固定 embedded，
子 Session 沿父节点；Coordinator 复用现有 SessionStore 和 NodeStore 检查归属、租约、
30 秒健康心跳及 Registry 句柄，缺失远程节点返回 ServiceUnavailable，不误判业务清理
完成，也不隐式换节点。上层原 Harness/Service 方法不增加网络参数。修正设计附件中
可空字段和二次 bind API 的过时描述；禁用节点租约写入改为取尽 SQLite RETURNING，
防止返回已撤销而紧接读取仍可用。`just test -p talos-coordinator -p talos-service -p talos-store`
309/309 通过；base/gateway 定向回归 302/302，`pnpm --dir talos-app check:repo` 通过。
生产跨进程链路须在 2.4 才能验收，不将模拟 Harness 路由测试充作独立节点 E2E。

## 2.4 真实 Kernel 依赖与生产组合

状态：Complete。依赖：2.3，已通过。先补原计划遗漏的控制面资源入口，
完整可运行链路不得后移到 E2E。

首版 Provider 为已有 Kernel。以真实 [KernelAgentProviderParams](../../../talos-rs/crates/orchestration/agent-provider-kernel/src/provider.rs)
及 [application_runtime.rs](../../../talos-rs/crates/app/talos-base/src/application_runtime.rs) 为清单：

| 依赖 | 实施位置和要求 |
| --- | --- |
| 模型 / ToolRegistry / skills | 节点按原配置组合真实依赖，保持现有能力与权限语义 |
| transcript / summary / continuation | 原 KernelSessionArtifactStore，使用节点本地 Workspace |
| SessionHistoryReader | 原 Session/History Service 提供授权读取；节点 Client 实现同一 reader |
| AttachmentReader | 原 File Service 的授权读取；节点正确解析原 file_id 和富输入 |
| memory progress / DreamMemory / UserWorkspaceMemory | Session memory cursor 留控制面；Markdown、bundle 与可重建索引在节点本地 Workspace，执行时直调节点本地原实现 |
| subagent 和其余控制面工具 | 原 Service 授权入口；保持父子 Session 绑定与工具结果模型 |

先按以下顺序交付，必要入口见[控制面依赖清单](../contracts/control-plane-dependencies.md)；
任何小阶段缺失均不能发布远程 `available`：

1. **2.4a 控制面依赖契约**：逐项核对上表对应既有 trait/Service owner、NodeAuth、
   Session 节点绑定与用户归属；为确需跨节点的 history、attachment、memory cursor、
   ToolImage 和 subagent 定义专用 typed gRPC 方法与
   PB 对象。路径由控制面或节点自身可信配置解析，不接受节点传来的控制面绝对路径。
   同时补控制面对节点 TLS 证书的信任配置和失效/重新注册的句柄发布时序。
   验收：每项生产依赖都有明确本地/远程实现 owner、请求字段、授权条件及失败映射。
2. **2.4b 资源 Client/Server**：在原 Service owner 提供授权处理，节点侧 Client
   实现既有 trait；本地 Workspace 文件仍由节点处理。验证跨用户、跨节点 Session、
   租约撤销、文件顺序/字节及 memory CAS，禁止 Noop 或节点直连控制面数据库。
3. **2.4c 生产组合与发布**：节点构造原 Kernel Provider、工具和 TLS Harness listener；
   Runner 注册真实 deployments，控制面按当前租约发布/撤下 RemoteAgentClient。
   用真实 Harness 文件副作用和跨进程生命周期证明组合可运行；完整用户 Turn、后续历史、
   实时事件与控制面持久化在阶段 3 经原 HTTP/SDK 入口一次验收，避免重复搭建 E2E。

| 小阶段 | 状态 | 独立验收与检查 |
| --- | --- | --- |
| 2.4a 依赖契约 | Complete；history、attachment、memory cursor、subagent、ToolImage typed PB 已生成；Memory 文件留节点 | 原 trait 与 RPC 字段已对照；附件消除原 trait 不存在的 Session 参数；图片按原逐张提交语义流式传输 |
| 2.4b Client/Server | Complete；资源 server/client 复用原 owner 和 trait | 真实 TLS 验证 history、附件字节/顺序、memory CAS/冲突、subagent、ToolImage；节点租约确定用户，Session 调用另验归属；scoped tests/clippy 通过 |
| 2.4c 组合与发布 | Complete；独立节点使用真实 Kernel/工具/本地 Workspace 与 Memory | 跨进程注册真实 deployment、健康后可路由、控制面重启重登记、SIGINT 注销撤下；RemoteAgentClient 对真实 Harness 产生并验证节点文件副作用 |

工作：

- 逐项记录既有可调用入口、传输 Client、身份/Workspace 权限映射；缺失入口在原 owner 内
  补最小契约再实现。节点 token 绑定 owner_user_id，访问既有 Service 时使用所属用户身份，不能伪造成管理员；
  不新增节点级 ACL，不扩成通用网关。
- 接通上述生产依赖和节点 TLS listener；最终由独立 `talos-worker` 构造真实 Harness；
  将阶段 1 租约生命周期与真实 Harness admission/shutdown 衔接。
- 完成后才发布 RemoteAgentClient、使对应节点执行 available；声明的每种能力均要可用。
  不允许空 history、Noop memory、测试 Provider 或内存数据库替代远程 Kernel 交付。
- 补充 node 启动、配置和管理员使用说明，更新受影响配置 schema/生成物。

成功标准：生产独立节点能够构造真实 Kernel 及全部必要依赖，注册真实能力，控制面仅在
当前租约和健康心跳下发布统一 Harness；注销和重新注册正确撤换实例。真实 Harness RPC
能够操作节点 Workspace，控制面资源 RPC 能够访问原授权 owner。完整用户链路由阶段 3 验收。
验证：按受影响 owner 执行 scoped tests/check；运行独立节点生产组合 smoke。缺失任何
必要依赖则阶段 2 仍未完成。

阶段 2 完成记录（实施后填写）：

| 证据 | 当前结果 |
| --- | --- |
| 七方法原契约与 RPC 映射 / embedded 回归 | 2.1/2.2 scoped 测试通过；阶段 3 仍需完整回归 |
| admission 顺序、事件、输入、结算 | 真实 TLS Harness fixture 已通过；完整用户入口留阶段 3 |
| Session/subagent 节点路由与 unknown 处理 | 2.3 定向回归通过；资源 RPC 的归属与租约场景通过 |
| 真实 Kernel 依赖和独立节点 smoke | 通过：真实 deployment 注册/发布、节点文件副作用、控制面重启重登记、SIGINT 注销撤下 |
| 前置阻塞 / 下一步 | 阶段 2 Complete；进入阶段 3，从原用户入口验收双 Turn、实时反馈与持久化 |
