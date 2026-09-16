# Session 用户消息队列与活跃 Turn 引导

Status: archived
负责人：agent
创建日期：2026-08-15
更新日期：2026-08-19
相关方案：`feat-033-user-file-storage-model-input`、`feat-037-tool-permission-user-interaction`、`ERROR_DESIGN.md`

## 1. 决策摘要

Talos 为每个 Session 提供一份由服务端管理的内存消息队列。Web、Desktop、TUI、HTTP、App Server、Rust SDK 和 TypeScript SDK 访问同一份队列，不再各自维护业务队列。

Queue、Steer、Interrupt 和 Start 是四种不同操作：

| 操作 | 用户意图 | 执行语义 | Turn 结果 |
| --- | --- | --- | --- |
| Queue | 稍后执行 | 当前 Turn 正常完成或失败后，按 FIFO 取队首启动新 Turn | 创建新 Turn |
| Steer | 修正当前执行 | 将选中的文本和附件消息提交给指定 Active Turn，在当前 Tool 批次结束后、下一次模型请求前注入 | 保留当前 Turn |
| Interrupt | 立即停止当前工作 | 取消指定 Active Turn，并暂停本次自动 Queue 消费 | 当前 Turn 取消，Queue 保持等待 |
| Start | 恢复或指定执行 | Session 空闲时显式启动队首或指定 Queue Message | 创建新 Turn |

服务端只新增一个内聚组件 `SessionMessageQueueService`，放在现有 orchestration service 内。它管理有界 FIFO、修改、排序和必要的并发保护；`TurnService` 继续负责启动 Turn、提交 Active Turn 输入和监听 Turn 终态。

Queue 是服务进程级临时状态：

- 每个 Session 默认最多排队 `10` 条，可由服务端配置；
- 不写数据库、Timeline、Session Store 或文件；
- 服务进程故障、退出、重启、Session 删除或实例迁移后 Queue 丢失；
- 客户端连接失效时清空本地 Queue 投影和 `revision`；
- 不恢复、不补偿、不自动重放。

每次可见 Queue 变化后，服务端通过现有 Session 事件订阅发布 `messageQueue/changed(session_id, revision)`。该通知只表示 Queue 已失效，客户端随后调用 List 读取完整 `MessageQueue`；通知不持久化、不进入 Timeline，也不支持断线补发。

## 2. 实施前工程基础

本方案基于以下现有能力，不建立平行链路。

### 2.1 TUI 本地 Queue

实施前 TUI 使用 `VecDeque<QueuedMessage>` 和 `MAX_QUEUE_LEN = 3` 管理本地队列，并在当前 Turn 结束后启动队首。这提供了原有交互行为，但无法跨端共享。

服务端 Queue 上线后，TUI 删除本地业务 `VecDeque`、本地容量常量和本地自动派发，只保留输入草稿、焦点和选择态。

### 2.2 `TurnService`

实施前已有的 `TurnService` 负责：

- 校验 `TalosInputItem`；
- 通过正常路径启动 Turn；
- 检查 Active Turn；
- 转发和记录 Timeline 事件；
- 通过 `TurnExecutor` 调用 Coordinator。

Queue 派发必须复用这条路径，不能复制 `StartTurn` 实现。

### 2.3 Active Turn 输入桥

现有输入链路是：

```text
TurnService
  -> TurnExecutor.submit_input
  -> AgentDispatchInput
  -> Coordinator
  -> AgentInput
  -> Agent Harness active-turn channel
  -> Kernel
```

实施前 `AgentDispatchInput::Message`、`AgentInput::Message` 和 active-turn channel 只承载 `String`。Turn 首次输入已经通过 `Vec<TalosInputItem>` 支持文本和 `Files` 附件；Steer 最小扩展这条已有输入桥，把同一套文本和附件处理接入运行中输入。未新增 Steer 专用输入通道、附件模型、trait 或 Runtime 回调。

本阶段 Steer 只接受 `TalosInputItem::Text` 和 `TalosInputItem::Files`。`SkillSelection` 仍只在新 Turn 初始化时生效，不支持运行中 Steer。

### 2.4 Active Turn 与 Interrupt

实施前 Active Turn 已由 Timeline active-turn state 和 Harness 运行状态共同约束。内部 Interrupt 已按 `session_id + turn_id` 定位运行实例，但公共 `InterruptTurnRequest` 当时只有 `session_id`。

为了避免延迟请求取消新的 Turn，本方案给现有 `InterruptTurnRequest` 增加必填 `turn_id`，不新增第二套 Interrupt API。

## 3. 目标与非目标

### 3.1 目标

1. Active Turn 运行期间可以接收多条后续用户消息。
2. 消息按 Session 隔离并按服务端接受顺序 FIFO 排列。
3. Web、Desktop、TUI 和 SDK 通过同一套 API 读取、修改、删除、排序、启动或 Steer 同一份服务端 Queue。
4. Session 空闲或当前 Turn 到达终态后，通过正常路径派发队首。
5. 文本和附件 Steer 精确作用于客户端指定的 Active Turn。
6. 同一模型轮次边界前接受的多条 Steer 按接受顺序一起进入下一次模型请求。
7. Interrupt 完成后 Queue 保持等待，显式 Start 可以启动队首或指定消息。
8. 所有竞争路径不丢消息、不重复提交消息。
9. 服务端消费或其他客户端修改 Queue 后，所有 Session 事件订阅客户端都能收到失效通知并读取最新 Queue。

### 3.2 非目标

1. 不持久化 Queue。
2. 不提供跨进程、跨实例或跨 Session Queue。
3. 不提供优先级、定时执行、批量清空或历史归档。
4. 不持久化、恢复或重放 `messageQueue/changed`，不把它扩展成通用事件总线。
5. 不把等待中的消息写入 Timeline。
6. 不复用或暴露 Harness 内部 run backlog。
7. 不涉及 Agent 间 mailbox、消息聚合或多 Agent 通信。
8. 不新增 Queue Store、通用调度器、通用事件总线、工作流引擎或恢复状态机。
9. Queue Message 不保存独立模型、权限或工作目录覆盖；派发时使用 Session 当前有效配置。
10. 不支持通过 Steer 在运行中的 Turn 动态加载 Skill。

