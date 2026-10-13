# 已有持久化数据与 Lody 对照

Status: Draft。作为同步覆盖清单，不引入 Lody 中 Talos 尚不存在的业务模型。
设计：[design](https://github.com/apilots/talos-sync/blob/7230f9fdf294e7ae218bdb6d3cf1c4ef60203380/spec/feat-044-talos-sync/design.md)、[contracts](https://github.com/apilots/talos-sync/blob/7230f9fdf294e7ae218bdb6d3cf1c4ef60203380/spec/feat-044-talos-sync/contracts.md)、[Worker契约](worker-coordination.md)。

## 1. Lody 客户端可观察事实

Lody只公开客户端，本清单依据本地源码，不能据此宣称服务端语言、DB或内部拓扑。
可复用的参考主要是Loro数据组织、snapshot/cursor联合持久化、认证/重连和短期presence。

| Lody同步内容 | 客户端依据 | Talos借鉴边界 |
| --- | --- | --- |
| 输入/消息ID、块、模型/输入配置、Plan/文件diff、finished/sendStatus/processing | [History](/Users/xujiang/data/ailabs/Lody/packages/shared/src/schema.ts:527) | 同步完整既有Agent事件，不能只同步最终文字；不用它的消息schema替换Talos。 |
| machine、Project、Agent配置绑定、父Session、标题、usage、最新输入/处理/等待指针 | [SessionMeta](/Users/xujiang/data/ailabs/Lody/packages/shared/src/schema.ts:749) | 固定Computer/子树、完整metadata和可见执行状态；处理指针不能当另一个控制面的执行准入。 |
| 持久mq、forkOperation、preview、externalHistoryCursor | [Session document](/Users/xujiang/data/ailabs/Lody/packages/shared/src/schema.ts:932) | 当前Talos内存queue和不存在的fork/preview模型不因此被扩成新功能。 |
| SQLite文档与remote cursor、认证snapshot/增量回调 | [repo store](/Users/xujiang/data/ailabs/Lody/apps/cli/src/lib/loro/sqlite-repo-store.ts:64)、[transport](/Users/xujiang/data/ailabs/Lody/apps/cli/src/lib/loro/streams-transport.ts:39) | 同事务保存文档/cursor，durable ACK与重连对账；不依赖Lody私有后端。 |

durable状态时间不证明在线；目录正文按机器读取，不能据公开客户端推断所有附件二进制已同步。
Talos可使用官方Loro Rust基础库，但首版不搬入通用文档repo、E2EE密钥管理、协作编辑器或Lody调度协议。

## 2. Talos覆盖清单

| 已有事实 | 来源 | 同步与执行边界 |
| --- | --- | --- |
| 用户输入、消息/块ID、公开reasoning、commentary/final、多媒体 | AgentMessage/AgentEvent | 全量原payload；包括已准入running输入和既有InlineBase64，不能漏掉增量或只存终态。 |
| 工具名、流式参数、progress、结果、错误、call ID | ToolCall/ToolResult/delta | 原结构完整去重/重放；不额外生成result JSON。 |
| compact trigger/stage/outcome/metrics、ExecutionSummary | 原Agent事件/blocks | 同步展示事实；Provider私有上下文/continuation不复制。 |
| success/failed/cancel、stopReason、partial、retryable、ApiError | TurnCompleted/TurnFailed/唯一reducer | 保留完整结果；同步断线不等于Turn失败。 |
| waiting action、选项/权限/问题、当前active metadata | WaitingForInput + SessionActiveTurnResponse | 原事件重建展示；Cancelling/回退无事件，capture原ActiveTurnState最小投影。回应只到当前协调者，不创建副本claim。 |
| token/context usage、工具/Turn数量、duration、Session计数 | MetricsUpdated/SessionInfo/Store | 按去重Turn事实派生，不把多端总量相加；导入不再计费。 |
| Computer/Agent/Workspace/Project/目录与user namespace | SessionRecord/SessionInfo | 固定Computer和执行binding；当前Actor权限与原Worker目录namespace分开，换控制面不改路径。 |
| 父Session/Turn、tree root、AgentPath | 原SessionTreeIdentity | 同一定义共享；子树导航和实际删除全部child；active child仍找其当前协调者。 |
| 标题、当前模型/权限/配置、活动时间 | Session Store/SessionConf/environment/channel/thread | 保留原None/显式覆盖、必要环境与来源；保留绑定Worker的模型/权限展示，接收端不重算；显式目录总在固定Worker。所有真实写入capture，字段合并，不用旧全量SessionInfo覆盖patch。 |
| system notice与历史分页边界 | TimelineEventPayload/HistoryService | Notice用source peer/event ID稳定键；本地event_seq映射，唯一reducer保留notice与原Turn归属。 |
| memory_processed_until_event_seq | SessionRecord/MemoryProgress | 转为稳定事件前缀水位同步，映射回本地序号，防换协调者后重复处理；不复制Worker索引或私有任务。 |
| 已完成TurnSubmission/TurnInput幂等结果 | 原IdempotencyRecord/TurnCommandStatus | 同时保留idempotency_key及独立request_id，event映射并恢复完整原响应；reserved/lease/claim不复制，不建RPC账本。 |
| clear/delete/parent tree删除 | 原业务事务 | reset generation/逐resource tombstone同事务capture；迟到旧代事件不能复活资源。 |
| FileInfo、tags/scope/MIME/大小/变更及file_id引用 | FileService/AgentUserContent::File | 安全metadata+正文peer；只由源FileService读/改/删，当前FileService经G连接访问NAT源，不伪造本端存储路径。源离线不可用。 |
| Computer名称/owner/启停、稳定UUID/embedded别名 | Node Store/NodeInfo | 复用目录事实，不复制token/hash/lease；当前在线仍来自真实RPC连接。 |
| 空Project、目录、Computer归属、计数及删除 | Project Store/ProjectInfo | 所有CRUD与Session影响的计数capture；目录读写/校验只到Worker。 |
| 尚未准入queue与通知 | SessionMessageQueueService | 当前owner的实时API，process-local；不同步持久队列，不静默迁移。 |
| 当前执行/恢复任务、continuation claim、租约、凭据 | 原Service/Store/Worker | 仅实际当前协调者与Worker；数据import不创建这些对象。 |

代码：[事件](../../talos-rs/crates/orchestration/agent-api/src/event.rs)、[消息](../../talos-rs/crates/orchestration/agent-api/src/message.rs)、
[Session record](../../talos-rs/crates/orchestration/store/src/session/types.rs)、[幂等](../../talos-rs/crates/orchestration/store/src/session/agent_execution/idempotency.rs)。

## 3. 不扩大原产品能力

| Lody能力 | Talos现状 / 本期决定 |
| --- | --- |
| 持久待发queue | [queue service](../../talos-rs/crates/orchestration/service/src/message_queue/service.rs:24)是内存；保留实时操作与原重启限制。切协调者不能消费另一端的未准入队列。 |
| 历史每Turn冻结模型配置 | 当前公共对象没有这份独立事实；只共享原Session当前设置，不倒推旧Turn配置。 |
| Plan/Goal/fork/PR/preview/native history cursor | 原工具文字/结果照常同步；不新增通用业务状态表或不存在的UI能力。 |
| 本端收藏/归档等偏好 | 当前SessionPatch没有这些字段；不因同步顺带增加产品写接口。 |
| 文件二进制全量复制 | 本期只有既有inline媒体 + 安全FileInfo/正文位置；Workspace正文Worker所有，外置附件按原FileService读取。 |

## 4. 必须验证

- 完整比对消息/工具/compact/metrics/partial/失败/notice，不只最后文字；同步期间另一端实际看到进度。
- A完成Turn并共享后，B用自己的Coordinator继续同一Computer/Session；创建节点的业务Service不参与B的准入与落库。
- B开始的Turn由同一Worker执行；history/附件/子Agent/memory回调真实到B，不残留注册时固定云数据端。
- waiting跨端回应一次；当前协调者失联不接管；父子树、删除、memory水位和已完成幂等结果保留。
- 同一内嵌Worker经本机loopback与远端RPC并发，证明统一排他；断网本机继续、重新同步后再开放换端。
- 双NAT下history/附件/memory/子Agent回调经agent通道返回当前调用者；sync不传命令。从未云登录/启sync的冷启动单机也必须可用。
- 外置附件源离线、内存queue重启、尚无产品模型如实记录限制；不靠假fixture或新复制系统掩盖。
