# Refactor 040：Agent Provider SPI 与全链路概念收敛

Status: implementation complete; merge review pending. This document consolidates Refactors 040 and 044. Verification evidence and open review items are retained in PR #16.

## 1. 目标与边界

本次不是增加一套 Agent 框架，而是为已有默认 Kernel Agent 建立清晰的 Provider 边界，同时消除 Service、持久化、事件、历史和客户端中的重复业务模型。

- 同一概念只有一个 owner、一套结构化定义；层间只保留有真实职责的转换。
- 对外统一 AgentMessage、AgentEvent 及其持久化事件 TimelineEvent。HTTP、JSON-RPC、Rust/TS SDK 共用业务语义。
- Harness 管执行期契约，Service 管产品策略及持久化编排，Store 管原子事实；Kernel 不反向依赖这些产品层。
- 保留当前产品能力：Session、Turn、输入/Steer、停止、用户交互、续接、压缩、清空、队列、历史、用量、Memory、Skill、Subagent。
- 移除旧实现、同义 DTO、无消费者 API、自由 JSON 状态框架和机械测试，不保留运行时兼容分支。

当前只有 Kernel Provider 的生产实现。Codex、Claude Code、Pi 是未来 SPI 实现方，不是本 PR 已交付的 Provider。ACP 可作为某个 Provider 与外部 Agent 的通信协议，但不替代 Talos 的产品 API、Store 或内部 SPI。

## 2. 纵向架构

```text
Web -> TS Browser SDK -> HTTP / SSE -----------+
Desktop -> typed Web client -> IPC / JSON-RPC +-> Gateway -> TalosService
TUI / CLI -> Rust SDK -> JSON-RPC ------------+                 |
Channel -> ChannelApplicationService --------------------------+
                                                              |
                           +----------------------------------+
                           |                                  |
                   Session / Turn / Timeline Service <-> Store
                           |
                     AgentCoordinator
                           |
                   AgentHarness = Harness<P>
                           |
                    AgentProvider::session
                           |
                       AgentSession
                           |
                  KernelAgentProvider adapter
                           |
                       AgentKernel
```

Gateway 和 Protocol 继续存在。它们承担协议、授权入口和连接生命周期，不拥有第二套执行状态或事件。Channel 通过 TalosService 实现的窄应用接口发起 Turn、订阅事件，不绕到 Kernel，也不依赖 Gateway。

### 编译边界

- `talos-agent-api`：产品 message/event/timeline/history 与 Agent 专属公共 DTO。
- `talos-types`：跨产品域共享的 DTO/primitive，例如 AgentTurnRef、AgentUserInput；不为目录整齐而复制或机械搬家。
- `talos-agent`：现有执行框架，拥有 Harness、Provider/Session SPI、Context、事件 reducer/sink 及执行所需只读 port。不新增 `talos-agent-spi`。
- `talos-agent-provider-kernel`：唯一 Kernel 适配点。
- `talos-agent-kernel`：原生推理、工具循环和私有 transcript，不依赖 Service、Store 或 Gateway。
- Store、协议和 SDK 不导出执行 reducer、run-control、Harness 或 Provider 内部类型。

## 3. 各层 API、对象与上下游

| 层 | API / 对象 | 唯一职责与转换 |
| --- | --- | --- |
| 客户端 | Rust GatewayClient；TS Browser/AppServer clients；Web 的 TalosWebClient | 复用生成的 request/response。Web/TUI 各自维护可丢弃展示投影，不执行服务端权威 reducer |
| Protocol / Gateway | HTTP handler、JSON-RPC application、Gateway / ServiceGateway | decode/encode、认证授权、连接和订阅。只包 transport envelope，不复制业务 DTO |
| Service | TalosService；SessionService；TurnService；Timeline services | 创建/绑定 Session、解析配置、准入、队列、续接、恢复和完成策略。唯一组装 AgentContext、唯一 Record-to-public 转换 |
| Store | SessionStore、AgentExecutionStore、ProfileStore 等各业务 Store | 私有 XxxRecord 和命名原子事务；不把 Record 暴露给 SDK/UI |
| Coordinator | resolve_agent、ensure_agent、start、submit_input、interrupt、compact_session、clear_session、close_session、shutdown | Agent 路由、已组装 Harness 查找与统一停机，不再定义一组同义执行 DTO |
| Harness | AgentHarness，通用实现 Harness<P> | 执行准入、规范事件、每 Session 排他、输入投递及资源清理；不管 DB、队列、标题或产品恢复 |
| Provider / Session | AgentProvider、AgentSession | Provider 级资源与原生 Session/Turn 执行；实现必须遵守取消、事件、任务登记契约 |
| Kernel Provider | KernelAgentProvider、KernelAgentSession | 将 SPI 输入转换为 Kernel request，将 Kernel event 写入既有 AgentEventSink |
| Kernel | execute(AgentKernelRequest, AgentKernelIo) -> AgentKernelExecution | 推理/工具执行与私有上下文；不构造产品 Turn 终态，不暴露原生事件给 SDK |