## 4. 核心语义

### 4.1 Queue

- `messageQueue/add` 总是先把消息交给服务端 Queue。
- Active Turn 存在时，消息等待当前 Turn 结束。
- Session 空闲且最近一次执行状态不是 Interrupted 时，`TurnService` 立即尝试派发队首。
- 只有队首可以自动创建新 Turn。
- Turn 启动被接受后，队首才从 Queue 删除。
- Turn 启动失败时，消息保留在原位置，不自动跳过或丢弃。
- Update 替换消息输入，但保留消息 ID 和位置。
- Reorder 只改变等待顺序，不启动 Turn。

### 4.2 Steer

- Steer 只能选择仍在 Queue 中的消息。
- Steer 只接受由 `Text` 和 `Files` 组成的输入；包含 `SkillSelection` 的消息整条拒绝，不做部分提交。
- 请求必须携带当前观察到的 `turn_id`。
- Harness 接受输入后，消息立即从 Queue 删除并归当前 Turn 所有。
- Harness 拒绝输入时，消息内容和顺序不变。
- 已被 Harness 接受的 Steer 即使随后遇到 Turn 失败或 Interrupt，也不恢复到 Queue。
- Steer 不创建新 Turn，也不静默回退为 Queue 派发。

### 4.3 Interrupt

- Interrupt 只取消请求中的 `turn_id`。
- Interrupt 不删除、不清空、不 Steer Queue Message。
- Interrupt 终态不触发自动派发，Queue 内容和顺序保持不变。
- Interrupt 后 Add 只入队，不唤醒处于 Interrupted 状态的 Session。
- 不新增 Queue `paused` 字段或独立暂停状态机；是否允许自动派发由现有 Session / Agent 执行状态和 Turn 终态原因决定。

### 4.4 Start

- `messageQueue/start` 只能在 Session 没有 Active Turn 时执行。
- 未指定消息时启动队首；指定消息时可以启动 Queue 中任意一条消息。
- Turn 启动被接受后才删除选中的 Queue Message，其余消息相对顺序不变。
- 启动失败时 Queue 不变。
- Start 创建的新 Turn 正常完成或失败后，恢复从队首自动消费。
- 普通 `turn/start` 不直接消费 Queue Message。

### 4.5 不变量

1. 服务端是 Queue 内容、顺序和容量的唯一权威来源。
2. 客户端本地 Queue 只是服务端 `MessageQueue` 的投影。
3. 一条 `QueuedMessage` 只属于一个 Session。
4. 同一消息不能同时被自动派发、Start、Steer、Update 或 Delete。
5. 同一 Session 同一时刻最多派发一条消息。
6. 可见 Queue 变化才递增 `revision`；内部忙碌标记不递增。
7. Steer、Start 或自动派发成功后消息只存在于 Turn 一侧；失败后只存在于 Queue 一侧。
8. Session 删除和 Queue mutation 不能交叉产生已删除 Session 的残留 Queue。
9. 服务进程重启后 Queue 为空，`revision` 重新开始。
10. 每次可见 Queue mutation 成功后都发布对应 `revision` 的 `messageQueue/changed`；失败操作和内部忙碌标记不发布通知。

## 5. 架构与职责

```text
Web / Desktop / TUI
        ^
        |  MessageQueue / Session events
        v
Rust SDK / TypeScript SDK
        ^
        |  HTTP SSE / App Server event subscription
        v
HTTP / App Server
        |
        v
TalosService
  |-- Session access control
  |-- Queue API entry
  |
  v
TurnService --------------------+
  |                             |
  |                             v
  |                  SessionMessageQueueService
  |                    - per-session FIFO
  |                    - capacity
  |                    - revision
  |                    - per-message busy guard
  |                    - typed change receiver
  |
  +-- normal Turn start
  +-- existing submit_input bridge
  +-- existing Interrupt path
  |
  v
Coordinator -> Agent Harness -> Agent Runtime / Kernel
```

### 5.1 `TalosService`

`TalosService` 继续作为传输无关的应用入口：

- 使用现有 `ResourceAccessService` 校验 Session 访问权限；
- 调用 `TurnService` 的 Queue 操作；
- 在 Session 删除成功后清理对应内存 Queue；
- 保持 HTTP、App Server 和 SDK 的 DTO 与错误语义一致。

`TalosService` 不保存 Queue，不复制 Queue mutation 逻辑。

### 5.2 `SessionMessageQueueService`

`SessionMessageQueueService` 是唯一新增业务组件，职责只有：

- 按 Session 保存有界 FIFO；
- Add、List、Update、Delete、Reorder；
- 在派发或 Steer 期间标记目标消息忙碌；
- 成功时删除消息，失败时解除忙碌；
- 维护 `revision` 并返回完整 `MessageQueue`；
- 在可见 Queue 变化提交后发布强类型 `session_id + revision`；
- 提供仅供应用层事件组装使用的进程内 change receiver；
- 清理已删除 Session 的 Queue。

它放在现有 orchestration service 中，不新建 crate，不定义 Store trait，不访问数据库，不执行 Agent，也不调用 `TurnService`、Coordinator 或 Harness。change receiver 只承载 Queue 变更，不抽象为通用事件总线；Queue Service 不管理连接、`subscription_id` 或 transport notification envelope。

### 5.3 `TurnService`

`TurnService` 负责跨组件编排：

- Add 后判断是否需要派发；
- 使用现有 Turn 启动实现派发队首；
- 执行显式 Start，启动队首或指定 Queue Message；
- 读取现有 Active Turn 状态；
- 通过现有 `TurnExecutor.submit_input` 提交 Steer；
- 在事件流结束且终态完成记录后，根据 Completed、Failed 或 Interrupted 决定是否派发下一条；
- 复用现有 Interrupt 路径。

Queue Service 不通过回调反向依赖 `TurnService`。

### 5.4 Gateway 与通知传输

