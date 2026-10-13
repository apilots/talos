# 同一条执行链路

[主方案](../design.md)。下表中除 Harness 获取方式外，上层调用和持久化顺序一致。

## 1. 路由和执行

| 步骤 | 内嵌 | 远程 |
| --- | --- | --- |
| 登记 | talos-server 从 talos-worker library 安装本地 Harness | 独立 talos-worker Register，安装租约并报告健康后发布 RemoteAgentClient |
| 定位 | Session.execution_node_id + agent_id 查询现有 Registry | 相同查询返回 RemoteAgentClient |
| start | 调用 Harness.start，得到 RunExecution | Client 调用 Start RPC；首个 admission 重建相同 RunExecution |
| admission | Service 在原事务中写入 | 完全相同 |
| consume | 释放本地 permit，返回原结果 | Client 在该 RPC 流内发 commit，等待真实 consume 的同一结果 |
| progress | 原 receiver 实时返回事件 | Client 收到流事件即投递同种 receiver，实时反馈协调层 |
| settlement | 原 Waiting/Finished 结算 | Client 从响应流重建相同结算 |
| 持久化/产品输出 | 原 Service 校验、落库、发布 | 完全相同 |

Coordinator 仍是 Service 的唯一执行入口，复用现有业务参数；Clear/Close增加原SessionHistoryCursor期望参数，不要求业务调用者额外传
node_id、token 或传输类型。Coordinator 内部按参数原有 session_id 从已注入的现有
SessionStore.get_session_record 读取节点绑定，再查询 Registry；这里只读路由字段，
不写业务事实、不新增 resolver/Store。Server shutdown 只关闭本进程拥有的 embedded Harness；远程 Worker 生命周期独立。

逻辑 Agent 的 resolve_agent/ensure_agent 保持原用途，通过 Registry.contains_agent
检查该 Agent 是否有注册实现，不把节点 ID 当作 Agent ID。Session 创建时，现有 Service
绑定逻辑优先选择属于该用户且可执行的远程节点，多个远程节点按 node_id 字典序稳定
选择；没有可用远程节点时使用 embedded，并将结果持久化。绑定后不自动换节点。
既有 Session 绑定 embedded；子 Session 优先沿父节点绑定并检查用户归属及目标 Agent 是否可用。
这些是注册路由规则，执行业务代码不判断本地/远程。interrupt/compact/clear/close 使用
同一 Session 绑定，不另行选择节点。

```rust
// Execution uses the existing domain model; maintenance checks the same Worker 历史游标.
async fn start(&self, params: AgentRunParams) -> TalosResult<AgentRunExecution>;
async fn submit_input(&self, params: AgentInputParams) -> TalosResult<AgentAdmission>;
async fn interrupt(&self, params: AgentInterruptParams) -> TalosResult<AgentInterruptOutcome>;
async fn compact_session(&self, params: AgentContext) -> TalosResult<AgentRunExecution>;
async fn clear_session(&self, params: AgentSessionContext, expected_history_cursor: SessionHistoryCursor) -> TalosResult<()>;
async fn close_session(&self, params: AgentSessionContext, expected_history_cursor: SessionHistoryCursor) -> TalosResult<()>;
async fn shutdown(&self, timeout: Duration) -> TalosResult<()>;
```

依据[现有 admission 顺序](../../../talos-rs/crates/orchestration/service/src/turn/agent_execution/execution_task.rs)，
permit 适配只负责延迟执行。它不要求 Service 知道是本地释放还是远程 commit。
节点上下文鉴权在执行前完成；Client 不能绕过 Service 原有 Actor 权限和 Session 授权。

## 2. 其他方法

| 方法 | 统一处理 |
| --- | --- |
| submit_input | 使用同一 AgentInputParams；admission 先落库，consume 等待真实输入接收 |
| continuation | 显式业务续接仍调用 start，传原 checkpoint/continuation，回到同一节点 |
| interrupt | 按 agent_id + TurnRef 定位；返回原 Cancelled/Completed/CleanupUnconfirmed |
| compact_session | 复用 maintenance admission，后续仍为 progress/settlement |
| clear_session / close_session | 本机与远端经同SDK；原SessionRequest携带expected_history_cursor，Worker原Gate锁内核对后维护，返回原结果 |
| shutdown | 停止 Harness；成功后撤下原句柄，再启动须构造新实例 |

已提交 WaitingForInput 表示上一次执行已完成清理。取消该等待由 Service 持有原 Session
互斥后提交取消事实；不依赖存在运行中的 RPC。客户端无权从 RPC 缺席自行推断这个事实。

