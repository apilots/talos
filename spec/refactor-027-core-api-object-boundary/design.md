# Core API Object Boundary 重构设计

Status: implemented
Owner: agent
Created: 2026-06-21
Updated: 2026-06-22
Related: `refactor-023-agent-kernel-simplification`, `feat-026-talos-skills-system`, `feat-027-plugin-marketplace-skill-install`, `feat-028-talos-mcp-system`

## 背景

Talos 当前已经有可运行的核心链路，也已经通过 `talos_types`、`app-protocol`、Rust SDK 和 TS SDK 建立了服务端/客户端共享对象的基础。但在实现过程中边界约束不够严格，导致几类技术债继续累积：

1. 对外对象命名不统一，同一类 API 同时存在 `Request`、`Params`、`Ack`、`Response` 等不同表达。
2. 核心 turn 请求曾在层间被重复建模，例如历史上的 `TalosMessageRequest -> GatewayRequest -> OrchestratorRequest -> AgentRequest -> InvocationContext`；当前主链路已压缩为共享 `StartTurnRequest -> AgentDispatchRequest -> AgentRequest -> AgentKernelRequest`。
3. `ApplicationServiceGateway` 是当前 Gateway 的具体实现，负责把 HTTP / JSON-RPC / Channel 之下的协议无关 API 薄委托到 Application Service；core turn 与主要资源 API 已纳入该出口。
4. `CoordinatorImpl` 当前同时承担 Agent 注册/路由、session resolve、部分 session progress/title 持久化，职责过重。
5. `TalosJsonValue` 被用作工具参数、工具结果、timeline payload、UI JSON 内容等多种语义的兜底对象，削弱强类型边界。
6. TS SDK 已经承担协议归一化职责，本轮目标是让 SDK 业务对象直接来自 `talos_types` 生成物，避免继续通过 `Omit`、normalizer、unwrap 等方式修补协议类型泄漏。

本 spec 不是重新设计 Agent Kernel。`refactor-023` 已经确定并实现 `AgentHarness -> agent-kernel/general_turn` 的主执行链路。本 spec 专注更外层：Gateway、Application Service、AgentCoordinator、AgentHarness 之间的职责和对象边界。

本重构的核心动机不是增加抽象层数量，而是消除服务端、SDK、协议层之间为同一业务语义反复定义对象、反复转换对象的问题。任何新增分层如果导致 `HTTP Params -> JSON-RPC Params -> Gateway Request -> Service Request -> SDK Request` 这类同构对象链继续增长，都属于错误方向。

## 当前实现事实

当前核心业务链路：

```text
HTTP / AppServer / SDK adapter
  -> Gateway
  -> TalosApplicationService
  -> TurnService
  -> AgentDispatchRuntime(CoordinatorImpl)
  -> Agent(AgentHarness)
  -> Agent Kernel
```

当前事件持久化链路：

```text
Agent Kernel
  -> AgentHarness emits TalosTimelineEvent
  -> Coordinator / TurnService stream back
  -> TimelineEventService::record_talos_event
  -> TimelineEventRecord / ActiveTurnRecord
  -> TimelineEventEnvelope
  -> HTTP SSE / JSON-RPC notification / SDK stream
```

当前对象链：

```text
StartTurnRequest
  -> AgentDispatchRequest
  -> AgentRequest
  -> AgentKernelRequest
  -> InvocationContext
```

当前几个关键事实：

- `AgentHarness` 是 Agent Kernel 上面的包装层，名称保留。
- `AgentHarness` 当前通过 `Agent` trait 对上提供执行能力。
- `CoordinatorImpl` 当前位于 Application Service / TurnService 下方，持有 Agent registry，并负责 route 到目标 Agent。
- `Application Service` 已经存在，是 HTTP / JSON-RPC / Desktop / SDK 等共享服务实现的基础。
- `TimelineEventService` 位于 `orchestration/service/session-service`，负责 timeline event append、active turn 更新和 live publish。
- `talos_types` 已经承载大量 SDK/服务端共享对象，但对象归属和命名需要收紧。

## 目标