- 不新增 Queue 专用 Subscribe API。App Server 和 HTTP 都复用现有 Session 事件订阅连接交付 `messageQueue/changed`；
- App Server 将通知发送给已订阅目标 Session 的连接；HTTP 在现有 Session SSE 中使用同名 event；
- Queue UI 建立 Session 事件订阅时使用现有持续订阅模式，不在 Session idle 或 Turn terminal 时自动关闭；
- `messageQueue/changed` 是不可恢复的 live notification，不进入 Timeline replay，也不占用 Timeline event ID；
- Gateway 只把 Queue Service 的强类型变更信号适配成 notification envelope，不读取或复制 Queue 状态；
- 客户端必须先建立 Session 事件订阅，再调用 List 取得初始 Queue，避免 List 与订阅之间出现静默丢事件窗口；
- 现有订阅发生 lag、终止或断线时，客户端清空 Queue 投影；重新连接后再次执行“订阅 Session 事件 -> List”。

### 5.5 Coordinator、Harness 与 Kernel

- Coordinator 继续转换 `AgentDispatchInput` 到 `AgentInput`。
- Harness 校验 Session、`turn_id`、运行状态和 channel 容量。
- Kernel 在模型轮次安全边界排空已接受输入。
- Queue Message ID 不进入 Coordinator、Harness 或 Kernel。

## 6. 数据模型与配置

### 6.1 公共对象

公共对象定义在现有共享 types 模块中，由 HTTP、App Server 和 SDK 复用：

```rust
pub struct QueuedMessage {
    pub id: String,
    pub input: Vec<TalosInputItem>,
}

pub struct MessageQueue {
    pub revision: u64,
    pub max_messages: u32,
    pub messages: Vec<QueuedMessage>,
}

pub struct MessageQueueChangedNotification {
    pub session_id: String,
    pub revision: u64,
}
```

设计约束：

- Request 已经携带 `session_id`，`QueuedMessage` 不重复保存公开 `session_id`；
- 数组顺序就是 FIFO 顺序，不增加 `position`；
- UI 不展示创建时间，本阶段不增加 `created_at`；
- `id` 用于 Update、Delete、Reorder、Start 和 Steer；
- `input` 直接复用 `TalosInputItem`，不定义第二套消息内容对象。
- Queue 派发新 Turn 时继续支持现有 `TalosInputItem` 语义；Steer 只消费其中的 `Text` 和 `Files`。
- `MessageQueueChangedNotification` 只携带 Session ID 和最新 revision，不携带 Queue delta 或完整内容。

`revision` 用于防止同一连接生命周期内命令响应、List 响应和通知乱序覆盖较新状态，不是持久化版本、事件游标或恢复凭据。

### 6.2 内部状态

内部只需要一个简单状态：

```rust
struct SessionMessageQueue {
    messages: VecDeque<QueuedMessage>,
    busy_message_ids: HashSet<String>,
    revision: u64,
}
```

`busy_message_ids` 只是防止同一消息在异步派发或 Steer 期间被重复操作，不是公共状态机，不导出到 API，也不写持久化。

同一 Session 的 Add、Delete、忙碌标记、成功删除、失败释放和 Session 清理必须串行化。异步 Turn 启动或输入提交期间不能持有全局锁，也不能阻塞其他 Session。

一旦某个 Session 创建过 Queue 状态，在 Session 删除或进程退出前不重置其 `revision`；空 Queue 仍可返回当前版本。

### 6.3 容量配置

配置归入现有 `agent` 配置域：

```yaml
agent:
  message_queue:
    max_messages_per_session: 10
```

规则：

- 默认值是 `10`；
- 配置值必须大于 `0`，无效配置启动失败；
- 配置由现有强类型配置系统解析，在组装 `SessionMessageQueueService` 时传入；
- 本阶段不定义热更新；配置变更随服务重启生效，而重启会清空 Queue；
- `MessageQueue.max_messages` 返回实际生效值，客户端不得硬编码 `10`；
- `messages.len()` 达到上限后 Add 返回专用 `MessageQueueFull`，Queue 和 `revision` 不变，也不发布 `messageQueue/changed`；忙碌中的可见消息仍计入容量；
- 单条消息继续使用现有 Turn 输入校验和大小限制；
- 文件只保存现有 file ID，不复制文件内容。

## 7. 公共 API

### 7.1 方法

```text
messageQueue/add
messageQueue/list
messageQueue/update
messageQueue/delete
messageQueue/reorder
messageQueue/start
messageQueue/steer
```

Add、List、Update、Delete、Reorder 和 Steer 直接返回同一个 `MessageQueue`，不为每个操作定义内容相同的 Response DTO。Start 创建新 Turn，直接返回现有 `StartTurnResponse`。

```rust
pub struct MessageQueueAddRequest {
    pub session_id: String,
    pub input: Vec<TalosInputItem>,
}

pub struct MessageQueueListRequest {
    pub session_id: String,
}

pub struct MessageQueueUpdateRequest {
    pub session_id: String,
    pub message_id: String,
    pub input: Vec<TalosInputItem>,
}

pub struct MessageQueueDeleteRequest {
    pub session_id: String,
    pub message_id: String,
}

pub struct MessageQueueReorderRequest {
    pub session_id: String,
    pub message_ids: Vec<String>,
}

pub struct MessageQueueStartRequest {
    pub session_id: String,
    pub message_id: Option<String>,
}

pub struct MessageQueueSteerRequest {
    pub session_id: String,
    pub message_id: String,
    pub turn_id: String,
}
```

约束：

