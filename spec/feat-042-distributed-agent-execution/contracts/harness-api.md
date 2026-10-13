# 统一 Harness 接口与 RPC 映射

[主方案](../design.md)。本地和远程实现同一业务端口；只有 agent-client 与节点 RPC adapter
了解网络。以下对象来自现有代码，迁移定义位置时保持原有语义。

## 1. 唯一的业务端口

```rust
// agent-protocol; no transport authentication in business params.
pub trait AgentHarness: Send + Sync {
    fn start(&self, p: AgentRunParams) -> AgentFuture<'_, AgentResult<AgentRunExecution>>;
    fn submit_input(&self, p: AgentInputParams) -> AgentFuture<'_, AgentResult<AgentAdmission>>;
    fn interrupt(&self, p: AgentInterruptParams) -> AgentFuture<'_, AgentResult<AgentInterruptOutcome>>;
    fn compact_session(&self, p: AgentContext) -> AgentFuture<'_, AgentResult<AgentRunExecution>>;
    fn clear_session(&self, p: AgentSessionContext, expected_history_cursor: SessionHistoryCursor) -> AgentFuture<'_, AgentResult<()>>;
    fn close_session(&self, p: AgentSessionContext, expected_history_cursor: SessionHistoryCursor) -> AgentFuture<'_, AgentResult<()>>;
    fn shutdown(&self, timeout: Duration) -> AgentFuture<'_, AgentResult<()>>;
}
```

| 参数 / 类型 | 原定义与字段 |
| --- | --- |
| AgentRunParams | [run.rs](../../../talos-rs/crates/orchestration/agent-protocol/src/run.rs)：context、input、checkpoint、continuation |
| AgentInputParams | 同文件：agent_id、identity、input |
| AgentInterruptParams | 同文件：agent_id、identity |
| AgentContext | [context.rs](../../../talos-rs/crates/orchestration/agent-protocol/src/context.rs)：session、turn_id、project_id、working_directory、environment、model、model_selection、permission_mode |
| AgentSessionContext | 同文件：agent_id、session_id、EffectiveWorkspace |
| SessionHistoryCursor | [session_execution.rs](../../../talos-rs/crates/common/types/src/session_execution.rs)：原history_generation、completed_turn_id、completed_event_sequence；Clear/Close复用该对象进行Worker维护准入，不新增业务上下文 |
| AgentRunCheckpoint | [checkpoint.rs](../../../talos-rs/crates/orchestration/agent-protocol/src/checkpoint.rs)：last_sequence、messages、metrics |
| AgentMessage / AgentEvent / AgentRunMetrics | [agent-api](../../../talos-rs/crates/orchestration/agent-api/src/lib.rs) 中现有规范模型；保留原定义 |
| AgentCapabilities | [capability.rs](../../../talos-rs/crates/orchestration/agent-protocol/src/capability.rs)；保留 Provider/Harness 内的能力判断，注册不重复广告无消费者的字段 |

领域 Context 保留已解析的 EffectiveWorkspace；PB SessionContext 只传 agent/session/user/
workspace 身份。节点按自己的 Workspace 配置解析 EffectiveWorkspace，再执行相同
Harness。working_directory/environment.directory 保留原 PathBuf 语义：相对路径在节点
Workspace 下解析，显式绝对路径指目标节点路径，不自动搬用控制面的默认根目录。RPC 不
新增 Workspace 围栏或路径权限；个人电脑与 Pod 的执行限制遵循原 Harness/沙箱策略。

## 2. 结果与句柄

```rust
pub struct AgentAdmission { pub events: Vec<AgentEvent>, pub permit: AgentAdmissionPermit }
pub struct AgentRunExecution {
    pub admission: AgentAdmission,
    pub events: AgentProgressEventReceiver,
    pub settlement: AgentRunSettlementReceiver,
}
pub enum AgentRunSettlement {
    WaitingForInput { event: AgentEvent, continuation: AgentContinuationRef, metrics: AgentRunMetrics },
    Finished {
        event: AgentEvent, messages: Vec<AgentMessage>, metrics: AgentRunMetrics,
        result: TalosResult<()>,
    },
}
pub enum AgentInterruptOutcome { Cancelled, Completed, CleanupUnconfirmed { task_count: u32 } }
impl AgentAdmissionPermit { pub async fn consume(self) -> AgentResult<()>; }
impl AgentProgressEventReceiver { pub async fn recv(&mut self) -> Option<AgentEvent>; }
impl AgentRunSettlementReceiver { pub async fn recv(self) -> AgentResult<AgentRunSettlement>; }

// Existing product results, assembled/persisted by the upper layer.
pub struct AgentRunOutput {
    pub messages: Vec<AgentMessage>, pub metrics: AgentRunMetrics, pub completion: AgentTurnCompleted,
}
pub struct AgentRunFailure {
    pub messages: Vec<AgentMessage>, pub metrics: AgentRunMetrics, pub failure: AgentTurnFailed,
}
pub enum AgentRunResult { Completed(AgentRunOutput), Failed(AgentRunFailure) }
```