1. 明确 Gateway 是 HTTP / JSON-RPC 之下的统一业务 API 门面。
2. 明确 Application Service 是 Gateway 下方的业务事务和持久化承接层。
3. 明确 AgentCoordinator 只负责 Agent 注册、路由、选择和分发。
4. 明确 AgentHarness 是 Agent Kernel 上方的包装层，保留现有名称。
5. 服务端和 SDK 对外共享对象统一放在 `talos_types`。
6. 对外 API 对象统一命名为 `XxxRequest` / `XxxResponse`。
7. 协议层只处理 HTTP / JSON-RPC / SSE / stdio envelope，不复制业务对象。
8. 数据库对象统一 `XxxRecord`，不外泄给 SDK 和协议层。
9. 压缩重复对象链，减少无意义来回转换。
10. 收敛 `TalosJsonValue` 到明确开放 JSON 边界。

## 非目标

- 不重写 Agent Kernel。
- 不重写 AgentHarness 内部执行逻辑。
- 不改变现有 HTTP / JSON-RPC / SDK 的能力范围。
- 不把本 spec 写成执行计划；实施阶段另用独立 plan。
- 不为旧命名保留长期 fallback；迁移期可以有短期 adapter。

## 目标分层

```text
Client / SDK / Web / Desktop / CLI
        |
        v
Transport Adapter
HTTP / SSE / JSON-RPC / stdio
        |
        v
Gateway
协议无关的统一业务 API 门面
        |
        v
Application Service
业务事务、DB 持久化、Agent 执行过程承接
        |
        v
AgentCoordinator
Agent 注册、路由、选择、分发
        |
        v
AgentHarness
Agent Kernel 上层包装
        |
        v
Agent Kernel
执行核心
        |
        v
Provider / Tools / Memory
```

## 分层职责

| 层 | 职责 | 不允许做的事 |
| --- | --- | --- |
| Transport Adapter | 处理 HTTP path/query/header、SSE event name、JSON-RPC method/id、stdio framing。只做协议 envelope 到 Gateway 共享对象的解析。 | 不定义业务同构对象；不做业务持久化；不直接调用 AgentHarness。 |
| Gateway | 暴露协议无关的统一业务 API；接收/返回 `talos_types` 对外对象；屏蔽 HTTP 与 JSON-RPC 差异。 | 不直接访问 DB；不运行事务；不直接 route Agent；不生成 kernel 输入。 |
| Application Service | 负责权限、幂等、session 校验、turn 状态、timeline、active turn、session progress/title 持久化；承接 Agent 执行返回事件。 | 不运行 model/tool loop；不做 Agent registry；不生成协议 envelope。 |
| AgentCoordinator | 管理 Agent registry；按 session/context 路由；选择目标 AgentHarness；分发执行请求。 | 不持久化 session progress/title；不访问 timeline store；不定义对外 API 对象。 |
| AgentHarness | 保留现有名称；包装 Agent Kernel；持有 provider、tools、memory、kernel config；提供简洁 Agent API。 | 不依赖 HTTP / JSON-RPC / DB record；不拥有持久化事务；不生成对外 envelope。 |
| Agent Kernel | 唯一 model/tool loop、stop reason、usage 和内部执行事件来源。 | 不感知 Gateway、Application Service、协议 envelope 或数据库 record。 |

## Gateway API 边界

Gateway 是 HTTP 和 JSON-RPC 共同使用的业务 API 门面。SDK 不应形成第三套业务协议；SDK 只封装 HTTP 或 JSON-RPC transport，并复用同一套 `talos_types` 对象。

Gateway 必须保持薄层：它只抽象多端共享的协议无关 API，不承接 DB 事务、Agent 执行编排或配置派生重逻辑。真正把 AgentCoordinator 执行结果与 DB 持久化、timeline、active turn、session title/progress 串起来的是 Application Service。

Transport 到 Gateway 之间的业务对象必须统一来自 `talos_types`。HTTP body/query/path、JSON-RPC `params`、SDK method params、Desktop/Web client params 只允许体现传输协议差异；进入 Gateway 前必须收敛为同一组 `XxxRequest` / `XxxResponse` / `XxxEvent` 对象。协议层不能为同一业务能力复制 `HttpXxxParams`、`JsonRpcXxxParams`、`SdkXxxRequest` 等同构对象。

TS SDK 和 Rust SDK 是多端共享 API 的语言封装，不是新的业务模型层。SDK 暴露的核心 Request / Response / Event / Domain Object 必须来自 `talos_types` 及其生成物；SDK 可以提供 transport client、方法封装、类型 re-export 和少量兼容 adapter，但不能为同一业务能力重新定义一套客户端专属数据结构。

目标 Gateway API 形态：