源码权威：
[Harness port](../../talos-rs/crates/orchestration/agent/src/port.rs)、
[Provider SPI](../../talos-rs/crates/orchestration/agent/src/provider.rs)、
[Context](../../talos-rs/crates/orchestration/agent/src/context.rs)、
[Kernel facade](../../talos-rs/crates/orchestration/agent-kernel/src/facade.rs)。
本文不再复制整套 Rust 声明，以免文档中的伪 API 与实现漂移。

## 4. Session、Turn 与 Context

产品 Session 是持久化会话；AgentSession 是 Provider 拥有的进程内句柄，两者不是同一个对象。Provider 可以保存私有 continuation/artifact，但不能拥有 Talos 的 SessionRecord、幂等、队列、历史或恢复策略。

Harness 感知有限 Session 信息，用于复用句柄、排他和清理；一次执行处理 Session 中一个逻辑 Turn 的一次 invocation。Waiting 后可释放 live 执行槽，后续 invocation 继续同一个 Turn。

```text
SessionRecord + Workspace/Agent Definition + model/config
                 | SessionService::build_agent_context
                 v
AgentContext
  session: AgentSessionContext { agent_id, session_id, workspace }
  turn_id
  project_id / working_directory / environment
  model / model_selection / permission_mode
                 |
        Coordinator -> Harness -> AgentSession
```

- AgentTurnRef = session_id + turn_id，是现有精确身份，不再新增 RunIdentity/ExecutionIdentity。
- AgentContext 只在启动、续跑和 compaction 时构建，传递后不改写。
- running input 使用 AgentInputParams { agent_id, identity, input }；interrupt 使用 AgentInterruptParams { agent_id, identity }。无需为了控制正在运行的 Turn 再解析模型和 Workspace。
- Context 不落盘，不添加 AgentContextRecord、fingerprint、revision 或 Harness Context Store。
- 重建来源属于上层：Session 绑定的 Workspace ID 及其路径/Agent Definition 由业务约定保持稳定；运行中影响 Context 的 patch/reload 由既有 gate 拒绝。进程停止时手工篡改配置属于不受支持的运维行为，不为此设计新校验系统。
- request/trace ID 不写入 Session；Provider-native Session ID 不进入公共 Context。

## 5. Provider SPI 与执行控制

### Provider 级 API

```text
capabilities() -> AgentCapabilities
session(AgentSessionContext, AgentOperationControl) -> AgentSession
shutdown(AgentOperationControl) -> ()
```

session 获取或重建内存句柄；传入的是 Session 生命周期 control，不是伪造 Turn。shutdown 释放 Provider 级资源，即使内置 Kernel 没有额外进程，也保持同一契约。

### Session 级 API

```text
turn(SessionTurnParams) -> SessionTurnResult
compact(SessionCompactionParams) -> SessionCompactionResult
clear(AgentOperationControl) -> ()
close(AgentOperationControl) -> ()
```

- SessionTurnParams：context、initial_input、continuation、input receiver、events sink、control；不再拆 Harness + IO 的第二套公共包装。
- SessionTurnResult：Completed { stop_reason } 或 WaitingForInput { pending, continuation }；错误走 AgentResult。Provider 不生成 Talos 持久化终态。
- SessionCompactionParams：context、events、control；Result 复用 TokenUsage。
- clear 删除可重建的 Provider 私有状态并保持句柄可用；close 释放 live 资源，不删除可续接材料。产品清空由 Service 协调 Store，不能由 Provider 越层清库。
- 不增加同义 run/resume/run_streamed API；续接仍调用 turn，携带既有 continuation。