- Queue API 只操作已存在的 Session，不承担隐式创建 Session；
- Add 成功后如果消息已经立即启动，返回的 `MessageQueue` 可以为空；
- List 返回完整 Queue，不分页；这是严格最多 `10` 条的有界集合，不适用普通无界 List 的 cursor pagination；
- Update 替换完整 `input`，保留消息 ID 和位置，并复用现有输入校验；
- Delete 只删除仍在等待且不忙碌的消息；
- Reorder 的 `message_ids` 必须是当前 Queue 全部消息 ID 的无重复完整排列，不能遗漏、增加或跨 Session 引用消息；
- Update、Delete 和 Reorder 遇到忙碌消息时拒绝整个操作，不做部分 mutation；
- Start 的 `message_id` 为空时启动队首，非空时启动指定消息；Session Busy、Queue 为空或目标不存在时 Queue 保持不变；
- `MessageQueueStartRequest.message_id` 在 wire 上是 optional + nullable，省略和 `null` 都表示队首；
- Steer 成功响应表示 Harness 已经接受输入；
- Steer 请求中的 Queue Message 包含 `SkillSelection` 时返回 `InvalidRequest`，消息保持在 Queue；
- 本阶段不提供 clear。

### 7.2 `messageQueue/changed`

```text
messageQueue/changed
```

通知 payload 使用 `MessageQueueChangedNotification`。服务端在 Add、Update、Delete、Reorder、Steer 成功删除、Start 成功删除和自动派发成功删除后发布通知；List、失败操作和忙碌标记不发布通知。

约束：

- 通知只发送给已通过 Session 权限校验的活跃订阅；
- 通知是失效信号，不是事件日志；客户端收到高于本地 `revision` 的通知后调用 List，不按通知次数推导业务操作；
- 同一个命令调用方可能同时收到完整 Queue 响应和相同 `revision` 的通知；已经持有该版本时不重复 List；
- 服务端不得重排不同 `revision`，客户端丢弃小于或等于本地版本的通知；
- App Server 和 HTTP 都通过现有 Session 事件订阅交付该通知，不定义 Queue 专用 subscription envelope；
- 断线期间的通知不保留、不重放；重连后先恢复 Session 事件订阅，再调用 List 覆盖本地状态。

### 7.3 Interrupt 请求

复用并调整现有对象：

```rust
pub struct InterruptTurnRequest {
    pub session_id: String,
    pub turn_id: String,
}
```

`turn_id` 必填。当前 Active Turn 不匹配时拒绝请求，不得取消更新的 Turn。`InterruptTurnResponse` 继续复用现有定义。

### 7.4 错误语义

本功能扩展现有公共 `ErrorCode`，增加稳定的 Queue Full 和 Queue Busy 业务错误码，不定义 Queue 私有错误对象：

| 场景 | 错误码 |
| --- | --- |
| Session 不存在或调用方无权观察 | `ResourceNotFound` 或现有授权错误 |
| 输入为空、结构无效或配置无效 | `InvalidRequest` |
| Queue 已满 | `MessageQueueFull` |
| Queue Message 不存在 | `ResourceNotFound` |
| Queue Message 正在派发、Start 或 Steer | `MessageQueueBusy` |
| Reorder 不是当前完整 ID 排列 | `Conflict` |
| Start 时 Queue 为空 | `InvalidRequest` |
| Start 时存在 Active Turn | `ActiveTurnRunning` |
| Active Turn 不存在或 `turn_id` 不匹配 | `ResourceNotFound` |
| Active Turn 不接受消息或输入 channel 已满 | `InvalidRequest` |
| Steer 消息包含 `SkillSelection` | `InvalidRequest` |

HTTP 继续使用现有错误映射，App Server 和 SDK 使用同一 `ErrorCode`。公共 fallback message 不携带 Session、Turn 或 Message ID；详细上下文只写服务端日志。

`MessageQueueFull` 的 wire value 为 `messageQueueFull`，默认公开消息为 `Message queue is full`，HTTP status 为 `409 Conflict`。所有客户端必须按该错误码显示“Queue 已满，无法添加新的消息”一类本地化提示，并保留用户输入草稿；不得根据 HTTP status、调用的方法、当前本地条数或错误消息文本猜测 Queue Full。`message` 只作为未本地化客户端的英文 fallback，不作为程序分支条件。

`MessageQueueBusy` 的 wire value 为 `messageQueueBusy`，默认公开消息为 `Message queue message is busy`，HTTP status 为 `409 Conflict`。客户端必须按该错误码显示本地化的“该等待消息正在提交”提示；不得通过通用 `Conflict` 或英文 message 猜测忙碌状态。

### 7.5 请求结果未知

Queue mutation 在响应前遇到连接失效时，客户端不得自动重试、按消息内容猜测结果或创建补偿操作。连接恢复后先调用 List，以服务端当前 `MessageQueue` 对账，再由用户决定下一步。

有 Session 事件订阅能力的客户端重连后，先恢复订阅再调用 List 完成对账；无长连接的 HTTP/SDK 调用方直接调用 List。两者读取的是同一份服务端状态。

本功能不增加 Queue 专用幂等缓存、TTL、命令日志或 Duplicate 状态。

## 8. 服务端流程

### 8.1 Add 与派发

```text
messageQueue/add
  -> TalosService 校验 Session 权限
  -> 复用现有用户输入校验
  -> Queue Service 检查容量并追加消息
  -> revision + 1
  -> 发布 messageQueue/changed
  -> TurnService 尝试派发队首
       -> 有 Active Turn：保持等待
       -> Session 空闲但执行状态为 Interrupted：保持等待
       -> Session 空闲且允许自动消费：标记队首忙碌并走正常 Turn 启动路径
            -> 启动接受：删除队首，revision + 1，发布 messageQueue/changed
            -> 启动拒绝：保留队首，解除忙碌
  -> 返回当前 MessageQueue
```

Add 不依赖客户端对 Busy 状态的判断。即使客户端观察到的 Active Turn 已经过期，服务端仍先接受 Queue Message，再根据权威状态决定等待或派发。

向允许自动消费的空闲 Session Add 时可能连续产生“已入队”和“已派发删除”两个版本及两条通知；它们都是真实可见状态。客户端按 `revision` 合并失效通知，最终只需读取最新 Queue，通知不承担业务事件审计职责。

### 8.2 自动派发触发点

只在以下时点尝试派发：

- 向空闲 Session Add；
- Active Turn 正常完成；
- Active Turn 失败；
- 显式 `messageQueue/start`。

Interrupted 终态、List、Update、Delete 和 Reorder 不触发自动派发。派发失败不启动循环重试，不删除或跳过队首。