```rust
async fn start_turn(request: StartTurnRequest) -> TalosResult<StartTurnResponse>;
async fn retry_turn(request: RetryTurnRequest) -> TalosResult<RetryTurnResponse>;
async fn interrupt_turn(request: InterruptTurnRequest) -> TalosResult<InterruptTurnResponse>;
async fn submit_tool_result(
    request: SubmitToolResultRequest,
) -> TalosResult<SubmitToolResultResponse>;
async fn subscribe_timeline(
    request: SubscribeTimelineRequest,
) -> TalosResult<SubscribeTimelineResponse>;
async fn recover_timeline(
    request: RecoverTimelineRequest,
) -> TalosResult<TimelineRecoveryResponse>;
```

约束：

- HTTP handler 和 JSON-RPC runtime 都调用 Gateway。
- IM channel、SDK、Desktop、Web 等入口也应走同一 Gateway API，而不是各自直接调用 Coordinator 或内部 dispatch 对象。
- Gateway 调 Application Service。
- Gateway 不直接调用 AgentCoordinator 或 AgentHarness。
- Gateway 不定义 `GatewayRequest` / `GatewayResponse` 作为对外业务对象。
- Transport adapter 只允许做 envelope 解析和少量协议字段装配，例如 HTTP status/path/query/header、JSON-RPC method/id、SSE event name、stdio framing。
- 禁止在 HTTP、JSON-RPC、SDK、Desktop/Web client 之间复制一套结构相同但名字不同的业务对象。
- TS / Rust SDK 暴露的业务 API 对象必须 re-export 或生成自 `talos_types`；SDK 本地对象只能用于 transport 配置、client runtime 状态或明确标注的短期兼容 adapter。

## Application Service 边界

Application Service 是本重构最关键的承接层。它负责把 Gateway 统一 API 转成应用事务，并衔接 AgentCoordinator 和数据库状态。

`Application Service` 是层名，不是要求所有应用逻辑都写进一个单一文件或单一巨型 service。该层内部必须按业务领域拆分为清晰的 service 文件和 API，例如 session、turn、event、project、fs、model、memory、workspace、config、auth 等。对外由 `TalosApplicationService` 作为组装 facade 暴露稳定入口，但具体事务逻辑应落在对应领域 service 或对应领域的 `TalosApplicationService` impl 文件中，避免 `lib.rs` 和 Gateway 实现继续膨胀。

Application Service 负责：

- 请求前校验 session、权限、model、workspace、project。
- 保留 session 生命周期语义：客户端可以传入唯一 `session_id`；如果客户端未带则由服务端生成；每次 turn 请求都必须按 `session_id` 判断 session 是否已存在，存在则续用，不存在才创建。
- 处理 idempotency key。
- 分配或承接 request_id、turn_id、message_id。
- 检查 active turn 冲突。
- 调用 AgentCoordinator。
- 接收 Agent 执行过程返回的 `AgentEvent`。
- 写入 `TimelineEventRecord`。
- 更新 `ActiveTurnRecord`。
- 更新 session progress / title / usage。
- 如果为了新 turn 创建了 session，但 turn 未被接受或执行入口失败，必须回滚刚创建的 session 记录，避免留下无效 session。
- 生成对外 `TimelineEventEnvelope`。

Application Service 不负责：

- Agent routing。
- Agent registry。
- Kernel prompt/model/tool loop。
- HTTP status code 或 JSON-RPC id/method envelope。

Application Service 内部拆分约束：

- 新增应用事务能力时，优先放到对应业务域文件，例如 `session.rs`、`turn.rs`、`fs.rs`、`model.rs`。
- 领域 service API 命名必须和业务概念对齐，不能用模糊的 `handle`、`process`、`misc` 承接核心语义。
- `TalosApplicationService` 可以作为 facade 聚合依赖并暴露入口，但不应在 `lib.rs` 堆业务实现。
- DB record 到业务对象、业务对象到事件 envelope 的转换属于 Application Service 或其下属领域 service，不应泄漏到 Gateway、Transport 或 SDK。
- Gateway 只调用 Application Service 的领域 API，不直接依赖领域 service 内部存储、registry 或配置派生细节。

## AgentCoordinator 边界

AgentCoordinator 只表达 Agent 协调能力：

```text
Agent registry
Agent routing
Agent selection
Agent dispatch
```

目标输入输出：

```text
AgentDispatchRequest -> AgentResponse
AgentDispatchRequest -> stream AgentEvent
```

约束：