### 控制如何到达运行中的 Turn

```text
Service input -> Harness durable admission -> bounded input channel
                                            -> AgentSession safe boundary
Service interrupt -> Harness ActiveRunControl -> cancellation + task cleanup
                                            -> AgentSession / native process
```

SessionTurnInputSender 与 Receiver 是同一个有界通道两端，不是两份输入模型。Sender 属于 Harness；Session 使用 Receiver 接收 SessionTurnInputDelivery，并在实际接收后确认投递。

AgentOperationControl 只给 Provider 取消信号与 RunTaskGroup 登记能力；ActiveRunControl 的终结、超时、强制清理由 Harness 拥有。Provider 的全部 task/process 必须登记，不能创建游离后台任务。Session、Turn、shutdown 分别拥有真实 scope，不为管理操作编造 Turn identity。

AgentCapabilities 只声明有真实 admission 消费方的差异：rich_input 集合、dynamic_input、user_actions 集合、durable_continuation、context_compaction。文本、事件顺序、流式交付、取消和清理是基线能力。无需把 MCP 等 Provider 私有细节塞进通用 capability。

未来 Provider 不支持的可选能力显式返回 unsupported，不能偷偷回退 Kernel；要宣称完整支持 Talos，必须通过相关产品场景契约，而不只是实现 trait 后能编译。

## 6. Harness 与 Service 的交接

[执行契约](../../talos-rs/crates/orchestration/agent/src/run.rs)：

```text
start(AgentRunParams { context, input, checkpoint?, continuation? })
  -> AgentRunExecution
       admission { canonical events, one-shot permit }
       progress event receiver
       settlement receiver
```

checkpoint 与 continuation 必须成对，且身份一致。checkpoint 是规范事件 reducer 的执行状态，不能进入 Provider SPI；continuation 是 Provider 私有引用。

1. Harness 预留执行与输入，返回暂停的 admission。
2. Service 用 Store 原子提交 admission、active index、幂等结果。
3. 提交成功后消费 permit，Provider 才能产生副作用；投递输入遵循同一顺序。
4. Service 单 writer 消费 progress，先持久化再发布。
5. Harness 交付唯一 settlement：WaitingForInput 或 Finished。waiting/terminal 不混入 progress 通道。
6. Service 提交 settlement 并释放 live 占用，再执行统一完成策略。

输入 delivery reservation 阻止 settlement 越过尚未完成的 commit/permit。请求 future 被取消不能遗失已经准入的执行/投递任务。

Service 在进入有副作用的准入前将完整提交交给独立任务持有，覆盖 Store commit、writer 启动、完成处理及队列 reservation 交接。请求仅等待结果；请求取消不能留下没有执行 owner 的 Admitting 占用。Harness 仍负责执行取消与资源清理。

一个产品准入流程只建立一次请求取消保护；队列任务直接复用内部提交实现，保留队列 commit 与自动派发屏障，不嵌套启动第二个准入任务。

## 7. 事件、Envelope 与 History

```text
Kernel event
  -> Kernel Provider -> AgentEventSink
  -> AgentEvent { identity, sequence, occurred_at, payload }
  -> Store AgentEventRecord
  -> Service TimelineEvent { event_id, event_seq, persisted_at, event }
  -> HTTP SSE / JSON-RPC notification
  -> SDK -> Web/TUI display
```

只有两层必要语义：

- AgentEvent：一次逻辑 Turn 的规范事实。
- TimelineEvent：该事实已经持久化后的顺序和游标。它不是另一种业务事件，也不再包 ApplicationEvent/ServiceTimelineEvent 或重复 identity。

Store 把 AgentEvent 字段规范化写入 AgentEventRecord，增加 store-global event_seq 与 persisted_at；Service 的单一 mapping 形成 TimelineEvent。event_id 绑定持久化序号，客户端只把它当 opaque cursor，不猜格式。Turn 内 sequence 连续，跨 Turn 的 event_seq 单调但不要求连续。