### 8.3 Update、Delete 与 Reorder

```text
messageQueue/update
  -> 校验 Session 权限和替换 input
  -> 查找 message_id
  -> 消息忙碌：MessageQueueBusy
  -> 原位置替换完整 input，保留 message_id
  -> revision + 1
  -> 发布 messageQueue/changed
  -> 返回 MessageQueue
```

```text
messageQueue/delete
  -> 校验 Session 权限
  -> 查找 message_id
  -> 消息忙碌：MessageQueueBusy
  -> 删除消息
  -> revision + 1
  -> 发布 messageQueue/changed
  -> 返回 MessageQueue
```

```text
messageQueue/reorder
  -> 校验 Session 权限
  -> 校验 message_ids 是当前全部 ID 的无重复完整排列
  -> 任一消息忙碌：MessageQueueBusy
  -> 原子替换顺序
  -> revision + 1
  -> 发布 messageQueue/changed
  -> 返回 MessageQueue
```

Update、Delete 和 Reorder 都只操作仍在等待的消息。校验失败时 Queue 和 `revision` 不变，不发布通知。

### 8.4 Start

```text
messageQueue/start
  -> 校验 Session 权限
  -> 校验 Session 没有 Active Turn
  -> 选择 message_id 指定消息，未指定时选择队首
  -> 标记消息忙碌
  -> 复用正常 Turn 启动路径
       -> 启动接受：删除选中消息，revision + 1，发布 messageQueue/changed
       -> 启动拒绝：保留消息并解除忙碌
  -> 返回 StartTurnResponse
```

Start 可以在 Interrupted 后恢复 Queue，也可以从非队首选择一条消息立即执行。选中消息删除后，其余消息保持原相对顺序；新 Turn 正常完成或失败后继续自动派发当前队首。

### 8.5 Steer

```text
messageQueue/steer
  -> 校验 Session 权限
  -> Queue Service 查找消息
  -> 校验消息只包含 Text 和 Files
  -> 标记消息忙碌
  -> TurnService 使用现有 Active Turn 状态校验 turn_id
  -> TurnExecutor.submit_input
  -> Coordinator -> Harness
       -> 接受：Queue Service 删除消息，revision + 1，发布 messageQueue/changed
       -> 拒绝：Queue Service 保留消息并解除忙碌
  -> 返回 MessageQueue
```

异步调用期间消息仍显示在 Queue 中，但不能再次 Steer、Delete 或自动派发。

### 8.6 Active Turn 输入桥调整

沿现有类型逐层调整，不新增平行对象：

```rust
AgentDispatchInput::Message {
    input: Vec<TalosInputItem>,
}

AgentInput::Message {
    input: Vec<TalosInputItem>,
}
```

Harness active-turn channel 相应承载一条结构化用户输入。已有纯文本调用方使用 `TalosInputItem::Text` 构造输入；附件沿用 `TalosInputItem::Files` 和现有 file ID。

Agent Runtime 和 Kernel 复用 Turn 首次输入已有的文本规范化、附件解析和模型输入构造，不能为 Steer 再写一套附件系统。每次 Steer 保持为一条独立用户输入。

`SkillSelection` 不进入 active-turn channel。包含 Skill 的 Queue Message 只能继续等待并在后续新 Turn 派发时按现有初始化流程生效，或者由用户删除。

### 8.7 模型轮次边界

```text
模型请求 A
  -> 模型响应 A
  -> A 选择的 Tool 调用或 Tool 批次
  -> Tool 结果
  -> 排空当前已接受的 Steer
  -> 按接受顺序追加用户输入
  -> 模型请求 B
```

如果模型响应没有 Tool 调用，Kernel 在结束 Turn 前执行同样检查。存在 Steer 时继续当前 Turn 并构造下一次模型请求。

同一边界前存在多条 Steer 时，Kernel 一次排空并只构造一次下一轮模型请求。排空开始后新接受的 Steer 等待再下一边界。

Harness 只有在输入仍保证可被当前 Turn 消费时才能返回成功。Turn 已越过最终输入边界或 channel 已关闭时必须拒绝，Queue Message 保持不变。

### 8.8 Interrupt

```text
turn.interrupt(session_id, turn_id)
  -> 校验 Session 权限
  -> 校验当前 Active Turn 与 turn_id 一致
  -> 复用现有 cancel_active_turn 路径
  -> Runtime 和 Tool 按现有取消契约结束
  -> Timeline 记录取消终态
  -> Harness 释放 Active Turn
  -> 事件流关闭
  -> 不触发 Queue 自动派发
```

Interrupt 不修改 Queue，因此不递增 Queue `revision`，也不发布 `messageQueue/changed`。客户端通过现有 Turn 事件观察 Interrupted，并可以显式调用 `messageQueue/start`。

### 8.9 Timeline

- 等待中的 Queue Message 不进入 Timeline；
- Queue Message 启动新 Turn 后，按正常 Turn 路径进入 Timeline；
- Steer 被 Kernel 消费时，作为当前 Turn 的用户输入进入 Timeline；
- Queue 的 `revision` 不是 Timeline event sequence。
- `messageQueue/changed` 不是 Timeline event，不写入 Timeline，不使用 Timeline cursor，也不通过 Timeline recovery 补发。

### 8.10 Session 事件订阅与 List

```text
Client opens Session event subscription
  -> 校验 Session 权限
  -> 复用现有 App Server / HTTP Session 事件连接
  -> Client 调用 messageQueue/list 取得初始 MessageQueue
  -> 后续 mutation 发布 messageQueue/changed(session_id, revision)
  -> Client 在通知 revision 高于本地版本时重新 List
  -> 连接关闭 / unsubscribe / lagged / 服务退出：清空本地 Queue
```

必须先建立 Session 事件订阅再调用 List。若 List 进行期间收到更高 `revision` 的 Changed，客户端在当前 List 完成后再次 List，直到响应 `revision` 不低于已观察到的最高通知版本。相同 Session 同一时刻最多保留一个在途 List，其余 Changed 合并为一个待刷新版本。