- `AgentDispatchRequest` 是内部对象，不进入 SDK/schema。
- AgentCoordinator 不持久化 session progress/title。
- AgentCoordinator 不生成 timeline cursor metadata。
- AgentCoordinator 不定义 `StartTurnRequest` 等对外对象。

## AgentHarness 边界

AgentHarness 名称保留。它是 Agent Kernel 上方包装层，对 AgentCoordinator 暴露简洁 API：

```text
AgentRequest -> AgentResponse
AgentRequest -> stream AgentEvent
```

AgentHarness 负责：

- 持有 provider、tool registry、memory、kernel config。
- 管理执行辅助状态，例如 active turn queue、cancel、memory flush。
- 把 `AgentRequest` 转成 kernel 内部 `InvocationContext` / turn input。
- 把 `KernelStreamEvent` 转成 `AgentEvent`。

AgentHarness 不负责：

- DB record 持久化。
- HTTP / JSON-RPC envelope。
- SDK 对象兼容。
- session progress/title 持久化。

## 对象命名规范

| 对象类别 | 命名 | 所属边界 |
| --- | --- | --- |
| 对外共享对象 | `XxxRequest` / `XxxResponse` / `XxxEvent` / `XxxEventEnvelope` | `talos_types` |
| 对外领域 DTO | `XxxInfo` / `XxxSnapshot` | `talos_types` |
| Service 内部事务对象 | `XxxCommand` / `XxxResult` | `application-service` |
| Coordinator 内部分发对象 | `AgentDispatchRequest` / `AgentDispatchResponse` | AgentCoordinator |
| AgentHarness 边界对象 | `AgentRequest` / `AgentResponse` / `AgentEvent` | `orchestration/agent` |
| 数据库存储对象 | `XxxRecord` | store / repository |
| Kernel 内部对象 | `InvocationContext` / `KernelEvent` / `KernelStreamEvent` / `KernelStepState` | `agent-kernel` |

对外 API 对象必须按 API 一一对应命名，不使用 `Params` 表达业务请求对象。

建议对外对象：

```text
StartTurnRequest / StartTurnResponse
RetryTurnRequest / RetryTurnResponse
InterruptTurnRequest / InterruptTurnResponse
SubmitToolResultRequest / SubmitToolResultResponse
SubscribeTimelineRequest / SubscribeTimelineResponse
TimelineEvent / TimelineEventEnvelope
SessionSnapshot / SessionHistorySnapshot
```

## 对象转换关系

请求方向：

```text
Protocol Envelope
  -> StartTurnRequest
  -> StartTurnCommand
  -> AgentDispatchRequest
  -> AgentRequest
  -> InvocationContext
```

响应方向：

```text
Kernel result
  -> AgentResponse
  -> Application Service
  -> StartTurnResponse
  -> Gateway
  -> Protocol Envelope
```

事件方向：

```text
KernelStreamEvent
  -> AgentEvent
  -> Application Service
  -> TimelineEventRecord
  -> TimelineEventEnvelope
  -> Gateway
  -> Protocol Envelope
```

规则：

- `StartTurnRequest` 是 SDK 和服务端共享的对外契约。
- `StartTurnCommand` 是 Application Service 内部事务对象，可按实现需要保留或省略。
- `AgentDispatchRequest` 只表达选择 Agent 所需的上下文。
- `AgentRequest` 只表达 AgentHarness 调用 kernel 所需的执行输入。
- `TimelineEventEnvelope` 中的 `event_id`、`event_seq`、`created_at_ms` 等 cursor metadata 只能由持久化层生成。
- 同一业务语义只能有一个 canonical 对象来源；如果对象字段基本相同，只是因为 HTTP、JSON-RPC、SDK 不同而重复定义，必须合并到 `talos_types`。
- 允许的转换只有两类：协议 envelope 到共享业务对象的解析转换，以及跨职责层的语义转换，例如 `StartTurnRequest -> AgentDispatchRequest -> AgentRequest`。
- 不允许的转换是同一层或同一语义对象在服务端、SDK、协议层之间来回包装和解包。
- SDK method 参数和返回值不能成为第三套业务对象；如果 SDK 需要更友好的方法名，方法签名仍应接收/返回 `talos_types` 的共享对象或其生成类型。

## 现有类型替换方向