这些均为既有类型。PB [types.proto](../../../talos-rs/crates/orchestration/agent-pb/proto/types.proto) 定义输入、消息和事件；
[harness.proto](../../../talos-rs/crates/orchestration/agent-pb/proto/harness.proto) 定义方法和结算。Finished 内层 Err 是已知失败；
settlement receiver 外层 Err 是未收到可靠结算。Progress EOF 不能单独作为成功。

## 3. Client 与服务端适配

```rust
// Concrete SDK in agent-client; implements every AgentHarness method above.
pub struct RemoteAgentClient;
impl RemoteAgentClient {
    pub async fn connect(endpoint: &str, trusted_ca_pem: Vec<u8>,
        agent_id: String, auth: NodeAuth) -> TalosResult<Self>;
}
// Concrete adapter in the node's talos-agent RPC module; implements generated AgentHarnessRpc.
pub struct AgentHarnessRpcAdapter;
impl AgentHarnessRpcAdapter {
    pub fn new(node_id: String, config: Arc<Config>,
        harnesses: Vec<(String, Arc<dyn AgentHarness>)>) -> Self;
    pub fn install_registration(&self, registration: Registration, deadline: Instant) -> TalosResult<()>;
    pub fn clear_registration(&self);
}
```

TLS 信任由 Client 的 CA 参数提供，节点 Workspace 配置由 adapter 的 Config 提供。Client 构造时绑定
agent_id，请求参数不允许切换到另一 Agent。节点 adapter 验证 auth、当前租约、agent_id，
并检查 Session.user_id 与 Registration.owner_user_id 一致；后续 input/interrupt 根据
原 Session/Turn 的归属校验。注册凭据不能调用 Harness。上层业务调用不会接触 token。

节点 Runner 依据 Register/Renew 的服务器有效期计算单调 deadline，统一调用
install_registration。同 token、同 owner 原位延长有效期，不能取消在途 RPC；更换 token
才撤销旧调用。clear_registration 拒绝新调用并撤销旧流，Harness 自身由 Worker Host 管理。
控制面 shutdown 只释放本进程内嵌 Harness；显式 Shutdown RPC 仍保持原 Harness 关闭语义。

Workspace 维护的三个附加端口、原类型和 PB 映射见[Memory 接口](memory-api.md)。

| Harness 方法 | gRPC | 响应映射 |
| --- | --- | --- |
| start | Start 双向流 | admission → accepted → progress* → settlement |
| compact_session | CompactSession 双向流 | 同上 |
| submit_input | SubmitInput 双向流 | admission → accepted / failure |
| interrupt | Interrupt unary | 原 AgentInterruptOutcome |
| clear_session / close_session | ClearSession / CloseSession unary | () / TalosError |
| shutdown | Shutdown unary | () / TalosError |

Start/Compact/Input 的首帧包含 auth 和原业务参数。节点调用本地 Harness，返回 admission
并持有原 permit。Client 把 admission 转成原对象交给 Service；consume 时在**同一条 RPC**
发送带 auth 的 commit 帧。节点只消费一次 permit；重复首帧/commit 是 InvalidRequest。
三个方法的 accepted 都在真实 permit.consume 成功后返回；失败传回原 TalosError。
SubmitInput 因此也保持原有等待 Provider 接收输入的语义。

未 commit 默认 30 秒释放 permit。commit 前请求流结束视为放弃；commit 后 Client 正常
half-close 请求流，继续接收响应，不视为取消。取消 RPC/响应消费失败才触发该调用的
取消清理。Client 的 consume 等待真实 accepted/Failure，不以“已交给传输”冒充成功。
无论本地还是远程，业务事件都先持久化再执行。