`messageQueue/changed` 不支持 `Last-Event-ID` 或 Timeline recovery。断线重连后客户端重新建立 Session 事件订阅并主动 List，不推测断线期间发生了哪些 mutation。

## 9. 并发与生命周期

### 9.1 同一消息竞争

`busy_message_ids` 决定唯一结果：

- 两个 Steer 同时操作一条消息：一个进入输入提交，另一个返回 `MessageQueueBusy`；
- Steer 与 Update、Delete 或 Start 同时操作：先标记或先 mutation 的一方成功，另一方返回 `MessageQueueBusy` 或 `ResourceNotFound`；
- 自动派发、Start 与 Steer 同时操作同一消息：只有一个能标记消息忙碌；
- Reorder 与其他 mutation 按 Session 串行化；如果提交的 ID 集合已不是当前完整 Queue，则返回 `Conflict`，不得覆盖并发结果；
- 任何路径都不能向 Turn 重复提交同一消息。

### 9.2 Session 删除

Queue mutation 与 Session 删除必须按 Session 串行化。删除成功后清理 Queue；删除完成后到达的 Queue 请求必须在现有 Session 权限检查处失败。

删除 Session 时，现有 Session 事件订阅按原生命周期终止。客户端通过现有 Session 删除或订阅终止流程清空 UI，不为已删除 Session 发送一个可继续读取的空 Queue。

该约束在现有 service 内实现，不新增通用 Session lifecycle manager。

### 9.3 进程生命周期

```text
Talos process starts   -> Queue empty
Client adds messages   -> Queue exists only in process memory
Client connection lost -> Session event subscription ends; client clears Queue projection and revision
Client reconnects      -> Resubscribe Session events, then List current process state
Talos process exits     -> Queue discarded
Talos process restarts  -> Queue empty; revision starts again
```

如果原服务进程仍存活，重连后的 List 可以再次返回原 Queue。这是读取仍存在的服务端状态，不是客户端恢复。List 不恢复通知历史。

同一 Session 的 Queue API、Turn API 和事件流必须路由到同一服务实例。多实例部署需要 Session 粘性路由；本方案不增加分布式缓存、数据库或消息中间件。

### 9.4 通知可靠性

- Queue mutation 和对应通知在同一服务进程内按 `revision` 有序发布；
- Queue Service 在持有对应 Session mutation 临界区时递增 `revision` 并发布通知，保证并发 mutation 的通知顺序与版本顺序一致；
- 复用现有 Session 事件分发的有界 channel 和 backpressure 语义，不能反向阻塞 Queue mutation 或 Turn 执行；
- 没有 Session 事件订阅者或接收端已经关闭时，mutation 仍然成功，服务端不缓存待发送通知；
- 慢消费者发生 lag 时不得静默继续，服务端按现有机制终止 Session 事件订阅；
- 客户端收到订阅终止或底层连接失效后立即清空 Queue 和 `revision`，重新订阅 Session 事件并调用 List；
- 服务端故障或退出可能来不及发送终止通知，客户端仍按连接失效规则清空；
- 不保存未送达通知，不按 `revision` 请求补发，也不从 Timeline 重建 Queue。

## 10. 多端交互

### 10.1 一致性规则

1. Add、List、Update、Delete、Reorder 和 Steer 返回完整 `MessageQueue`；Start 返回现有 `StartTurnResponse`。
2. 页面加载、切换 Session 和重连时，客户端先建立现有 Session 事件订阅，再调用 List 建立 Queue 投影。
3. 同一连接生命周期内，客户端只应用大于当前 `revision` 的 Queue；相同版本幂等忽略，更旧版本丢弃。
4. 收到高于本地版本的 `messageQueue/changed` 后调用 List；多个 Changed 合并，不能并发发起无界 List。
5. 命令成功时立即应用其中的 Queue；其他客户端 mutation、Start、Steer 删除和服务端自动消费通过 Changed + List 刷新。
6. List 响应低于已观察到的最高 Changed revision 时，再执行一次 List，直到本地状态收敛。
7. 连接或 Session 事件订阅失效时立即清空本地 Queue 和 `revision`；不得保留旧投影等待重连。
8. 客户端不维护独立业务 Queue，不从 Timeline 反推 Queue，也不自行预测自动消费。
9. Queue 容量使用 `MessageQueue.max_messages`，但 Add 是否允许以及 Queue Full 错误始终由服务端判定。

### 10.2 Web 与 Desktop

Waiting Messages 是独立组件，但视觉上位于 Composer 输入框上方，并与 Composer 共用一个连续外层区域：

```text
┌──────────────────────────────────────────────────────────┐
│  第一条等待消息                               Steer  删除 │
│  第二条等待消息                               Steer  删除 │
│  第三条等待消息                               Steer  删除 │
│                                                          │
│  输入下一条消息...                                       │
│  +  权限                                  模型  麦克风  ■ │
└──────────────────────────────────────────────────────────┘
```

视觉要求：

- Waiting Messages 内部不为每条消息增加卡片边框；
- 行间只使用留白或极轻分隔；
- Delete 使用轻量图标按钮；
- Steer 使用低强调文字操作；
- 不使用高饱和背景或重色状态块；
- 长消息显示单行摘要并截断；
- Queue 为空时不占额外高度。

行为要求：

- Active Turn 运行期间 Composer 继续可编辑；
- Enter 调用 Add，不再拒绝提交；
- 服务端成功后清空草稿，失败后保留草稿；
- Add 返回 `MessageQueueFull` 时显示明确的 Queue 已满提示，不自动重试，也不根据本地条数推断结果；
- Waiting Message 支持 Update、Delete、Reorder 和 Start；Update 保留 ID 和位置，Reorder 提交完整 ID 顺序；
- Text + Files 消息提供 Steer；包含 Skill 的消息不提供 Steer，但仍可 Update、Delete、Reorder 和 Start；
- Steer 请求在途时只禁用对应 Steer 操作并显示轻量加载状态，不将整条消息置灰，也不禁用 Delete 或更多操作；服务端仍以 `MessageQueueBusy` 处理并发竞争；
- Active Turn 存在时 Start 不可用；Interrupted 后 Queue 保持显示，由用户显式 Start 队首或指定消息；
- Stop 只执行 Interrupt，不从 UI 删除或启动 Queue Message；
- 收到 `messageQueue/changed` 后调用 List，再用返回的完整 Queue 替换 Waiting Messages；
- Changed 触发的 List 成功后清除先前 Queue mutation 错误，避免已消费消息的 Busy 提示残留；
- 连接失效时立即清空 Waiting Messages；
- 重连后先恢复 Session 事件订阅，再通过 List 重建显示。