| 当前类型 | 目标类型 |
| --- | --- |
| `TalosMessageRequest` | 已删除；主路径使用 `StartTurnRequest` |
| `TurnStartResponse` / `MessageCommandAck` | `StartTurnResponse` |
| `TalosMessageRetryRequest` / `TurnRetryParams` | 已删除；主路径使用 `RetryTurnRequest` |
| `TurnInterruptParams` | `InterruptTurnRequest` |
| `ToolDynamicResultSubmitParams` | `SubmitToolResultRequest` |
| `EventSubscribeParams` | `SubscribeTimelineRequest` |
| `EventSubscribeResponse` | `SubscribeTimelineResponse` |
| `GatewayRequest` / `GatewayResponse` | 已从代码主链路移除；内部 dispatch 改为 `AgentDispatchRequest` / `AgentDispatchResponse` |
| `OrchestratorRequest` | `AgentRequest` |
| `OrchestratorExecution` | `AgentResponse` |
| 内部 kernel 输入 | `AgentKernelRequest` |
| `TalosTimelineEvent` | 按边界拆为 `AgentEvent` 和对外 `TimelineEvent` |

## TalosJsonValue 收敛

`TalosJsonValue` 不应继续作为核心业务语义兜底。保留开放 JSON 能力，但必须语义化：

```text
ToolArguments
DynamicToolArguments
ToolExecutionResult
JsonContentValue
```

规则：

- 工具调用参数使用 `ToolArguments`。
- 动态工具参数使用 `DynamicToolArguments`。
- 工具执行结果使用 `ToolExecutionResult`。
- UI / timeline JSON 内容使用 `JsonContentValue`。
- 禁止用 `TalosJsonValue`、`serde_json::Value`、`HashMap<String, Value>` 或自由 metadata map 表达核心字段。
- SDK 不应向 UI 暴露 `{ value: unknown }` 包装。

## 迁移约束

1. 先新增标准对象和 adapter，再逐步迁移调用点。
2. 每次迁移只能保留一个 canonical 对象来源。
3. 旧类型可以短期 `From/TryFrom` 到新类型，但不能作为新代码入口。
4. HTTP 和 JSON-RPC 必须同时迁移到同一 Gateway API。
5. 每个阶段必须保持 cargo check、schema generation、TS SDK build 可通过。
6. 不为测试增加生产环境不需要的特殊分支。

## 验证策略

需要覆盖：

- Rust `talos_types` schema / TS generation。
- Gateway HTTP 与 JSON-RPC adapter contract。
- Application Service turn submit、retry、interrupt、dynamic tool result。
- Timeline event persistence、replay、recovery。
- Rust SDK / TS SDK type build。
- Web / Desktop 对 timeline envelope 的消费。
- App-server protocol e2e。

## 成功标准

- SDK 和服务端对外对象只来自 `talos_types`。
- 对外 API 全部使用 `XxxRequest` / `XxxResponse`。
- 协议层只处理 envelope，不复制业务对象。
- HTTP、JSON-RPC、SDK、Desktop/Web 对同一业务能力不得各自维护同构 `Params` / `Request` / `Response`。
- TS SDK 和 Rust SDK 的业务对象来自 `talos_types`，不能自造客户端业务 DTO。
- 服务端内部不得为同一业务语义保留多套中转对象；跨层转换必须能说明职责差异。
- Gateway 成为 HTTP / JSON-RPC 共享的业务 API 门面。
- Application Service 是唯一业务事务和持久化承接层。
- AgentCoordinator 不写 session progress/title，不访问 timeline store。
- AgentHarness 保留名称，并只作为 Kernel 上层包装。
- 旧 `TalosMessageRequest` / `TalosMessageRetryRequest` 已从主路径和对外协议删除；内部链路只保留职责差异明确的 `StartTurnRequest -> AgentDispatchRequest -> AgentRequest -> AgentKernelRequest`。
- `TalosJsonValue` 只用于明确开放 JSON 边界。
- SDK/UI 不再处理协议包装泄漏。

## Review Gap

2026-06-21 review 后确认当前实现仍未达到目标态，缺口状态如下：