AgentMessage 使用强类型 role/status 枚举和 blocks 数组；block content 使用显式 tagged union。MessageStarted 可以为空，内容由后续 block 事件建立；MessageCompleted 带完整规范消息。客户端不能将空 MessageStarted 当成用户正文已交付。

Kernel 消费 Steer 输入的安全边界关闭此前 AgentMessage，后续输出进入新 AgentMessage，保证规范历史中“原问题/工具 -> Steer 输入 -> 后续回答”的顺序，不只修正前端排序。

### History 不另存一份消息

SessionHistoryResponse { data, next_cursor } 中每项为 SessionHistoryTurn { identity, result, completed_event_seq, completed_at }，从规范终态及对应 AgentEvent 生成。History 是有界读模型，不另写 transcript/history/result_json 副本。

- 服务端 reducer 负责严格 identity、sequence、message/block 生命周期、终态和指标不变量。
- Web LiveTurnProjection 只负责有界展示，属于 Web 而非 SDK；TUI 维护自己的显示状态。不存在“跨语言共享 reducer”要求。
- canonical history 不使用 Kernel artifact；Kernel artifact 不接受 UI history 替代。
- replay 固定 high-water，分页排空后交接 live；历史 cursor 固定 snapshot tail。拒绝跨 Session cursor，不因热缓存 GC 丢掉长期规范事件。
- 完整 Turn 读取复用 Store 的 Session cursor 分页，固定 durable tail，逐页筛选同一 identity；恢复与 History 增量 reduce，用户响应只保留目标 action 的事实。每页上限不是整轮事件数量上限，Subagent 投影也不截断规范事实。

## 8. 持久化对象：哪些保留、哪些移除

| 保留对象 | 保存的业务事实 | 不能重复保存 |
| --- | --- | --- |
| SessionRecord | Session binding、配置、标题、树关系、统计和进度 | 不保存 Context JSON、重复 model 字段、请求级 trace |
| AgentEventRecord | 规范事件，唯一长期内容事实 | 不另写 ApplicationEvent、完整 response JSON 或平行历史 |
| AgentActiveTurnRecord | Running / WaitingForInput / Cancelling 与 durable tail | 不保存 Completed/Failed、完整 pending action、messages/result/error 副本 |
| AgentContinuationBindingRecord | identity、action_id、Provider ref、expected tail、Waiting/Claimed | pending action 和已提交 response 从规范事件读取 |
| IdempotencyRecord / CommandLease | request hash、稳定 ID、命令结果状态与提交位置 | 不存第二份业务结果，重放从规范事实读取 |
| Profile usage | 按用途/模型统计的累计用量快照 | 续接不能再次累加前次 invocation 已计用量 |
| Provider artifact | 原生 prompt、continuation 和压缩材料 | 不作为产品 Session/History 的第二权威 |

这些 DB 对象只在 Store/Service 边界内使用；公开 SessionInfo、TimelineActiveTurn、SessionHistoryTurn 是明确的产品读模型，不是把 Record 换个名字外泄。

[AgentExecutionStore](../../talos-rs/crates/orchestration/store/src/session/agent_execution.rs) 提供命名事务：reserve_command、commit_initial_admission、commit_running_input_admission、begin_cancellation、commit_waiting_settlement、claim_continuation、commit_terminal_settlement 等。事件、active、continuation、幂等交接必须原子，不能让调用方逐表拼事务。

### 三种状态不是重复状态机

- Store active：跨重启非终态事实。
- Service LiveExecution：单 writer、publish 和完成等待句柄。
- Harness ActiveOperation：准入、执行任务和 cleanup 隔离。

删除任意一个都会丢失相应职责；保留不意味着再复制 terminal/pending/result。三者之间用同一个 AgentTurnRef 定位。

## 9. 故障恢复与完成策略

恢复由 Service 在订阅/恢复/中断等真实入口触发，不由 Harness 扫数据库。