## 3. 失败处理与现有代码衔接

正常领域失败保留原结果；网络失败返回 ServiceUnavailable/Timeout 等现有 TalosError。
Client 不自动重放 start/input/维护调用；取消流只触发对应本地执行的尽力 interrupt。
既有进度序号及事务保证已确认事实的落库顺序，传输层不增加数据库确认协议。

兼容要求在 Client/adapter 边界落实：网络不可达必须返回 ServiceUnavailable/Timeout，
不能转换成 ResourceNotFound 或 Completed；实际远端 Harness 的业务结果逐项保持。
协调层拿到同一 AgentHarness，Service 不按 remote/local 分支处理执行或改写持久化。

对现有 [timeline_writer.rs](../../../talos-rs/crates/orchestration/service/src/turn/agent_execution/timeline_writer.rs)、
[interrupt.rs](../../../talos-rs/crates/orchestration/service/src/turn/service/interrupt.rs) 和
[user_action_resume.rs](../../../talos-rs/crates/orchestration/service/src/turn/user_action_resume.rs)
保留原业务路径，并使用可靠 settlement 的清理证据处理终态提交失败：传输错误不能误入业务已完成分支；已有本地恢复、Waiting 续接和取消继续通过。
不以新增网络能力为由重写这些业务流程。跨进程未知结果的自动恢复不是本期交付。

Client 只对当前实例确实接纳的 Turn 使用原 Harness 的 ResourceNotFound 清理语义。
新 Client 对旧 Turn 的缺席返回现有 RunCleanupUnconfirmed；节点重启、租约失效或
Registry 缺席不构成完成证据。此边界在 Client 内存中处理，不新增执行数据库记录。

## 4. 独立节点的组合边界

RPC adapter 包装的是 `talos-worker` library 构造的同一个可运行 Harness。内嵌 Host 注入
本地 Service ports，独立 Host 注入 `agent-client` 的 RPC ports；Provider、模型、工具与
Workspace 配置只在 Worker library 组装一次。gRPC 注册不参与这些业务依赖的构造。

| 现有依赖 | 独立节点中的责任 |
| --- | --- |
| LlmProviderRouter / ToolRegistry / SkillRootsResolver | 由 Worker 使用 Host 配置组合；Provider 仍管理执行引擎 |
| KernelSessionArtifactStore | 使用节点 Workspace 内的 transcript/summary/continuation 文件 |
| SessionHistoryReader / AttachmentReader | 保留原接口，由相应 Service 的授权 Client 实现，不让 Harness 读取控制面数据库 |
| KernelMemoryProgressStore | Session cursor 留在控制面，节点通过授权 Client 实现原端口 |
| DreamMemoryFormationStore / UserWorkspaceMemoryReader | 原 Markdown、bundle 和可重建索引放在执行节点 Workspace，节点直调原实现 |
| subagent 等控制面工具 | 经现有 Service 接口调用，节点注册不承担其业务职责 |

接口实际依赖见[KernelAgentProviderParams](../../../talos-rs/crates/orchestration/agent-provider-kernel/src/provider.rs)。
本方案首版目标是现有 Kernel Provider。上表所需远程适配归对应 Service owner，在 Harness
RPC 交付阶段完成授权入口、Client 和生产组合；不把 agent-protocol 扩展成通用服务网关。
现有入口不足时先在该 owner 定义必要的调用/授权映射，再实现适配。相关能力未接通时
不能对外宣称可用，也不能用空 history、Noop memory 或测试 Provider 代替真实交付。
自动 Memory reflection 和 Dream 维护通过 Session 绑定的 Harness 读写目标节点文件，
不使用控制面本地 Memory store。控制面保留维护调度、Session 空闲互斥、模型用量记录及
处理游标；Dream 继续复用原 Kernel 的 canonical history 维护算法（不读取本地 summary）。
接口见[Memory 文件端口](memory-api.md)，实际验证见[阶段 6](../plans/06-runtime-repairs.md)。

有效 Waiting/Finished settlement 只在原 Harness 已确认清理后生成。Service 在已收到有效
settlement 后遇到数据库终态提交失败，直接复用该停止事实走原失败结算；不再要求已结束的
RemoteAgentClient 重新证明清理，不维护完成账本。尚未取得可靠 settlement 的传输失败
仍按原 interrupt 结果处理，未知结果保持未确认，不能从节点离线推断成功。
Dream 的配置、日期和形成算法在 Worker 执行，控制面只调度并提供本次模型回调。