- 已收敛：`Application Service` 不再依赖 `talos_gateway::Gateway`，执行入口改为 `TurnExecutor`，组装层使用 `AgentDispatchTurnExecutor` 连接 `Coordinator`。
- 已收敛：session title 写入迁到 `Application Service`，`Coordinator` 只暴露 agent 执行后的 title update。
- 已收敛：session delete / clear / compact 的 DB 写入迁到 `Application Service`，`SessionRuntime` 只表达运行时准备、clear 和 compact usage。
- 已收敛：`SessionRuntime` 的运行时动作接收 `TalosSession`，不再要求 `Coordinator` 通过 `session_id` 回查 DB。
- 已收敛：`AgentDispatchRuntime` / `TurnExecutor` 的 cancel 和 dynamic tool result 入口接收 `TalosSession`，Application Service 负责准备 session 后再调用 Coordinator。
- 已收敛：`Coordinator` 不再持有 `SessionStore` / `SessionManager`，不再实现旧 `SessionService`；`AgentDispatchRequest.session` 必须由 `Application Service` 预置，Coordinator 仅负责无 DB 的 agent 注册、路由和分发。
- 已收敛：collaboration agent 的 session 生命周期和输入执行都通过 `Application Service` 承接；工具层不再构造内部 dispatch 对象或直接调用 `Coordinator`。
- 已收敛：`AgentHarness` 对上主 API 已改为 `AgentRequest` / `AgentResponse` / `AgentEvent`，旧 `OrchestratorRequest` / `OrchestratorExecution` 从 Rust 代码中移除；Harness 内部 kernel 输入明确命名为 `AgentKernelRequest`。
- 已收敛：Agent registry 使用 `Agent` trait，不再用 `AgentOrchestrator` 命名表达可调度 Agent。
- 已收敛：删除未使用的 `CoordinatorResponse` / `CoordinatorStatus` / `RawAgentEvent` 导出，避免 Coordinator 暴露带 `TalosJsonValue` 兜底 payload 的半成品 API。
- 已收敛：`talos-gateway` core 的 `Gateway` trait 已改为 `start_turn` / `retry_turn` / `interrupt_turn` / `submit_tool_result` 具体业务 API，`ApplicationServiceGateway` 只调用 `Application Service`，不再持有 `AgentDispatchRuntime` 或暴露泛化 `GatewayRequest` / `GatewayResponse` 主入口。
- 已收敛：Gateway 具体实现已按业务域拆分为 turn / session / project 等文件，`api_impl` 只负责实现 `Gateway` trait 的薄代理，避免所有 API 堆在单个实现文件中。
- 已收敛：Application Service 开始按业务域拆分实现文件，`model.list` 已落到 `application-service/src/model.rs`，后续新增领域 API 不能继续堆进 `lib.rs`。
- 已收敛：HTTP handler 与 JSON-RPC app-server 的 turn command 主路径已复用 `Gateway` core 门面，包括 start / retry / interrupt / submit tool result。
- 已收敛：timeline subscribe / recover 已纳入 `Gateway` core 门面；session 校验和 cursor 解析由 `Application Service` 承接，HTTP/SSE 与 JSON-RPC runtime 只负责协议包装。
- 已收敛：`session.recover` 不再使用 app-protocol 本地 `TimelineRecoveryParams`，统一改用 `talos_types::RecoverTimelineRequest` 并同步 Rust/TS SDK 导出。
- 已收敛：Application Service 到 Coordinator 的内部执行接口已从 `RequestHandler` / `GatewayRequest` / `GatewayResponse` 改为 `AgentDispatchRuntime` / `AgentDispatchRequest` / `AgentDispatchResponse`，职责命名与 AgentCoordinator dispatch 语义对齐。
- 已收敛：HTTP 与 JSON-RPC app-server 的普通 session 资源 API 已纳入 `Gateway` core，包括 list/read/history/patch/delete/close/compact/clear；Gateway 仍只薄委托 `Application Service`。
- 已收敛：active turn snapshot 已上移到 `talos_types`，HTTP 与 JSON-RPC app-server 的 `session.active_turn` 已纳入 `Gateway` core，协议层不再为该查询直接消费 `ActiveTurnRecord`。
- 已收敛：timeline recover 的 active turn 映射已收回 `Application Service`，HTTP 与 JSON-RPC app-server 不再消费 `ActiveTurnRecord`。
- 已收敛：`TimelineRecoveryResponse` / `TimelineRecoverySnapshot` / `TimelineReplay` / `TimelineCursorStatus` 已上移到 `talos_types`，Gateway / HTTP / JSON-RPC / Rust SDK / TS SDK 使用共享 response 对象。
- 已收敛：简单 session resource 请求已上移到 `talos_types`，包括 read / delete / close / compact / clear / active_turn / history；Gateway 对这些 API 接收共享 request 对象。
- 已收敛：`SessionListRequest` / `SessionPatchRequest` 以及 session patch 子对象已上移到 `talos_types`，app-protocol 不再定义本地 `SessionListParams` / `SessionPatchParams`；HTTP / JSON-RPC / Gateway / Rust SDK / TS generated types 使用共享对象。
- 已确认：turn start 的 `session_id` 语义必须保留。客户端可自带唯一 `session_id`；Application Service 先查询，只有不存在时创建；如果新建 session 后 turn 未 accepted，必须回滚该 session 记录。
- 已收敛：`ProjectRecord` 明确为 DB record / store 边界对象，不再作为公共 SDK / protocol 类型导出；对外使用 `ProjectInfo` 以及 `ProjectCreateResponse` / `ProjectListResponse` / `ProjectReadResponse` / `ProjectPatchResponse` / `ProjectDeleteResponse`。
- 已收敛：`ProjectCreateRequest` / `ProjectListRequest` / `ProjectReadRequest` / `ProjectPatchRequest` / `ProjectDeleteRequest` 已由 `talos_types` 提供；app-protocol 不再定义本地 `Project*Params`；HTTP / JSON-RPC / Gateway / Rust SDK / TS generated types 使用共享对象。
- 已收敛：project create/list/read/patch/delete 已纳入 Gateway core，HTTP 与 JSON-RPC app-server 均通过 `ApplicationServiceGateway` 调用 Application Service，权限、可见性和路径状态仍由 Application Service 承接。
- 已收敛：`FsDirectoryReadRequest` / `FsDirectoryCreateRequest` 已由 `talos_types` 提供；app-protocol 不再定义本地 `FsDirectory*Params`；HTTP / JSON-RPC / Gateway / Rust SDK / TS generated types 使用共享对象。
- 已收敛：fs directory read/create 已纳入 Gateway core，HTTP 与 JSON-RPC app-server 均通过 `ApplicationServiceGateway` 调用 Application Service，路径安全、root 限制和权限仍由 Application Service / FsService 承接。
- 已收敛：`model.list` 使用 `talos_types::ModelListRequest`；`app.list` / `tool.list` 已确认不是当前对外业务 API，不再作为 Gateway / SDK 迁移目标。
- 已收敛：`model.list` 已纳入 Gateway core，HTTP 与 JSON-RPC app-server 均通过 `ApplicationServiceGateway` 调用 Application Service 的 model 领域 API。
- 已收敛：`tool.execute` 已确认不是当前对外业务 API；外部触发工具执行必须通过 `turn.start` prompt 进入 Agent 执行流，工具注册与执行归 Agent Kernel / AgentHarness 边界。
- 已收敛：`MemoryFileListRequest` / `MemoryFileListResponse` / `MemoryFileReadRequest` / `MemoryFileWriteRequest`、`WorkspaceDocListRequest` / `WorkspaceDocListResponse` / `WorkspaceDocReadRequest` / `WorkspaceDocWriteRequest`、`WorkspaceInfoRequest` / `WorkspaceInfoResponse` 已由 `talos_types` 提供；app-protocol 不再定义本地 `Memory*Params` / `Workspace*Params`。
- 已收敛：`MemorySearchRequest` / `MemorySearchResponse` / `MemorySearchResult` 已由 `talos_types` 提供；app-protocol `memory.search` 不再定义本地 `MemorySearchParams`；`talos_memory` 复用共享对象，Application Service 内部仅保留 `MemorySearchCommand` 作为服务层领域命令。
- 已收敛：`MemoryFileListRequest` / `MemoryFileWriteRequest` / `MemorySearchRequest` / `WorkspaceDocWriteRequest` 等共享 request 的 TS 生成 optional 语义已和服务端 `serde(default)` 对齐，客户端不需要为服务端默认字段补空值。
- 已收敛：`EventUnsubscribeRequest` / `EventUnsubscribeResponse` 已由 `talos_types` 提供；app-protocol 不再定义本地 `EventUnsubscribeParams`。
- 已收敛：`ConfigReadRequest` / `ConfigValueWriteRequest` / `ConfigBatchWriteRequest` / `ConfigReloadRequest` 已由 `talos_types` 提供；app-protocol 不再定义本地 `Config*Params`。
- 已收敛：`AuthLogoutRequest` / `AuthSessionReadRequest` / `AuthSessionStoreRequest` / `AuthSessionClearRequest` / `UserCurrentRequest` 以及 auth session snapshot 对象已由 `talos_types` 提供；app-protocol 不再定义本地 `Auth*Params` / `UserCurrentParams`。
- 已收敛：Rust SDK 不再导出本地 `MessageCommandAck` / `MessageCommandStatus`，`AppServerClient::start_turn` 主路径直接返回共享 `StartTurnResponse`。
- 已收敛：TS SDK 不再导出本地 `TalosMessageAck` / `TalosMessageCommandStatus`，HTTP/Web turn start 主路径直接返回共享 `StartTurnResponse`，retry 返回共享 `RetryTurnResponse`。
- 已收敛：TS SDK / Web HTTP adapter 的 memory search 查询对象已从 `MemorySearchParams` 改为 `MemorySearchQuery`，明确它是 HTTP query shape，不与共享 `MemorySearchRequest` 混用。
- 已收敛：Web 中 `JsonContentValue` / `DynamicToolArguments` 的本地手动 `{ value }` unwrap 已改为调用 TS SDK helper，协议 wrapper 知识不再散落在 Web UI 组件中。
- 已收敛：服务端内部 idempotency 的 `MessageCommandAck` 已改名为 `MessageCommandResult`，存储列已从 `ack_json` 迁移为 `result_json`；目标态代码只读写 `result_json`。
- 已收敛：Application Service 内部 memory list/search 输入已从 `MemoryListParams` / `MemorySearchParams` 改为 `MemoryListCommand` / `MemorySearchCommand`，Service 层内部不继续使用协议式 Params 命名。
- 已删除：`app.list` / `tool.list` / `tool.execute` 不再暴露为 app-protocol / SDK / Desktop IPC API，避免把不存在真实产品语义的 adapter 能力固化进 Gateway。
- 已收敛：Gateway 业务 API 返回统一 `XxxResponse`，不再由 HTTP / JSON-RPC adapter 为 project、session close、auth logout、auth login cancel、auth session store/clear 手工包装 `{ "success": true }`。
- 已收敛：`get_active_turn_handler` 序列化失败改为显式返回 `active_turn_serialize_failed`，不再 fallback 成业务 `failed` 状态。
- 约束：`tool.dynamic_result.submit` 保留为动态工具结果回填入口，属于 Core Turn API；它不代表外部可以直接执行任意工具。
- 已收敛：config / skills / plugin / memory / workspace / auth / user 资源 API 已纳入 Gateway core；HTTP 与 JSON-RPC app-server 只做协议 envelope、path/query/body 解析和共享 Request 组装，不再直接调用 Application Service 领域 service。
- 已收敛：cloud login / cloud session cache API 归属 Application Service 的 account 领域；Gateway 暴露 `auth.*` / `user.current`，HTTP 和 JSON-RPC 业务 API 不再自行持有 cloud login/cache 业务对象。
- 已收敛：HTTP auth middleware 的 cached token candidate、verified token touch、rejected token clear 已封装为 `AccountService` 方法，Transport 不再直接操作 cloud session cache manager。
- 已收敛：HTTP / Rust SDK / TS SDK / Web / Desktop / CLI / TUI 的 turn start/retry 主路径已删除旧 `TalosMessageRequest` / `TalosMessageRetryRequest` 兼容入口，直接使用共享 `StartTurnRequest` / `RetryTurnRequest`。
- 已收敛：TS SDK `types.ts` 的 session/model/config 对外业务 DTO 已改为 generated/shared aliases，SDK 不再自造这些业务对象。
- 约束：app-protocol 只应保留协议 envelope、initialize 和 server request 语义对象；新增业务 API 时不得重新引入本地同构 `Params`。
- 已收敛：JSON-RPC `ActorCredentialValidator` 的 cached token read、verified token touch、rejected token clear 已通过 `AccountService` 承接，鉴权 runtime 不再直接操作 cloud session cache manager。
- 已收敛：`TalosJsonValue` 不再由 app-protocol / Rust SDK / TS generated 对外导出；memory store / agent-kernel / runtime / TUI 已按工具参数、动态工具参数、工具结果和 JSON 内容改用语义 wrapper。
- 约束：后续新增 API 必须继续执行 `talos_types` 共享对象、Gateway `XxxRequest -> XxxResponse`、Transport 只做 envelope 的边界规则。

## 风险

- 一次性改名范围较大，容易影响 HTTP、JSON-RPC、SDK、Web、Desktop 多端。
- 如果 Gateway 过重，可能重新吞掉 Application Service 的事务职责。
- 如果 AgentCoordinator 继续持久化 session 状态，会再次形成跨层职责泄漏。
- 如果 `TalosJsonValue` 只改名不收束使用范围，强类型目标不会真正达成。