| 场景 | 行为 |
| --- | --- |
| 仅浏览器断连/刷新，Service live 仍在 | 继续原 Turn，重放事件；不重发输入、不判 orphan |
| Waiting 且 continuation 尚未 claim | 保持等待，客户端恢复同一个问题 |
| 已 claim 的用户响应，尚无后续 durable progress，进程重启 | 从规范 User response 和 Provider continuation 续跑同一 Turn |
| 已 claim 且已有后续 durable progress，进程重启 | 当前 Kernel 无该位置的安全 continuation；不重放旧操作，确认 cleanup 后生成唯一失败终态 |
| 非终态但无可用 continuation | 确认 cleanup 后，从 durable checkpoint 生成唯一失败终态 |
| Cancelling 恢复 | 确认 cleanup 后生成取消终态 |
| cleanup 未确认 | 保留 active 隔离与诊断，不伪造“已结束”或重新获取句柄冒充清理成功 |

外部恢复复用已有 Session maintenance reservation；writer 故障恢复与显式中断都检查实际 cleanup outcome。自然完成和中断竞争通过 tail CAS 最多提交一个终态。

正常与恢复路径均返回真实 AgentRunSettlement，进入 TurnService::finish_turn：失败清理未完成 Profile，非取消终态在 live 与队列 reservation 释放后才派发下一条消息。取消不自动执行队列。Waiting 的用量作为累计快照保存，续接更新而非重复加算。

标题属于 SessionService：首次成功 Turn 完成时使用配置的 Inner Model 生成短标题，原子更新仍为默认标题且匹配当前 Turn 的 Session，避免覆盖用户修改或旧 Turn 并发结果。标题先写入再发布终态；生成错误有诊断，不添加猜测标题 fallback，不改变已经发生的 Agent 业务结果。

## 10. 前端恢复、消息交接与指标

- 初始导航输入在发起执行时消费，不等待 Turn 完成才清导航状态；整页刷新不得重复发送。
- 本地 pending input 立即显示，只由相同的服务端 message ID 接管，不能被旧 Turn 的消息或空 MessageStarted 清掉。
- 新 Turn 保留 Session 订阅 cursor；跨 Turn 也按 event_seq 去重。原 Turn 指标与消息句柄不能泄漏到下一 Turn。
- SSE 已拥有分页回放和 live 交接；移除首次恢复查询与 SSE 同时写投影的重复路径。终态不切断 Session 订阅，避免读到旧终态就停止恢复。
- 终态统一更新 Session metadata、列表、active 和 History。标题已在服务端终态之前落库，不再轮询猜测标题生成时间。
- 每个页面 Session 只有一个事件连接：规范 Timeline 与队列 revision 通知共用订阅、断连和重连；Queue 只保留 revision 协调及管理请求。send/retry 成功回调不再改执行终态，避免覆盖自动派发的下一 Turn；恢复到 idle 后仍继续订阅。
- Queue revision 只触发队列协调，不刷新 History/active；手动启动队列也复用 Session 事件推进执行状态。History 等查询由既有终态/恢复路径统一刷新，队列不再充当第二个会话状态 owner。
- start/retry 的提交 Promise 在服务端 ACK 后完成，乐观输入由 accepted message ID 接管；不再订阅展示 Store 等待终态，也不把重试请求 pending 当成 cancelling。真正停止仍调用服务端 interrupt，后续执行和终态仅由 Session SSE 推进。
- 新发送跟随最新输出；用户主动上滚阅读时不抢滚动。移除将问题固定到视口顶部的旧锚定流程。
- ActiveTurnRunning 错误不等于 ContextCompacting；显示真实错误并重新读取权威状态，不用刷新/重试掩盖业务错误。
- canonical created/started/completed 时间戳为 Unix seconds；Web 的 *Ms 展示字段只在映射边界乘 1000。

### tokens/s 的明确口径

