# 控制面与 Worker：统一 RPC 与当前 Turn


默认拓扑决策：agent.talos.love是公网主注册/管理节点，统一Computer目录、账号鉴权及Connect RPC中转。共享的Desktop/talos-server内嵌Worker及云端纯Worker均注册到这个默认入口；调用端Desktop经它调用其他Desktop或云Worker。Desktop保留自己的管理DB与协调能力；当前发起端负责Turn准入/事实落库和结果反馈，同账号管理数据经sync.talos.love同步。公网主注册节点不替当前发起端代写Session事实；独立本机工作不依赖公网服务。

Status: Implemented（阶段1–3）。统一RPC、当前协调者回传、Session 历史游标与共享续跑已接线；产品登录/UI和Docker联合验收仍在阶段4–5。
相关：[边界](https://github.com/apilots/talos-sync/blob/7230f9fdf294e7ae218bdb6d3cf1c4ef60203380/spec/feat-044-talos-sync/design.md)、[共享对象](https://github.com/apilots/talos-sync/blob/7230f9fdf294e7ae218bdb6d3cf1c4ef60203380/spec/feat-044-talos-sync/contracts.md)、[提交流程](https://github.com/apilots/talos-sync/blob/7230f9fdf294e7ae218bdb6d3cf1c4ef60203380/spec/feat-044-talos-sync/flows-and-storage.md)。
完整连接/结果/资源回传与独立单机模式：[RPC routing](rpc-routing.md)。

## 1. 两个 endpoint、一个 Worker

| 部署 | 管理 endpoint | Worker endpoint / 调用 |
| --- | --- | --- |
| Desktop app-server + embedded Worker | 本机管理 HTTP listener；已有 stdio 复用同一 Application/Service，保持 Electron 调用接口 | 独立 loopback gRPC listener；本机 Coordinator 也通过 agent-client 调用。共享时同一 adapter 加 outbound Connect。 |
| talos-server + embedded Worker | 产品 HTTP 与 Node 注册/Connect 的管理 listener，可按 HTTP/2 路由组合 | 独立 gRPC listener；服务内 Coordinator 同样走客户端，不直调内嵌 Harness。 |
| standalone talos-worker | 无产品管理 API/业务 DB | 同一 gRPC listener + 按配置注册/Connect，复用 host/adapter/鉴权/清理。 |
| talos-sync | 独立WSS同步listener（含其HTTP升级握手） | 无 Worker listener，也不代理 Worker 调度。 |

复用 agent_node 的配置 owner，新增内聚 WorkerRpcConfig 以解耦“listener”与既有 runner transport：
`bind_address`、`tls`配置；最大帧/并发沿用现有RPC载体边界，不增加重复配置。实际 endpoint 从已绑定 socket 返回，不猜端口。
Desktop 默认 loopback:0 防止多实例冲突；talos-server/standalone 可配固定 Worker 地址。管理端口与 Worker 端口禁止冲突。
已实现配置为`agent_node.worker_rpc`，四份仓库YAML补默认值并保留既有用户修改。原 control_plane listener 是注册入口，不能误当 embedded Worker listener。

`WorkerRpcHost::bind(&WorkerRpcConfig, AgentHarnessRpcAdapter) -> Self`封装listener/adapter/address/shutdown；`TalosWorker::start_local_rpc() -> (Arc<WorkerRpcHost>, NodeAuth)`封装同Worker的本机入口并返回私有凭据。独立部署由`RegisteredWorker::start(Config)`组合相同host，管理宿主用`RegisteredWorker::attach(&TalosWorker, Arc<WorkerRpcHost>, &AgentNodeRunnerConfig, on_failure)`给现有host附加注册。
注册 runner 消费这个 host/adapter，不再次 TalosWorker::build；Registry 只接收 agent-client 的 AgentHarness 实现。
启动顺序：Worker构建 -> listener bind/ready -> 客户端连接 -> 注册 Harness -> 管理 API ready -> 可选云注册。
该顺序不包含等待sync/云登录；本机身份与凭据按现有本地机制建立，云注册/租约只授予远程访问。云租约observer撤销只影响它的远程入口/调用，不撤销本机准入或停止整个Worker。
停机先停止准入，再取消/确认清理，最后 join listener/Connect；listener 失败不能偷偷回退直调。

loopback 可使用有类型的本机 h2c transport + 本机 NodeAuth，非 loopback 必须 TLS + 原鉴权。
本机凭据由宿主按节点凭据机制交给客户端并遵守现有保存权限；不因为在同进程/127.0.0.1 就免鉴权。
内嵌loopback凭据由可信宿主生成、只在宿主内存交给SDK，不进入配置/云注册/同步；管理Service先完成原账号/业务授权。独立云NodeAuth仍校验Computer owner；云lease撤销/重连drain只处理远程调用，不能等待或取消本机调用。当前listener配置为agent_node.worker_rpc（默认loopback:0、tls=None），runner仅负责注册transport/identity/control endpoint。
网络业务均是既有 AgentHarnessRpc/WorkerEnvironmentRpc/PB；上层仍只持有 AgentHarness。
设备在 NAT 后时，远端经已认证主动 Connect 到同一 adapter，不能假设 localhost 公告地址对其他机器可达。

## 2. 注册入口与当前调用者分离

Computer 保持一个稳定 UUID；云 NodeService 颁发的 NodeAuth/lease 不同步给其他控制面。
其他控制面用真实账号凭据建立到 Agent Gateway 的管理连接；Gateway 校验 Computer owner 后代理已有 WorkerRpcCall/Result。
它注入所需 NodeAuth、维护网络 callId/流控，不调用其自身 SessionService、不写代执行 Turn 的业务事实。
management peer是稳定宿主UUID；每个Computer流由Gateway颁发独立connection UUID，路由归已验证账号/目标Computer并且只在内存。同peer可同时打开多Computer流；不加协调节点授权表或RPC待执行账本。

跨控制面只增加下面的传输上下文；业务 RunParams、AgentContext、InputParams、Result 不重定义：

```protobuf
message CoordinatorCallContext {
  string connection_id = 1;
  uint64 rpc_call_id = 2;
  SessionContext session = 3;
  optional string turn_id = 4;
  optional string project_id = 5;
  string coordinator_id = 6; // Stable management peer, independent of a network connection.
}
message SessionHistoryCursor {
  uint64 history_generation = 1;
  optional string completed_turn_id = 2;
  uint64 completed_event_sequence = 3;
}
```

CoordinatorCallContext和SessionHistoryCursor的PB位于已有agent-pb；历史游标 domain由talos-types唯一维护，agent-protocol提供RPC端口/codec并引用原对象；通过RPC request的可选caller对象承载，不改业务params。Gateway按认证连接和原请求提取scope并注入；调用者不可选择任意回调URL。资源client每次执行冻结原auth与caller。
目标Worker复用已有AgentNodeControl.Connect主动连agent.talos.love；调用控制面主动连接新增AgentCoordinatorRpc.Connect双向流，双方都无需公网监听。
新增的是调用者/结果/回调接线，不另造Worker Connect协议。两者都归agent入口；Bearer认证沿用现有账号机制，与sync连接独立。
这不是Worker的NodeAuth注册流，也不调用远端Session业务准入。carrier仅使用以下有类型的分支：

| 方向 / carrier | 内容 |
| --- | --- |
| CoordinatorToControlFrame.connect | peerId、computerId、servesFiles；每个流绑定一个Computer，多流复用HTTP/2 channel。账号由已验证Bearer决定，不接受accountId/admin标志。 |
| CoordinatorToControlFrame.call | 原WorkerRpcCall；Computer已由握手绑定，Gateway校验归属/调用者并注入NodeAuth及首帧caller，不重复传computerId。 |
| ControlToCoordinatorFrame.result | 原WorkerRpcResult，callId/sequence仍按既有载体；跨连接ID转换仅在内存。 |
| ControlToCoordinatorFrame.resource_call | 原WorkerRpcCall的callId/sequence/caller、仅允许其既有资源request分支。 |
| CoordinatorToControlFrame.resource_result | 同callId/sequence、原WorkerRpcResult中的资源response oneof；保留原stream结束/失败/流控。 |
| 双向cancel/ack | 复用现有网络取消/消费确认语义；不是业务持久ACK或新执行账本。 |

resource request/response oneof逐项引用ReadSessionHistory、ResolveAttachments、Read/AdvanceMemoryProgress、
SaveToolImage及Spawn/Send/Wait/Interrupt/FollowUp/ListSubagent的现有PB对象；闭合枚举，不用任意method/JSON/bytes替代业务结构。
管理Gateway为这些原RPC增加可选caller PB对象；direct loopback回调与reverse carrier解码都进入同一资源handler。本机回调使用内部临时loopback listener，复用原管理资源owner与私有host NodeAuth；不新增公网端口或用户配置。
新的ConnectCoordinator帧只属于agent-pb/protocol/client；同步库/独立sync服务不处理它。
鉴权与资源业务handler分开：本机回调验证本机NodeAuth；reverse回调的Worker凭据先由注册Gateway验证，
当前B只接受自己已认证管理连接上的frame，并核对Computer/active identity/账号及本端实际invocation，再进入原业务端口。
B不能拿导入NodeInfo验证别的控制面颁发的token，也不能将“来自Gateway”当作跳过原资源权限检查的理由。
connection_id由Gateway为每条已认证管理流生成随机UUID，不能由调用者复用/指定；流结束移除精确连接路由。peer_id只标识源管理宿主；源FileService以(account, peer_id)选择最新在线且声明servesFiles的连接。invocation的identity/Computer必须匹配原请求和Session binding，旧connection_id不能命中新连接。
回调只选已有认证管理连接，不接受用户提供任意 callback URL。旧连接退出不能覆盖新连接的路由。

history、attachments、memory progress、tool images、subagent、Dream 回调按当前 invocation 选数据端。
NAT下原AgentSessionDataClient仍以agent入口为网络端点，附invocation context，由Gateway路由到当前协调者；“注册端点不变”不等于“业务owner不变”。
使用既有 AgentSessionDataRpc/AgentSubagentControlRpc/AgentToolImageRpc 对象；本机同样走 RPC，允许管理 listener 的资源服务接收回调。
只增加必要的caller对象/现有Connect载体覆盖，不能复制业务 DTO 或在 Worker 保存 Session DB。
并发 Session A/B 属于不同协调者时必须隔离 callback handle；禁止修改一个全局 client.endpoint。
缓存的 AgentSessionContext 保留 Computer/Agent/Workspace 身份，当前回调上下文按每个 Turn 绑定，不因换协调者重建 Workspace。


### 已实现SDK/资源接线

| 实际API / owner | 用途 |
| --- | --- |
| `AgentNodeClient::connect_coordinator_with_resources(token, peer_id, computer_id, Arc<dyn AgentResourceRpcHandler>)` | 开已认证双向流，返回`(Arc<AgentExecutionConnection>, NodeAuth)`；此NodeAuth持有本端账号凭据而非目标Worker lease，Gateway才替换。无资源业务场景可用`connect_coordinator`，其回调明确拒绝。 |
| `RemoteAgentClient::from_connection(connection, agent_id, auth)` / environment connection client | 同一原Harness/WorkerEnvironment API隐藏Connect。 |
| `TalosApplicationRuntime::coordinator_resource_handler(computer_id)` | SDK回调owner，组合原History/Memory/Attachment/File/AgentControl；SDK先校验活跃scope，原handler再校验持久Session绑定与业务权限。 |
| `AgentSessionDataClient::for_invocation(auth, session, caller)` | 固定本次调用凭据和资源关联，不修改共享client endpoint。 |
| `TalosWorker::use_local_resource_rpc(client)` | 完整管理组装在开放API前装受保护loopback资源端口；Kernel只依赖原端口resolver，不依赖上层Service。 |
| `AgentNodeClient::read_peer_file(token, source_peer_id, FileReadRequest, max_bytes)` | 返回原`FileInfo`和`Bytes`，走源FileService；不要求active Turn。 |
| `update_peer_file(..., FileUpdateRequest)` / `delete_peer_file(..., FileDeleteRequest)` | 原response/错误；tags/metadata保留None与显式空值区别，不导入本端假objectKey。 |

源文件产品载体为`AgentPeerFileRpc::{ReadFile, UpdateFile, DeleteFile}`，原业务字段一一投影至`PeerFile*Params`，响应为原FileInfo投影/有界chunk/原Failure。与现有工具图像复用同一个FileInfo PB投影（`ToolFileInfo`增加原FileScope），不造另一Rust文件模型。Gateway账号认证；源端重新验证自己的账号凭据后进入原FileService的actor/scope权限。stage3只补物化位置及原FileService中的远端分支，不把文件正文加入Sync。

## 3. 排他、完整历史与切换

当前 Harness 已在 Start/Compact 等操作中预留 Session，并在控制面提交 admission 后 consume permit：
[reservation](../../talos-rs/crates/orchestration/agent/src/harness.rs:137)、[Start](../../talos-rs/crates/orchestration/agent/src/harness/execution.rs:39)、[RPC commit](../../talos-rs/crates/orchestration/agent/src/rpc_streams.rs:57)。
复用这条链；加强 Worker 公共入口对 immutable binding 的校验，使同一 Session 不能绕到另一个 Harness 或 local 入口逃避排他。
CRDT 收敛和各本地 DB 的 active 行均不能代替 Worker 排他。

1. 当前B使用本地完整事实；只有缺少所需Shared 历史游标的数据时才catch_up，导入Session/tree/binding、Timeline、memory/幂等完成态。纯本地使用原Store，不访问sync。
2. agent-client 查询同一 Worker 的 Session 状态。运行或清理未确认时拒绝新 Turn；空闲时核对执行 历史游标。
3. Start 预留必须原子校验 期望历史游标 + 当前绑定，不能仅“查询 idle 后再调用”。返回原 ActiveTurnRunning/Conflict。
4. 原 admission events -> B 业务事务/capture -> 原 commit/permit；只有这一条链可开始模型。失败释放未消费 reservation，并恢复预留前完整 marker（协调者、Local/Shared、active 与历史游标）；首次预留没有旧 marker 则撤销新 marker。未执行的 B 不能阻止 A/C 按原 Shared 历史游标续跑。已消费后的清理确认与事实提交规则不变。
5. Worker progress/settlement -> B 原事务；终态不能先于已知 outcome。跨端下一 Turn 需先得到 hub 的持久同步确认。
6. B通过下面的状态提交释放跨端边界，原子更新Worker最小恢复标记与runtime 历史游标；A/C补齐该历史游标后可成为下一Turn协调者，仍只能调用同一个Worker。

只扩展最小 Session 状态 RPC（已有 agent-pb/protocol 所有）：
`ReadSessionExecutionState(SessionContext) -> {历史游标, activeTurn?, coordinatorId?, handoffReady}`；
`CommitSessionExecutionState({SessionContext, identity, 历史游标, visibility: Local|Shared}) -> Empty/Failure`。
Start 首帧 context 增 historyCursor；这些是跨控制面因果边界，不是另一套执行结果或持久租约。
Read/Commit RPC的request沿用NodeAuth + params信封，response为原Empty/Failure或下面的状态；它们进入同一Connect RPC载体：

```protobuf
enum SessionStateVisibility { SESSION_STATE_VISIBILITY_UNSPECIFIED = 0; LOCAL = 1; SHARED = 2; }
message SessionExecutionState {
  SessionHistoryCursor history_cursor = 1;
  optional TurnRef active_turn = 2;
  optional string coordinator_id = 3;
  bool handoff_ready = 4;
}
message CommitSessionExecutionStateParams {
  SessionContext session = 1;
  TurnRef identity = 2;
  SessionHistoryCursor history_cursor = 3;
  SessionStateVisibility visibility = 4;
}
```

UNSPECIFIED拒绝；提交者必须是该状态的当前调用者，不能为别人的active Turn伪造终态。
清理完成与共享事实确认均属于公共Worker RPC host的Session gate，不能只依靠先返回的Provider settlement。
handoffReady 只在实际清理确认且共享事实已提交后成立；输入等待与 cleanupPending 不能被视为空闲。
本机离线执行允许当前协调者在 Local 可见性下继续同 Worker；另一协调者不能越过未共享 历史游标。恢复网络后 flush 再开放 Shared。
另一管理端已收到完全相同的终态历史、但 Worker 仍由原协调者持有 Local 标记时，准入最多等待 30 秒，定期读取原 Worker 的 Shared 确认；超时返回 ServiceUnavailable。等待本身不提交 Shared、不改 owner、不跳过 durable ACK；原协调者的 Local 离线执行和旧游标冲突保持原规则。
Local是当前协调者已持久化的事实，不限定Worker的物理位置；同一协调者经agent通道继续远端任务也不要求sync在线。
coordinatorId在active或尚未Shared时保留当前调用者，避免idle期间被别的调用者越过未共享事实；Shared确认后不再形成Session永久owner。
Worker不解析Loro、不建业务DB或执行账本。RPC host在Worker自己的`$TALOS_HOME/data/worker/session-heads/`保存每Session最小恢复标记：immutable binding、历史游标、最后coordinatorId、active identity?、Local/Shared及cleanupConfirmed。文件名只来自校验后的Session ID/安全编码；私有文件原子替换/fsync，在准入consume前标记未确认、终态确认后更新；失败不开始模型或开放handoff。
Worker重启先读标记；只有全新Session可按空历史游标初始化。最后协调者若是本机，从本机原Store恢复；若是B，由B通过原Read/Commit状态RPC提交其持久事实并核对标记identity/历史游标，纯Worker同样如此。B在NAT后仍使用G认证管理连接，G不能用自己的旧DB代替B。
本机已有已确认Local事实且coordinator匹配时不等待G/H；若最后协调者是B、事实未Shared且B不可达，则该Session显式不可执行，其他独立本机Session仍可工作。不能用A旧DB或hub旧Shared 历史游标抹掉较新的Local标记。
恢复未确认Turn先走原协调者取消/清理/terminal事实恢复，不凭active map为空就准入，不重放工具；标记未知或损坏明确失败。Shared需本端完整事实与标记历史游标一致，不要求所有本机启动先访问sync。
旧流断连按现有取消/清理处理；事实未恢复时显式不可执行，不能自动接管或重放工具。
clear/delete/compact/close 沿用原能力与权限，穿过同一 Worker 排他；reset/tombstone 确认后再开放新代。

## 4. 执行中交互与重试

新空闲 Turn 的所有业务准入、文件上传、落库都发生在当前控制面；不存在“回创建 Session 的 Service”。
执行中的 Input/Interrupt/queue 需要当前 Turn 的 Service/claim，跨端经 Agent Gateway 连接交给当前协调者，范围仅限该 active identity。
复用既有 SubmitTurnInput/InterruptTurn/MessageQueue 请求与响应domain；ActiveSessionCommand闭合枚举仅包装这八项原API。AgentCoordinatorRpc.InvokeActiveCommand的PB oneof逐项投影原字段，UserContent、ActionResponse、TurnRef、InterruptResult复用原PB，禁止任意method/JSON/bytes。G验证账号/目标Computer并按sourcePeer中转；源Service校验本端真实active claim/current peer/identity后调用原业务流程，不安装另一个执行器或队列。
此部分通过现有认证gRPC管理连接中转，PB只投影原产品DTO；客户端和源Service复用同一domain对象，原Timeline通知继续服务UI。源Service同时校验Computer、Session/Turn、当前协调者及原真实claim。
响应 waiting 时 action/Turn/coordinator 必须匹配；路由失效明确失败，不在另一端生成 continuation。

HTTP Idempotency-Key 与 body clientRequestId 按原规则归一；跨端请求先补齐原 completed command，再按同 requestHash 返回已有结果。
两端新请求并发最终由同一个 Worker 拒绝一个；不能只检查两个不同 DB 的活动行。
断连/超时不自动重放，不宣称 exactly-once 或工具回滚；查询稳定 Turn/command facts 确认是否已经接受。
尚未准入的队列留当前内存 owner，不跨控制面自动转移、拼接或冒充重启可恢复。

中心注册与双向同步：共享Worker只向G=agent.talos.love（现有talos-server）注册、续租和Connect；A/B/C管理实例按同账号双向同步Computer目录与业务事实到各自原DB，不复制凭据/租约或重复注册。每个实例的协调者通过统一Harness/RPC SDK选择自身loopback或G远端路由；内嵌Worker与独立Worker的业务与准入行为一致。

阶段3接入字段：AgentContext.history_cursor引用唯一SessionHistoryCursor domain（PB field 9）；SessionRequest/SubmitInputRequest/InterruptRequest的caller引用统一CoordinatorCallContext。Read/Commit请求携带caller；Commit.identity可空仅用于原clear后的generation递增。原sessions.history_generation在同一clear/import事务维护；不新增执行账本。Worker鉴权授予个人Computer完整访问，workspace_user_id为不可变文件命名空间，不作为另一账号授权。所有业务账号/Computer/Session权限仍在G和原Service校验。

共享管理宿主也建立到G的Coordinator.Connect资源连接（绑定自己的已注册Computer），供另一端访问其FileService和当前Turn产品API。本机Harness仍使用loopback，不能把自己的管理回传连接替换为远端执行客户端。纯Worker只建立Node.Connect，不提供管理端业务handler。


阶段5准入/实时反馈自审：当前协调者可以从原progress receiver立即取最多64个已到达的连续事件，复用原Store事务一次提交并逐条发布。事件identity/sequence、CAS tail、业务与outbox共提交、Input必须跟在前序事实之后及terminal单独结算均不变；不等待凑满批次、不合并或丢弃事件、不增加RPC协议/状态表/超时。目的仅是消除逐事件事务积压造成的Input和子Agent终态回传延迟。模型目录查询必须传递原ModelListRequest.nodeId，与目标Worker配置接口指向同一环境。

配置准入：原update/delete/reload请求先将公开Computer UUID映射为宿主实际节点；目标Worker现有SessionGate最终检查所有未终态marker（包括WaitingForInput和重启后未确认清理），并在配置事务期间排斥新的执行准入。只复用原Reservation与恢复文件；控制面副本/本端执行claim不能作为远端Worker是否空闲的唯一依据，不新增执行表或分布式锁。

阶段5修正：Clear/Close（Delete沿用两者）复用现有SessionHistoryCursor作为期望参数。Service在原Store读取历史游标，agent-client原SessionRequest携带expected_history_cursor，Worker在现有SessionGate锁内核对历史游标后维护准入；缺失历史游标直接InvalidRequest，陈旧历史游标返回Conflict且不修改marker/Provider状态，不增加兼容分支或持久格式。原插件/MCP写接口与config写接口统一先映射Computer，再复用同一Worker配置Reservation检查WaitingForInput及重启未终态；原只读方法不占配置Reservation。

命名：`SessionHistoryCursor`为会话历史游标，`history_generation`为清空后的历史代次，`completed_turn_id`为最后完成Turn，`completed_event_sequence`为该Turn内已完成的事件位置（offset）。事件序号并非整个Session的全局offset，也不是Timeline分页cursor。`commit_cleared_history`提交清空后的空历史游标。PB field number不变，Worker私有恢复文件路径/字段编码保持原格式；没有新增状态或兼容分支。

清空恢复：同一协调者的原Store已提交下一代空历史，而Worker仍为已确认空闲的前一代时，原prepare_execution/共享重试通过已有CommitSessionExecutionState补交清空结果；不得恢复别的协调者或覆盖活动Turn。等待取消与未提交续跑均以原Worker Gate的准确identity及cleanup为准；SDK不维护另一个已接受Turn集合，未消费Reservation释放恢复此前waiting identity。

Direct中转：G原发布owner保存同一个AgentEnvironmentClient传输handle；direct与outbound都供Coordinator.Connect转发原PB，SDK内替换NodeAuth/注入当前caller。G不安装第二个SessionGate、不做业务准入/落库。撤销原publication关闭handle及该路由；实时结果与资源回调仍返回当前协调者。

等待/孤儿取消的原Service恢复结算没有live execution task；原Store终态提交后、释放Service维护排他之前，必须复用commit_worker_history_cursor补交Worker Local历史游标，再安排原Shared重试。Worker确认失败要显式返回，不先宣布取消成功；原prepare恢复已提交终态，不重放工具。