节点在原 Harness 产生 progress 时即时发送，全部 progress 之后发送 settlement；Client
持续解码并即时投递原 receiver，协调层/Service 无需等待最终结算即可消费。有限缓冲只用于
流控，不能把整次执行缓存到完成后再反馈。RPC adapter 和 Service 都先 drain progress，再消费 settlement，并校验事件序号。
流只存运行时对象，没有额外 invocation/request ID。
Interrupt 直接使用 agent_id + TurnRef，完全不依赖已结束的 Start 流。
Start/Compact 在 admission 前的 Failure 映射为方法 Err；admission 后、accepted 前的
Failure/断流完成 permit.consume 的 Err，并关闭未完成的结果接收；accepted 后的传输失败
交给 settlement receiver 外层 Err。SubmitInput 没有 settlement receiver，其 admission
后的 Failure/断流由 permit.consume 返回 Err，accepted 对应 Ok(())。
正常 settlement 后结束响应流，不再追加事件。

## 4. 通用失败契约

- 领域失败携带现有 ApiError；传输失败映射为 TalosError，保留本地
  typed source。远端原始栈只在远端诊断中保留，不从英文 message 重建错误分类。
- 原 Harness 返回的 ResourceNotFound、CleanupUnconfirmed 等业务结果保持原义；
  Client 不把断线伪造成这些业务结果，不给 Service 增加按传输模式分流的要求。
- gRPC 建流/认证错误使用标准 status，结构化 details 为 Failure；流内失败为 PB Failure。
  Client 在一个入口校验 code/字段。业务方法失败和网络结果未知不能混同。
- 使用 gRPC deadline、取消及有界流控，不自行增加 deadline 协议。队列超限明确失败并
  取消对应调用，不丢事件继续运行。默认消息上限 4 MiB，超限返回明确错误；大文件走原文件 owner。
- 网络失败不自动重试副作用调用。适配层尽力 interrupt 已启动的该次执行；无法确认清理时
  返回 ServiceUnavailable / RunCleanupUnconfirmed，不能把“没有句柄”转换为 Completed。
- RemoteAgentClient 仅对本实例确实接纳的 Turn 保留内存中的执行身份；节点或控制面重启后，
  新 Client 对旧 Turn 的 `ResourceNotFound` 不能证明旧执行已经停止。此时 `interrupt`
  返回现有 `RunCleanupUnconfirmed`，让 Service 保留未确认事实，不把它映射成取消成功。
  这个判别仅在 Client 内存中，不新增数据库执行状态或 RPC 业务字段；同一实例的原
  Harness `ResourceNotFound` 仍保持原义。
- shutdown 是永久停止该实例。成功后 Coordinator 摘除匹配句柄、adapter 撤下该 Harness；
  节点重用现有构造路径建立新实例后才能重新登记。clear/close 保持现有 Session 语义。

发送进度时即使对端停止读取、队列填满，adapter 仍同时等待租约撤销/到期。
发送受阻不延后 Harness interrupt；同 token 的 deadline 更新继续生效，不丢弃事件重试发送。

有限 unary 默认等待 30 秒；shutdown 使用原业务清理预算加 30 秒传输余量。
Start/Compact/Input 的建立与首个 admission、consume 的 commit/accepted 各有 30 秒阶段预算；
长期执行进度流不设置总时长限制。未消费/超时的 Client permit 撤掉流读取任务，释放网络连接。

撤销/断流时先关闭执行事件消费者，解除 Provider 在 event send 上的背压，再调用 Harness
interrupt 并报告失败或 CleanupUnconfirmed；下层原 Harness 保留未确认清理的权威状态。
Worker shutdown 先撤本地租约，再尽力 Unregister，不等网络确认才停止本地执行。

ApiError 的 JSON/PB 接收边界共享严格校验，拒绝空白/超长 message、非法本地化参数和
code 不匹配；不静默修补收到的结构。原 typed source 在节点投影前由负责边界报告；CLI
只输出公开 message，诊断栈不进入用户输出。gRPC details 通过同一 error_pb 编解码。

Client 根据 tonic 本地 TimeoutExpired 的 typed source 识别真正的超时（tonic 的 status 可能是
Cancelled）；远端主动取消仍返回 Cancelled。关闭 Host 时先关闭本地 Harness/Provider 与 MCP，
HTTP/2 传输排空最多 5 秒，超时只释放连接并报告，不能把传输断开变成业务 Completed。