主流推理测量区分 TTFT、生成阶段逐 token 延迟和端到端输出吞吐。生成阶段每用户吞吐通常以首 token 之后的输出 tokens 除以生成时间；不能把输入/缓存 tokens 加入分子。[NVIDIA GenAI-Perf 指标定义](https://docs.nvidia.com/deeplearning/triton-inference-server/archives/triton-inference-server-2570/user-guide/docs/perf_analyzer/genai-perf/README.html)

当前 AgentRunMetrics 提供累计 output_tokens 与 duration_ms，没有逐轮首 token/末 token 时间。因此 UI 展示“本轮平均输出吞吐”：
`sum(output_tokens) / (duration_ms / 1000)`。
duration 为现有执行计时，包含模型等待和工具执行；不是纯模型 decode speed，也不对各轮速度做算术平均。输入/缓存用量仅用于总量统计。缺少有效 duration 时不显示速度；tooltip 明确口径。不为这一修复新增未落地的性能指标 schema。

## 11. Kernel 私有状态与迁移

Kernel request 保留原生 UserInput、解析后的附件、ModelSelection/ModelInfo、环境、Kernel config、continuation、Skill 与 Memory prompt context。Kernel Io 只携带原生 input receiver 与 task group；Execution 独立排空 events 与 result，避免输入等待阻塞事件流。

删除 StateNamespace、StateDelta、SessionState.bags、EventActions，改用封闭的 SessionStateUpdate 表达实际使用的 prompt warning、compaction、continuity。不保留可任意扩展的 JSON bag。

Kernel transcript 只保留真实调用的 load/append 路径，移除无人消费的 list/filter 和 ToolResult stream 冗余字段。artifact 新写 schema_version=2，版本 1 仅在 artifact owner 中显式迁移，未知有语义的 action 失败，不 fallback 为默认值。

业务 JSON 例外仅限真实动态边界，例如外部 tool 的 schema/arguments、序列化 transport body；不得扩展为核心状态、配置或结果的万能 Value。

## 12. 破坏性变化与跨端一致性

本 PR 是相对 main 的整体迁移，不是每次消融都新增协议：

- JSON-RPC request/notification 采用单数 resource 的 `<resource>/<method>`；HTTP SSE 与 App Protocol 使用同一规范 TimelineEvent。
- Wire DTO 默认 camelCase、显式 tagged union；Rust/TS 由同一生成源对齐。SDK、CLI/TUI、Web/Desktop 同步迁移，不保留旧 dot 方法和 alias。
- StartTurnRequest 不接受重复 Context，Service 从 Session 权威构建。Provider 选择经 Agent Definition/Session binding，不让客户端猜 native backend 标识。
- Rust SPI 从产品 crate 移到现有 talos-agent，属于 source breaking；SDK 不再导出服务端 reducer。仓库外消费者必须按 release note 升级，不能假定已同步。
- Desktop adapter 由 TalosWebClient 的真实消费面进行编译期校验，删除 unsafe cast 和无消费者 convenience API；不趁机建立万能 transport framework。
- migration 将无法可靠转换生命周期的旧 Timeline 原样归档为 legacy_timeline_events，新运行路径只读 canonical agent_events。旧历史不会自动出现在新 History；如需导入，必须独立版本化 importer，不能静默删数据或添加第二 reader。
- schema 切换前需完成旧 active Turn 的收尾；不声称在旧版运行中的 Turn 上无损热升级。迁移失败显式阻断启动。
- 本轮 UI 回归修复不新增 wire 字段或方法。

## 13. 消融结论与验收原则

已删除/合并：AgentRuntime 同义 SPI、Run/Execution identity 包装、重复 Context、第二条内部 Timeline 转发、重复结果持久化、通用 Kernel JSON state/action 框架、无消费者 transcript API、SDK 暴露的 Web accumulator、Desktop 多余封装、恢复查询与 SSE 的双写、标题轮询、旧滚动锚定。

保留：Coordinator 路由、Harness 执行契约、Provider/AgentSession 原生执行、Service 产品编排、Store 原子事实、Timeline 持久化 envelope、History 读模型、各端展示投影。它们保护的是不同边界，不以“净删除行数”强行合并。

验收只覆盖真实场景：准入后执行、输入确认与 Steer、安全中断、Waiting/续接、断连分页回放、唯一终态、队列、压缩/清空、迁移、跨端协议及用户页面。复用现有 fixture，不为删除的概念写负向测试，不为纯命名/静态枚举增加机械测试。

尚未完成的验证不能写成设计已证明；具体成功证据、截图和剩余风险以 Plan 及验收报告为准。