### 10.3 TUI

TUI 使用同一 `MessageQueue`，不维护业务 Queue 副本。

```text
  Waiting messages 2/10
  1  第一条等待消息                         [Steer] [Delete]
  2  第二条等待消息                         [Steer] [Delete]

  > 输入下一条消息
```

行为要求：

- Active Turn 期间输入调用 Add；
- 服务端成功后才清空输入；
- Add 返回 `MessageQueueFull` 时保留输入并显示明确错误；
- Waiting Message 支持 Update、Delete、Reorder 和 Start；所有操作使用服务端 Message ID；
- Text + Files 消息可以 Steer；包含 Skill 的消息不提供 Steer；
- Interrupted 后不弹出或自动提交本地队首，用户通过 Start 恢复；
- 数量上限来自 `max_messages`；
- 删除本地 `MAX_QUEUE_LEN` 和自动派发；
- 收到 `messageQueue/changed` 后调用 List 并替换 Waiting Messages，Turn 终态不自行消费本地 Queue；
- 连接失效时清空 Queue 和 `revision`；
- UI 变化使用现有 TUI styling、wrapping 和 snapshot 测试工具。

## 11. 兼容性与变更面

### 11.1 公共类型和协议

- 在共享 types 中只定义一套 `QueuedMessage`、`MessageQueue`、`MessageQueueChangedNotification` 和必要 Request；
- HTTP、App Server、Rust SDK 和 TypeScript SDK 复用这些对象；
- Add、List、Update、Delete、Reorder 和 Steer 直接返回 `MessageQueue`；Start 返回现有 `StartTurnResponse`；
- App Server 增加 `messageQueue/add`、`list`、`update`、`delete`、`reorder`、`start`、`steer` 和 `changed`；
- HTTP 暴露同一组 Queue operation，并在现有 Session SSE 中增加 live-only `messageQueue/changed` event，不新增 Queue SSE endpoint；
- Rust SDK 和 TypeScript SDK 对外提供同一组 operation；Web、Desktop 和 TUI 都通过现有 Session 事件订阅 + List 同步；
- `ErrorCode` 增加 `MessageQueueFull` 和 `MessageQueueBusy`，HTTP 都映射到 `409 Conflict`；
- 更新 App Protocol schema、TypeScript 生成物和 SDK 导出；
- transport 只允许现有 notification envelope 和 SSE framing 差异，不复制 Queue 业务 DTO。

### 11.2 Breaking Change

- `InterruptTurnRequest` 新增必填 `turn_id`；
- 延迟 Interrupt 不再按 Session 取消任意当前 Turn；
- Interrupt 终态不再触发 Queue 自动派发，用户通过 `messageQueue/start` 恢复；
- TUI 从最多三条本地 Queue 改为服务端默认十条；
- Web 在 Busy 状态下 Enter 从拒绝提交改为 Add；
- Active Turn Message 输入从纯字符串扩展为承载 Text + Files 的 `Vec<TalosInputItem>`；
- 客户端 Queue 状态改为服务端投影，断线时立即清空。
- 公共 `ErrorCode` 新增 `MessageQueueFull` 和 `MessageQueueBusy`；Queue Full 和 Queue Busy 不再返回通用 `Conflict`。
- 现有 App Server Session 事件订阅和 HTTP Session SSE 增加 live-only Queue Changed notification；该通知不保证断线重放。

### 11.3 明确不改动

- 不新增数据库 migration；
- 不修改 Session Store schema；
- 不新增 Queue persistence trait；
- 不修改 Timeline schema，不把 Queue notification 写入 Timeline；
- 不新增 Queue 专用 Subscribe / Unsubscribe 或 SSE endpoint；
- 不新增通用事件总线或 Queue 通知持久化；
- 不保留服务端失败时回退到客户端本地 Queue 的双路径。

## 12. 验证策略

### 12.1 Queue Service

覆盖：

- 单 Session FIFO 和多 Session 隔离；
- 默认接受前 `10` 条并拒绝第 `11` 条；
- 自定义容量和零值配置失败；
- Update、Delete、Reorder、Start、Steer 和自动派发之间的同消息竞争；
- 成功删除递增 `revision`，失败和忙碌标记不递增；
- Update 保留 ID 和位置，Reorder 只接受当前全部 ID 的完整排列；
- Add、Update、Delete、Reorder、Start 删除、Steer 删除和自动派发删除发布对应 `revision` 的 Changed；
- Queue Full、其他失败和忙碌标记不发布通知；
- 并发 mutation 的通知严格按 `revision` 排序；
- 派发失败保留队首；
- Session 删除清空 Queue；
- Queue mutation 与 Session 删除不产生残留状态。

### 12.2 Turn 与 Kernel 集成

覆盖：

- Active Turn 期间 Add 不启动并发 Turn；
- Turn 正常完成和失败后派发队首；
- Interrupt 终态不派发，Interrupt 后 Add 只入队；
- Start 在空闲 Session 启动队首或指定消息，成功后删除选中项；
- Active Turn 存在时 Start 失败且 Queue 不变；
- 普通 `turn/start` 不直接删除 Queue Message；
- 派发复用正常 Turn 启动路径；
- Steer 使用现有 `submit_input` 链路；
- 文本和附件复用现有解析及模型输入逻辑；
- 包含 `SkillSelection` 的 Steer 返回 `InvalidRequest` 且消息保持不变；
- Tool 批次结束后的下一模型请求可见 Steer；
- 同一边界多条 Steer 按顺序进入一次模型请求；
- 最终输入边界竞态中，Steer 要么被接受并保证消费，要么被拒绝并保留在 Queue；
- Steer 接受后立即 Interrupt 不恢复该消息；
- Interrupt 只取消请求中的 `turn_id`。

### 12.3 API 与 SDK

覆盖：

- HTTP 与 App Server 使用相同 Request 和 `MessageQueue`；
- Rust 与 TypeScript 导出一致；
- Add、List、Update、Delete、Reorder 和 Steer 返回的 Queue 都包含实际 `max_messages`；
- Start 返回现有 `StartTurnResponse`，Queue 删除通过 Changed + List 对其他客户端可见；
- Queue 已满返回 `MessageQueueFull + Message queue is full`，HTTP status 为 `409 Conflict`；
- Queue Message 忙碌返回 `MessageQueueBusy + Message queue message is busy`，HTTP status 为 `409 Conflict`；
- 其他 `409 Conflict` 场景不会被客户端误判为 Queue Full；
- App Server 和 HTTP 的现有 Session 事件订阅都能交付 `messageQueue/changed(session_id, revision)`；
- 客户端先建立 Session 事件订阅再 List，List 期间发生 mutation 也能通过最高通知 revision 触发再次读取；
- Changed、List 和 mutation response 乱序时按 `revision` 收敛到最新 Queue；
- Session 授权失败不泄漏 Queue；
- 两个客户端观察相同 FIFO 顺序；
- 响应前断线不自动重试，重连后先恢复 Session 事件订阅再通过 List 对账。

### 12.4 Web、Desktop 与 TUI

覆盖：

- Busy 时 Enter 调用 Add；
- 成功清空草稿，失败保留草稿；
- Waiting Messages 与 Composer 连续布局；
- 消息行无独立卡片边框；
- Update、Delete、Reorder、Start、Steer 和 Interrupt 行为；
- 容量来自 `max_messages`；
- `MessageQueueFull` 显示明确错误并保留草稿，`MessageQueueBusy` 显示明确的提交中提示，其他 Conflict 不猜测为 Queue Full 或 Queue Busy；
- Queue Changed 对账成功后清除陈旧 mutation 错误；Steer 在途时只禁用对应 Steer 操作；
- Interrupted 后 Queue 保持等待，用户显式 Start 后才恢复执行；
- 其他客户端操作和服务端自动消费通过 Changed + List 刷新；
- 连接失效清空 Queue，重连先恢复 Session 事件订阅再 List；
- TUI 不再本地派发；
- Web 使用视觉测试，TUI 使用 snapshot 测试。

### 12.5 端到端场景

```text
场景 A：运行中连续排队三条，三个新 Turn 按 FIFO 串行执行
场景 B：排队两条，将两条依次 Steer 到当前 Turn
场景 C：排队两条，Interrupt 后两条都保持等待，显式 Start 后选中消息启动
场景 D：Web Add，TUI 收到 Changed、List 后 Steer
场景 E：TUI Add，Web 收到 Changed、List 后 Update、Reorder 或 Delete
场景 F：Steer 接受后立即 Interrupt，Steer 消息不恢复，其余 Queue 保持等待
场景 G：服务进程存活时客户端断线，客户端先清空，重连 Session 事件并 List 后重新显示
场景 H：服务进程重启后 Queue 为空，客户端不恢复或重放
场景 I：默认容量下第 11 条 Add 返回 MessageQueueFull 和公开 message，前 10 条和 revision 不变
场景 J：旧 turn_id 的 Steer 和 Interrupt 都不能影响新的 Active Turn
场景 K：包含附件的 Steer 在下一模型请求中可见，file ID 正确进入 Timeline
场景 L：包含 SkillSelection 的 Steer 被拒绝且 Queue Message 保持不变
场景 M：Completed/Failed 自动消费队首后，Web 和 TUI 都通过 Changed + List 删除对应 Waiting Message
场景 N：命令响应、List 与 Changed 乱序到达，客户端按 revision 保留最新 Queue
场景 O：Session 事件订阅 lag 或连接失效后客户端清空 Queue，重新订阅并 List
场景 P：Start 指定非队首消息，启动成功后只删除选中项，其余相对顺序不变
场景 Q：并发 Add 后旧 Reorder 因 ID 集合不完整返回 Conflict，不覆盖新消息
```

## 13. Review 清单

1. 是否只新增 `SessionMessageQueueService`，没有 Queue Store、Manager、Registry 或通用事件总线。
2. Queue 派发是否复用 `TurnService` 正常启动路径。
3. 文本和附件 Steer 是否复用现有 Active Turn 输入桥及附件处理，而不是新增平行通道。
4. 公共 API 是否只定义一套 Queue 对象和必要 Request，没有为不同 transport 复制业务 DTO。
5. Queue Full 和 Queue Busy 是否分别使用稳定的 `MessageQueueFull`、`MessageQueueBusy` 错误码和公开 message，而不是由客户端猜测通用 Conflict。
6. 并发保护是否只使用 Queue 内部忙碌标记，没有额外状态机。
7. Session 删除与 Queue mutation 是否不会留下孤立 Queue。
8. 服务重启、断线和请求结果未知的语义是否明确为不恢复、不补偿、不自动重放。
9. Web、Desktop 和 TUI 是否共享服务端 Queue 和容量。
10. Interrupt 和 Steer 是否都精确绑定请求中的 `turn_id`。
11. Skill 是否只在 Queue 派发新 Turn 时生效，不会通过 Steer 动态加载。
12. `messageQueue/changed` 是否覆盖所有服务端可见 mutation，并只携带 `session_id + revision`。
13. 通知是否保持进程内、有界、不可恢复，不进入 Timeline 或持久化层。
14. Web、Desktop 和 TUI 是否都通过现有 Session 事件订阅 + Changed + List 同步，没有客户端本地业务 Queue。
15. Interrupt 是否只阻止本次自动派发，Start 是否在不引入 Queue pause 状态机的前提下恢复执行。
