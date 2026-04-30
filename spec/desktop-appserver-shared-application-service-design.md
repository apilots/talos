# Desktop App Server 与 talos-application-service 共享层设计方案

**版本**: v0.2  
**状态**: 已实现并完成 Phase 7 CR 前收敛

**最后更新**: 2026-04-30

> Phase 7 更新：`talos-application-service`、`talos-app-protocol`、`talos app-server`、
> Rust/TS app-server SDK、CLI/TUI 本地 app-server 接入、Electron Desktop shell 与协议回归
> 已落地。本文保留架构决策与最终边界，阶段执行细节以 `spec/plan/p1/PLAN.md` 及各 phase
> sub plan 的完成记录为准。
>
> 归档约定：本文使用“当前实现”描述已落地事实，使用“冻结约束”描述后续必须遵守的边界，使用“deferred”描述 CR 后
> follow-up，不再把早期方案中的“建议 / 目标形态”当作待实现事项。

---

## 1. 背景

Talos 已经具备 HTTP Server 与 Web UI 主链路，并且项目约束明确要求：

- `talos cli` 优先作为全量能力验证入口
- 服务端能力上线或重构时，优先补齐 CLI 接入和集成测试
- Web UI、TUI、CLI、未来 Desktop 不应重复承担服务端语义
- Kernel、Prompt、Tool、Memory、LLM Provider、Config 等核心模块必须复用现有实现

现在新增 Desktop App 方向后，客户端形态会变成：

- Web UI：浏览器环境，继续使用 HTTP API + SSE
- Desktop：Electron 壳 + 共享 Web UI + 本地 Talos Runtime
- TUI / CLI：本地优先，适合作为协议和端到端能力验证入口

参考 Codex 桌面端形态后，本方案选定：

1. 本地客户端使用 `JSON-RPC over stdio JSONL`
2. 现有 Web 继续使用 `HTTP API + SSE`
3. 两套协议入口下面必须统一到 `talos-application-service`；`coordinator` 收缩为下层 agent 协调组件
4. Web UI 与 Desktop UI 必须共享前端领域模型、状态机和主要组件

本文目标是把这套边界固化成可评审的架构方案。

---

## 2. 核心决策

### 2.1 本地协议使用 JSON-RPC over stdio JSONL

Desktop / TUI / CLI 本地模式通过启动 `talos app-server` 子进程通信：

```text
client process
    |
    | stdin/stdout line-delimited JSON
    v
talos app-server
```

协议采用标准 JSON-RPC 2.0：

- request: `jsonrpc + id + method + params`
- response: `jsonrpc + id + result` 或 `jsonrpc + id + error`
- notification: `jsonrpc + method + params`

每条消息独占一行 JSON，transport 不监听端口，不暴露本地 HTTP 服务。

### 2.2 HTTP 与 JSON-RPC 是两套入口，不是两套业务实现

允许存在两套 wire protocol：

```text
HTTP API + SSE
JSON-RPC over stdio JSONL
```

但它们下面必须统一调用同一组应用服务。当前代码里这组能力分散在几处：

- `talos_coordinator::CoordinatorImpl` 同时实现 `RequestHandler` 与 `SessionService`
- `gateway/http` 的 `ResourceAccessService` 包装 `SessionService`，承担 HTTP actor 访问控制
- `talos-server` 负责把 router、coordinator、gateway、HTTP server、auth、event store 等组装起来

这说明共享应用语义已经存在，但边界不对：`coordinator` 职责过重，HTTP 内部也沉淀了可复用的 resource access 逻辑。目标不是复制这些能力，而是把 session/project/access/config/memory/workspace docs 等多端共享能力迁入 `talos-application-service`，让 HTTP 与 JSON-RPC app-server 共同依赖它；`coordinator` 只保留下层 agent 协调、turn execution、stream mapping、agent cancellation 等职责。

禁止：

- JSON-RPC handler 直接调用 kernel 或 coordinator 私有实现，同时 HTTP handler 调另一套业务逻辑
- 为 Desktop 单独实现 session、config、memory、tool approval 业务规则
- 在 `gateway/http` 中继续新增可被 app-server 复用的 resource/service 逻辑
- 在协议层保存业务状态
- 在协议层引入灵活 metadata JSON 承载核心语义

### 2.3 Web UI 与 Desktop UI 共享

Desktop 不另造一套 UI 产品。目标是：

```text
shared frontend domain + shared React components
        |
        +-- Web runtime: HTTP + SSE transport
        |
        +-- Desktop runtime: Electron IPC + JSON-RPC stdio transport
```

UI 层只依赖统一的 `TalosClient` 接口，不感知底层 transport。

---

## 3. 目标与非目标

### 3.1 目标

1. 新增 `talos-application-service`，作为 HTTP 与 JSON-RPC app-server 共享的应用服务层。
2. 新增本地 `talos app-server`，使用 JSON-RPC over stdio JSONL。
3. 定义强类型 `talos-app-protocol`，覆盖 Desktop / TUI / CLI 本地协议。
4. 保持现有 HTTP Server 与 SSE 能力，并逐步改为调用同一 `talos-application-service`。
5. 抽象前端 `TalosClient`，让 Web UI 与 Desktop UI 共享领域模型和状态机。
6. 提供 `app-server-test-client`，作为 Desktop 之前的协议回归验证入口。
7. 明确配置、模型、session、turn、tool approval、memory、workspace docs 等能力的共享边界。

### 3.2 非目标

1. 本方案不废弃现有 HTTP API。
2. 本方案不要求 HTTP body 与 JSON-RPC params 完全同形。
3. 本方案不引入 gRPC、WebSocket 或 ConnectRPC。
4. 本方案不设计 Electron 视觉细节。
5. 本方案不重构 kernel planner 策略。
6. 本方案不引入新的 intent classifier 或基于前缀的任务分流。
7. 本方案不做旧客户端兼容包袱设计；项目尚未上线，优先保持最佳目标形态。

---

## 4. 总体架构

```text
┌──────────────────────────┐        ┌──────────────────────────┐
│ Web UI                   │        │ Desktop / TUI / CLI       │
│ Browser runtime           │        │ Local runtime             │
└─────────────┬────────────┘        └─────────────┬────────────┘
              │                                   │
              │ HTTP API + SSE                    │ JSON-RPC over stdio JSONL
              v                                   v
┌──────────────────────────┐        ┌──────────────────────────┐
│ talos-http-server         │        │ talos-app-server          │
│ thin protocol adapter     │        │ thin protocol adapter     │
└─────────────┬────────────┘        └─────────────┬────────────┘
              │                                   │
              └───────────────┬───────────────────┘
                              v
┌────────────────────────────────────────────────────────────────┐
│ talos-application-service                                      │
│ - session / project / access / config / model / memory API     │
│ - shared command/query/event semantics                         │
│ - owns multi-client application policy                          │
└─────────────┬───────────────────────┬──────────────────────────┘
              │                       │
              v                       v
┌──────────────────────────┐    ┌───────────────────────────────┐
│ coordinator               │    │ shared infra and domain        │
│ agent routing/execution   │    │ config / auth / storage        │
│ stream/cancel bridge      │    │ session-store / event-store    │
└─────────────┬────────────┘    └───────────────────────────────┘
              v
┌──────────────────────────┐
│ agent-kernel runtime      │
│ planner / prompt / tools  │
│ memory / llm provider     │
└──────────────────────────┘
```

关键边界：

| 层级 | 职责 | 禁止事项 |
|---|---|---|
| HTTP Server | HTTP decode、auth、SSE encode | 写业务规则 |
| App Server | stdio framing、JSON-RPC decode/encode、子进程生命周期 | 调 kernel 私有路径 |
| Application Service | 统一业务语义、资源权限、配置、会话、事件、调用 coordinator | 感知 HTTP/SSE/stdio framing |
| Coordinator | agent 路由、agent registry、turn execution、stream mapping、cancel | 资源权限、HTTP actor、配置/文件管理 |
| Kernel / Infra | 现有核心能力 | 为某个客户端写专用分支 |
| Frontend Client | 统一 TS 接口与状态模型 | UI 组件直接调用 transport |

---

## 5. 模块与目录

### 5.1 Rust 模块

当前实现按“application service 承载共享应用语义，gateway 承载 transport adapter”的方式分层。这里 `gateway` 在 Talos 语境下泛指 inbound protocol adapter，不限于网络网关；`app-server` 虽然是本地 stdio 子进程，但仍属于协议适配层，与 HTTP server 平级。

```text
talos-rs/
├── crates/
│   ├── app/
│   │   └── talos-server/              # composition root
│   ├── gateway/
│   │   ├── core/
│   │   ├── http/
│   │   ├── app-server/                # talos-app-server, stdio JSON-RPC
│   │   └── app-protocol/              # talos-app-protocol
│   └── orchestration/
│       ├── application-service/       # talos-application-service
│       ├── agent-kernel/
│       ├── coordinator/               # agent coordination only
│       └── session-store/
├── apps/
│   ├── cli/
│   ├── ui-tui/
│   └── app-server-test-client/
└── bin/
    └── talos/
```

说明：

1. `talos-server` 保留为 composition root，继续负责组装 storage、session store、event service、auth、router、application-service、coordinator、HTTP server、app-server。
2. `talos-application-service` 位于 `crates/orchestration/application-service`，承接多端共享的 session、project、resource access、config、model、memory、workspace docs、event replay 等应用语义。它不放在 `crates/app`，因为 `app` 只保留 composition root / runtime mode owner；application-service 是 HTTP 与 JSON-RPC adapter 共同复用的 orchestration use-case 层。
3. `talos_coordinator::CoordinatorImpl` 收缩为下层 agent 协调组件，只负责 agent registry、route-to-agent、turn execution、stream mapping、cancel、agent-side feedback bridge。
4. `gateway/http::ResourceAccessService` 迁入 `talos-application-service`，成为共享 resource access / authorization service；HTTP 只负责把 token/middleware 解析出的 identity 传入应用服务。
5. `talos-app-server` 放在 `crates/gateway/app-server`，与 `crates/gateway/http` 平级；它只实现 stdio transport、JSON-RPC dispatch、子进程生命周期和 back-pressure/cancellation 策略。
6. `talos-app-protocol` 放在 `crates/gateway/app-protocol`，只定义本地 JSON-RPC 协议 DTO、schema 导出、TS 类型生成。
7. HTTP Server 保留在 `gateway/http`，handler 应逐步改为调用 `talos-application-service`，不再在 HTTP crate 内沉淀共享 service 逻辑。

### 5.2 TypeScript 模块

现有 `talos-app/ts-sdk/packages/{core,browser}` 已经存在，不能另起一套平行目录。当前实现保留 `ts-sdk`，在其下扩展 transport 与共享 UI 边界：

```text
talos-app/
├── ts-sdk/
│   └── packages/
│       ├── core/                # shared domain types, generated protocol, client contracts
│       ├── browser/             # HTTP + SSE transport
│       └── app-server/          # JSON-RPC stdio and Electron IPC transport adapters
├── web/
└── desktop/
```

约束：

- `core` 不依赖 DOM、Electron、Node runtime
- `browser` 只实现 HTTP + SSE
- `app-server` 只实现 JSON-RPC stdio / Electron IPC bridge，业务方法内核放在 `core`
- UI 组件不直接 import transport 包
- `web/src/shared` 中可复用的 domain store、event reducer、UI 基础组件应逐步迁入共享包，但不在本 spec 中引入新的 `talos-app/packages/*` 平行体系

---

## 6. talos-application-service 设计

### 6.1 职责

迁移前代码阅读结论：共享应用语义已经存在，但分散在 `CoordinatorImpl`、`SessionManager`、`gateway/http::ResourceAccessService`、HTTP handler 和 `talos-server` 组装层中。

迁移前状态：

- `CoordinatorImpl` 实现 `RequestHandler`，承接 message / stream / cancel 主链路
- `CoordinatorImpl` 实现 `SessionService`，承接 session、project、tool approval、dynamic tool result 等能力
- `CoordinatorImpl` 内部 `SessionManager` 负责 session cache、effective root、project 校验、session title、progress 记录
- HTTP `ResourceAccessService` 包装 `SessionService`，负责 actor 访问控制和 user/admin 过滤
- `talos-server` 是 composition root，负责组装 coordinator、gateway、HTTP、auth、event store、llm provider、tool registry

当前实现：

- `talos-application-service` 位于 `crates/orchestration/application-service`，承接多端共享的应用语义
- `ResourceAccessService` 已从 HTTP crate 迁入 application service
- session/project/config/model/memory/workspace docs/admin resource policy 等已从 HTTP/coordinator 中迁入 application service
- `coordinator` 保留下层 agent 协调、routing、turn execution、stream mapping、cancel 等职责

`talos-application-service` 负责：

- session 创建、读取、更新、删除
- project CRUD 与 session-project 绑定
- resource access / actor visibility / admin guard
- turn 生命周期的应用层入口
- 用户输入提交
- 事件订阅与回放
- tool approval / dynamic tool result
- config 读取、写入、reload
- model list
- skills / apps / tools list
- memory markdown 文件能力
- workspace docs 能力
- auth/account 应用入口，其中 HTTP token/JWT 解析仍属于 transport 层
- admin users / admin sessions / admin projects 的 resource policy

它不负责：

- HTTP route path
- SSE frame 格式
- JSON-RPC method 字符串
- Electron IPC
- CLI 参数解析
- agent 内部规划、prompt 组装、tool execution

### 6.2 现有接口与当前边界

```rust
pub struct TalosServerRuntime {
    // Existing composition root output.
    application_service: Arc<TalosApplicationService>,
    coordinator: Arc<CoordinatorImpl>,
    http_server: HttpServer,
    app_server: Option<AppServer>,
}

pub struct TalosApplicationService {
    access: ResourceAccessService,
    account: AccountService,
    turns: TurnService,
    events: EventService,
    config: ConfigService,
    memory: MemoryService,
    workspace_docs: WorkspaceDocService,
    coordinator: Arc<dyn AgentCoordinator>,
}

pub struct CoordinatorImpl {
    router: Arc<dyn Router>,
    agents: Arc<RwLock<HashMap<String, Arc<dyn AgentOrchestrator>>>>,
}
```

接口轮廓：

```rust
pub trait TalosApplicationApi {
    async fn create_session(&self, actor: Actor, params: SessionCreateParams)
        -> TalosResult<TalosSession>;
    async fn list_sessions(&self, actor: Actor, query: SessionQuery)
        -> TalosResult<Vec<TalosSession>>;
    async fn start_turn(&self, actor: Actor, params: TurnStartParams)
        -> TalosResult<TurnStartResponse>;
    async fn subscribe_session_events(&self, actor: Actor, params: EventSubscribeParams)
        -> TalosResult<ApplicationEventStream>;
    async fn submit_tool_approval(&self, actor: Actor, params: ToolApprovalSubmitParams)
        -> TalosResult<()>;
}

pub trait AgentCoordinator {
    async fn run_turn(&self, command: AgentTurnCommand) -> TalosResult<GatewayResponse>;
    async fn run_turn_stream(
        &self,
        command: AgentTurnCommand,
    ) -> TalosResult<mpsc::Receiver<RawAgentEvent>>;
    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()>;
    async fn submit_tool_approval(&self, command: ToolApprovalCommand) -> TalosResult<()>;
    async fn submit_dynamic_tool_result(&self, command: DynamicToolResultCommand)
        -> TalosResult<()>;
}
```

`AgentCoordinator` 向上输出未编号的 raw agent stream。事件 ID、turn ID、session ID、持久化与 replay 统一由 `talos-application-service` 内的 event service 处理。事件持久化必须先于 SSE frame、JSON-RPC notification 发出；这样客户端即使在 `turn.start` 与 `event.subscribe` 之间存在订阅窗口，也能通过 `since_event_id` replay 补齐。event service 提供 live broadcast bus；HTTP SSE / app-server adapter 负责各自的 subscription task、session 过滤、慢消费者终止和 wire 编码。`coordinator` 不直接发布可被客户端订阅的最终事件，否则 `event.subscribe`、replay 与多订阅者会出现多套顺序源。

`turn.start` 的首批协议轮廓：

```rust
pub struct TurnStartParams {
    pub session_id: SessionId,
    pub input: UserInput,
    pub attachments: Vec<InputAttachment>,
    pub stream: StreamPreference,
    pub client_request_id: Option<ClientRequestId>,
}

pub enum UserInput {
    Text(TextUserInput),
}

pub struct TurnStartResponse {
    pub session_id: SessionId,
    pub turn_id: TurnId,
    pub accepted_at: Timestamp,
    pub started_event_id: EventId,
}
```

`turn.start` 不隐式创建订阅。需要实时事件的客户端应先调用 `event.subscribe`，或在收到 `TurnStartResponse.started_event_id` 后用 `since_event_id` replay。首批并发模型：单客户端可同时管理多个 session，但每个 session 只允许一个 active turn。若同一 session 已有 active turn，新的 `turn.start` 必须返回 `CONFLICT`。`turn.interrupt` 首批以 `session_id` 定位 active turn。

目标是拆清楚职责：

```text
HTTP/AppServer
  -> TalosApplicationService
  -> ResourceAccessService
  -> Session/Project/Config/Memory/Event services
  -> AgentCoordinator
  -> AgentOrchestrator
```

迁移期间 `CoordinatorImpl` 可以继续实现现有 `RequestHandler + SessionService + Coordinator` trait 以降低改造面，但新增协议入口不得直接依赖这些旧 trait。完成迁移后，`SessionService` 应归属 application service，`coordinator` 只实现 `AgentCoordinator`。

### 6.3 Command / Query / Event 语义

协议层可以有不同 DTO，但必须映射到统一 application service 方法。`Command` / `Query` 只作为文档里的语义分类，不对应必须落地的中间 enum，避免形成“协议 DTO -> command enum -> service params”的重复搬运层。

当前实现中的 application service 对外入口是强类型方法，例如：

```rust
create_session(actor, context, model, conf, project_id, current_directory)
list_sessions(actor, query)
get_session(actor, session_id)
patch_session(actor, session_id, patch)
delete_session(actor, session_id)
submit_turn(actor, session_id, request, idempotency_key)
cancel_active_turn(actor, session_id)
submit_tool_approval(actor, session_id, call_id, tool_name, approved, reason)
list_models()
get_llm(actor)
list_files(params)
search_files(params)
list_docs()
get_doc(kind)
put_doc(actor, kind, update)
```

语义分类：

| 分类 | 示例 | 实现约束 |
|---|---|---|
| Command | create / patch / delete session or project, start / interrupt turn, submit tool approval, write config / memory / workspace docs | 由协议 adapter 解码后直接调用 application-service 强类型方法 |
| Query | read / list session or project, model list, config read, memory search, workspace doc read, account get | 由 application-service 或其共享子服务返回强类型结果 |
| Event | persisted `TalosEventPayload` plus session event record metadata | 由 event service 持久化后通过 HTTP SSE 或 JSON-RPC notification fan-out |

冻结约束：

1. application-service 对外入口以强类型 service 方法为准。
2. 协议 DTO 不得绕过 application-service 直接调用 coordinator / kernel 私有路径。
3. 不新增 `ApplicationCommand` / `ApplicationQuery` 作为必经中间层。
4. `ApplicationEvent` 在 application-service 边界复用强类型 `TalosEventPayload`。
5. JSON-RPC `ApplicationEvent` 在 app-protocol 边界复用强类型 `TalosEvent`。
6. 事件 payload 不使用任意 metadata JSON。
7. 事件 ID、turn ID、session ID 的生成归 session event service / application-service 事件处理链路管理。

---

## 7. JSON-RPC App Server 协议

### 7.1 Framing

stdin/stdout 使用 JSONL：

```text
{...json message...}\n
{...json message...}\n
```

规则：

1. 每行是一条完整 JSON 消息。
2. stdout 只输出协议消息。
3. 日志必须写 stderr。
4. 非法 JSON 行返回错误或记录 stderr，不能污染 stdout。
5. 子进程 stdin EOF 后 app-server 应先取消该客户端拥有的 active turn，再有序退出。
6. app-server 内部所有 outgoing channel 必须有界。客户端读 stdout 过慢时，服务端必须通过 back-pressure 阻塞事件转发，不能无限缓存。
7. 如果客户端断开导致 stdout 写入失败，app-server 必须停止订阅事件，并取消该客户端已启动但未完成的 turn。
8. 高频 delta 事件允许在应用事件层保持完整语义，但 transport 层不得合并会影响 replay 的事件。

### 7.2 JSON-RPC 2.0 消息

Talos 本地协议采用标准 JSON-RPC 2.0。所有 request、response、notification 必须带 `jsonrpc: "2.0"` 字段。

```rust
pub enum JsonRpcMessage {
    Request(JsonRpcRequest),
    Response(JsonRpcResponse),
    Notification(JsonRpcNotification),
    Error(JsonRpcError),
}

pub struct JsonRpcRequest {
    pub jsonrpc: JsonRpcVersion,
    pub id: RequestId,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

pub struct JsonRpcResponse {
    pub jsonrpc: JsonRpcVersion,
    pub id: RequestId,
    pub result: serde_json::Value,
}

pub struct JsonRpcNotification {
    pub jsonrpc: JsonRpcVersion,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

pub struct JsonRpcError {
    pub jsonrpc: JsonRpcVersion,
    pub id: RequestId,
    pub error: JsonRpcErrorBody,
}
```

`serde_json::Value` 只允许存在于协议解码边界。进入 `ClientRequest` / `ServerNotification` 后必须转换为强类型对象。

错误码沿用 JSON-RPC 2.0 标准码：

| Code | 含义 |
|---:|---|
| `-32700` | Parse error |
| `-32600` | Invalid request |
| `-32601` | Method not found |
| `-32602` | Invalid params |
| `-32603` | Internal error |

业务错误使用实现定义区间 `-32099..=-32000`，并通过强类型 `AppServerErrorData` 表达上下文。

### 7.3 初始化流程

客户端启动 app-server 后必须先调用 `initialize`。

```text
client                       talos app-server
  |                                  |
  | initialize request               |
  |--------------------------------->|
  | initialize response              |
  |<---------------------------------|
  | initialized notification         |
  |--------------------------------->|
  |                                  |
```

初始化前收到非 `initialize` 请求必须返回 `Not initialized`。`initialized` notification 的方向是 client -> server；它表示客户端已完成本地初始化并确认可以接收后续 notification / server request。

当前初始化参数：

```rust
pub struct InitializeParams {
    pub client_info: ClientInfo,
    pub capabilities: ClientCapabilities,
    pub actor_credentials: ActorCredentials,
}

pub struct ClientInfo {
    pub name: ClientName,
    pub title: Option<String>,
    pub version: String,
}

pub struct ClientCapabilities {
    pub protocol_version: ProtocolVersion,
    pub supports_server_requests: bool,
    pub supports_event_replay: bool,
}

pub struct ActorCredentials {
    pub credential: ActorCredential,
}

pub enum ActorCredential {
    LocalLaunchToken(LocalLaunchToken),
    ExistingSessionToken(ExistingSessionToken),
}
```

HTTP 的 actor 来自 Bearer token / middleware。App-server 的 actor 必须在 `initialize` 阶段构造：Desktop main process、CLI 或 TUI 负责提供短生命周期 local launch token 或既有 session token；token 可以由父进程通过环境变量注入后再放入 `InitializeParams`，但不能通过命令行参数明文传递。`initialize` 校验失败时必须返回 `UNAUTHORIZED`，且 app-server 不得进入 initialized 状态。

### 7.4 Client Requests

当前已 freeze 覆盖：

| Method | Params | Response | 说明 |
|---|---|---|---|
| `initialize` | `InitializeParams` | `InitializeResponse` | 握手 |
| `session.create` | `SessionCreateParams` | `SessionCreateResponse` | 创建 session |
| `session.list` | `SessionListParams` | `SessionListResponse` | 列表 |
| `session.read` | `SessionReadParams` | `SessionReadResponse` | 读取详情 |
| `session.patch` | `SessionPatchParams` | `SessionPatchResponse` | 更新 |
| `session.delete` | `SessionDeleteParams` | `SessionDeleteResponse` | 删除 |
| `session.turns` | `SessionTurnsParams` | `{ data: TalosMessageResponse[] }` | 读取 session 历史 turn |
| `project.create` | `ProjectCreateParams` | `ProjectRecord` | 创建项目 |
| `project.list` | `ProjectListParams` | `{ data: ProjectRecord[] }` | 项目列表 |
| `project.read` | `ProjectReadParams` | `ProjectRecord` | 读取项目 |
| `project.patch` | `ProjectPatchParams` | `ProjectRecord` | 更新项目 |
| `project.delete` | `ProjectDeleteParams` | `{ success: true }` | 删除项目 |
| `turn.start` | `TurnStartParams` | `TurnStartResponse` | 提交用户输入 |
| `turn.interrupt` | `TurnInterruptParams` | `TurnInterruptResponse` | 中断 |
| `event.subscribe` | `EventSubscribeParams` | `EventSubscribeResponse` | 订阅 session 事件 |
| `tool.approval.submit` | `ToolApprovalSubmitParams` | `ToolApprovalSubmitResponse` | 审批 |
| `model.list` | `ModelListParams` | `ModelListResponse` | 模型列表 |
| `config.read` | `ConfigReadParams` | `ConfigReadResponse` | 读取配置 |
| `config.value.write` | `ConfigValueWriteParams` | `ConfigValueWriteResponse` | 写单项配置 |
| `config.batch.write` | `ConfigBatchWriteParams` | `ConfigBatchWriteResponse` | 批量写 |
| `config.reload` | `ConfigReloadParams` | `ConfigReloadResponse` | reload |
| `memory.file.list` | `MemoryFileListParams` | `MemoryFileListResponse` | memory 文件列表 |
| `memory.file.read` | `MemoryFileReadParams` | `MemoryFileReadResponse` | memory 文件读取 |
| `memory.file.write` | `MemoryFileWriteParams` | `MemoryFileWriteResponse` | memory 文件保存 |
| `workspace.doc.list` | `WorkspaceDocListParams` | `WorkspaceDocListResponse` | workspace docs 列表 |
| `workspace.doc.read` | `WorkspaceDocReadParams` | `WorkspaceDocReadResponse` | docs 读取 |
| `workspace.doc.write` | `WorkspaceDocWriteParams` | `WorkspaceDocWriteResponse` | docs 保存 |

仍保持 deferred 的 client methods：

| Method | 原因 |
|---|---|
| `tool.dynamic_result.submit` | 依赖 server-request 与动态工具结果回传边界稳定 |
| `admin.session.list` | 管理员 actor credentials 与本地权限产品边界仍需收口 |
| `admin.project.list` | 管理员 actor credentials 与本地权限产品边界仍需收口 |

#### 7.4.1 event.subscribe

`event.subscribe` 不是 JSON-RPC streaming response。它是一条普通 RPC：server 先返回 ack，之后通过 notification 推送事件。

```rust
pub struct EventSubscribeParams {
    pub session_id: SessionId,
    pub since_event_id: Option<EventId>,
    pub replay_limit: Option<EventReplayLimit>,
}

pub struct EventSubscribeResponse {
    pub subscription_id: EventSubscriptionId,
    pub session_id: SessionId,
    pub high_water_mark_event_id: Option<EventId>,
}

pub struct ApplicationEventEnvelope {
    pub subscription_id: EventSubscriptionId,
    pub event_id: EventId,
    pub turn_id: Option<TurnId>,
    pub event: TalosEvent,
}
```

Wire 层 `event` 字段使用共享强类型 `TalosEvent`。`ApplicationEventEnvelope` 顶层不重复存放 `session_id`；客户端路由字段来自 `envelope.event.session_id`。顶层字段只保留 subscription / replay / turn 相关 envelope metadata。

语义：

1. 同一 session 允许多个客户端同时订阅，包括 HTTP SSE 与 app-server JSON-RPC 同时订阅。
2. `since_event_id` 表示客户端最后确认看到的 event id；服务端必须从其后一个事件开始 replay。
3. `high_water_mark_event_id` 表示订阅建立时服务端已持久化的最新事件，可用于客户端判断 replay 是否追平。session 尚未产生任何事件时为 `None`，客户端应将其视为 replay 当前为空。
4. 订阅 ack 之后，JSON-RPC notification 的 `params` 使用 `ApplicationEventEnvelope`。
5. app-server 断开时自动释放该进程持有的 subscription；首批不要求跨 app-server 进程保留 subscription id。
6. event service 负责事件持久化、replay 查询和 live broadcast bus。
7. transport adapter 负责订阅任务、按 session 过滤 live bus、慢消费者处理、notification 编码和进程级 back-pressure。某个订阅者 lagged / slow consumer 不得阻塞全局 fan-out；app-server JSON-RPC 通过 `subscription.terminated` notification 终止该 subscription。

### 7.5 Server Notifications

服务端通过 notification 推送 application events。notification method 使用 `talos-app-protocol::events::canonical_method(TalosEventType)` 的当前冻结值，`params` 使用 `ApplicationEventEnvelope`，其中的 `event` 字段是共享强类型 `TalosEvent`：

| Method | Params | 来源 |
|---|---|---|
| `message.started` | `ApplicationEventEnvelope` | `TalosEventType::MessageStarted` |
| `message.reasoning.delta` | `ApplicationEventEnvelope` | `TalosEventType::MessageReasoningDelta` |
| `message.execution.summary` | `ApplicationEventEnvelope` | `TalosEventType::MessageExecutionSummary` |
| `message.output.delta` | `ApplicationEventEnvelope` | `TalosEventType::MessageOutputDelta` |
| `message.tool_call.started` | `ApplicationEventEnvelope` | `TalosEventType::MessageToolCallStarted` |
| `message.tool_call.args_delta` | `ApplicationEventEnvelope` | `TalosEventType::MessageToolCallArgsDelta` |
| `message.tool_call.queued` | `ApplicationEventEnvelope` | `TalosEventType::MessageToolCallQueued` |
| `message.tool_call.executing` | `ApplicationEventEnvelope` | `TalosEventType::MessageToolCallExecuting` |
| `message.tool_call.progress_delta` | `ApplicationEventEnvelope` | `TalosEventType::MessageToolCallProgressDelta` |
| `message.tool_call.completed` | `ApplicationEventEnvelope` | `TalosEventType::MessageToolCallCompleted` |
| `message.tool_call.pending_approval` | `ApplicationEventEnvelope` | `TalosEventType::MessageToolCallPendingApproval` |
| `message.tool_call.pending_dynamic_result` | `ApplicationEventEnvelope` | `TalosEventType::MessageToolCallPendingDynamicResult` |
| `message.completed` | `ApplicationEventEnvelope` | `TalosEventType::MessageCompleted` |
| `message.error` | `ApplicationEventEnvelope` | `TalosEventType::MessageError` |
| `subscription.terminated` | `SubscriptionTerminatedEnvelope` | subscription lifecycle |

### 7.6 Server Requests

部分流程需要服务端反向请求客户端，例如：

- tool 需要用户输入
- auth refresh
- 桌面端确认系统权限
- 文件变更审批

这类能力使用 server request，而不是 notification：

```text
server request with id
client response with same id
```

当前已冻结的 server request 协议类型：

| Method | Params | Response |
|---|---|---|
| `client.user_input.request` | `RequestUserInputParams` | `RequestUserInputResponse` |
| `client.auth.refresh` | `AuthRefreshParams` | `AuthRefreshResponse` |
| `client.approval.request` | `ClientApprovalRequestParams` | `ClientApprovalRequestResponse` |

如果客户端未声明 `supports_server_requests`，服务端遇到此类流程必须 fail fast。

### 7.7 Error Model

JSON-RPC error body 的 `code` 是规范整数码，不是字符串 enum。标准错误使用 JSON-RPC 2.0 标准码，业务错误使用 `-32099..=-32000`：

```rust
pub struct AppServerErrorCode(i64);

impl AppServerErrorCode {
    pub const PARSE_ERROR: Self = Self(-32700);
    pub const INVALID_REQUEST: Self = Self(-32600);
    pub const METHOD_NOT_FOUND: Self = Self(-32601);
    pub const INVALID_PARAMS: Self = Self(-32602);
    pub const INTERNAL_ERROR: Self = Self(-32603);

    pub const NOT_INITIALIZED: Self = Self(-32001);
    pub const ALREADY_INITIALIZED: Self = Self(-32002);
    pub const UNAUTHORIZED: Self = Self(-32003);
    pub const FORBIDDEN: Self = Self(-32004);
    pub const CONFLICT: Self = Self(-32005);
    pub const NOT_FOUND: Self = Self(-32006);
}

pub struct AppServerErrorBody {
    pub code: AppServerErrorCode,
    pub message: String,
    pub data: Option<AppServerErrorData>,
}

pub enum AppServerErrorData {
    Config(ConfigErrorData),
    Validation(ValidationErrorData),
    Turn(TurnErrorData),
    Tool(ToolErrorData),
}
```

禁止把 `data` 设计成任意 JSON metadata。新增错误上下文必须增加强类型 variant。

---

## 8. HTTP Server 适配方式

现有 HTTP Server 不应被替换，而是逐步变薄：

```text
HTTP request
    |
    v
route handler
    |
    | validate actor and convert HTTP DTO to shared params
    v
TalosApplicationService
    |
    v
application response / event stream
    |
    | convert to HTTP response / SSE frame
    v
HTTP client
```

### 8.1 HTTP 与 JSON-RPC DTO 关系

允许：

- HTTP DTO 保持 REST 风格
- JSON-RPC DTO 保持 method params 风格
- HTTP 字段命名保持现有 REST DTO 风格；JSON-RPC method 与 event id 统一使用点号 canonical id

必须：

- 两边映射到同一 application-service 参数
- 事件语义来自同一 `TalosEvent` / `TalosEventPayload`
- 同一业务错误映射自同一 application error
- 新能力上线时同时评估 HTTP、JSON-RPC、SDK、CLI、Web UI、Desktop 的覆盖

### 8.2 SSE 事件映射

事件 canonical id 统一使用点号，例如 `message.output.delta`。JSON-RPC notification 的 `method` 使用 app-protocol 冻结的 canonical method；HTTP SSE 与前端 reducer 应复用同一套 `TalosEventType` / `TalosEventPayload` 语义，不再维护另一套业务事件名。

`TalosEvent` 到 SSE 的映射：

```text
TalosEventType::MessageOutputDelta
    -> event: message.output.delta
    -> data: envelope carrying TalosEvent JSON

TalosEventType::MessageCompleted
    -> event: message.completed
    -> data: envelope carrying TalosEvent JSON
```

`TalosEvent` 到 JSON-RPC 的映射：

```text
TalosEventType::MessageOutputDelta
    -> method: message.output.delta
    -> params: ApplicationEventEnvelope JSON

TalosEventType::MessageCompleted
    -> method: message.completed
    -> params: ApplicationEventEnvelope JSON
```

这样前端状态机可以基于统一事件类型工作，而不是基于 transport 格式工作。

---

## 9. Frontend 共享设计

### 9.1 统一 Client Interface

前端 UI 只依赖统一接口：

```typescript
export interface TalosClient {
  initialize(params: InitializeParams): Promise<InitializeResponse>;
  createSession(params: SessionCreateParams): Promise<SessionCreateResponse>;
  listSessions(params: SessionListParams): Promise<SessionListResponse>;
  readSession(params: SessionReadParams): Promise<SessionReadResponse>;
  patchSession(params: SessionPatchParams): Promise<SessionPatchResponse>;
  deleteSession(params: SessionDeleteParams): Promise<SessionDeleteResponse>;
  startTurn(params: TurnStartParams): Promise<TurnStartResponse>;
  interruptTurn(params: TurnInterruptParams): Promise<TurnInterruptResponse>;
  submitToolApproval(params: ToolApprovalSubmitParams): Promise<ToolApprovalSubmitResponse>;
  subscribeSessionEvents(params: EventSubscribeParams): EventSubscription;
  readConfig(params: ConfigReadParams): Promise<ConfigReadResponse>;
  writeConfigValue(params: ConfigValueWriteParams): Promise<ConfigValueWriteResponse>;
  listModels(params: ModelListParams): Promise<ModelListResponse>;
}
```

Transport 实现：

```text
HttpSseTalosClient
    - browser runtime
    - HTTP request
    - fetch-based SSE

JsonRpcStdioTalosClient
    - Electron main process runtime
    - child_process spawn
    - stdin/stdout JSONL

ElectronRendererTalosClient
    - renderer runtime
    - talks to main process through preload IPC
    - exposes same TalosClient interface
```

### 9.2 Electron 进程模型

```text
┌──────────────────────────────────────────┐
│ Electron renderer                         │
│ shared React UI                           │
└──────────────────┬───────────────────────┘
                   │ typed IPC
                   v
┌──────────────────────────────────────────┐
│ Electron main                             │
│ JsonRpcStdioTalosClient                   │
│ app-server lifecycle manager              │
└──────────────────┬───────────────────────┘
                   │ stdin/stdout JSONL
                   v
┌──────────────────────────────────────────┐
│ talos app-server                          │
│ TalosApplicationService -> Coordinator     │
└──────────────────────────────────────────┘
```

约束：

1. Renderer 不直接获得 Node 权限。
2. Renderer 不直接读写本地文件系统。
3. Main process 只管理窗口、子进程、系统集成和 IPC。
4. 业务能力仍通过 `talos app-server`。
5. Desktop 专属能力如果需要暴露给 agent，也应建模成 tool 或 application-service 能力，不在 UI 层绕路。
6. app-server 崩溃后，main process 可以重启子进程；session、turn 历史与 replay 依赖现有 session store / session event store 恢复，不依赖 app-server 内存状态。
7. app-server 重启后 renderer 必须重新 `initialize`，再按 last seen event id 重新订阅 session events。

### 9.3 共享状态机

Web 与 Desktop 共享：

- session list store
- active session store
- turn lifecycle reducer
- event reducer
- tool approval state
- config state
- model list cache

Transport 只负责把 wire event 转成统一 frontend domain event：

```typescript
export type FrontendTalosEvent =
  | FrontendEventEnvelope<'message.started', TalosMessageStartedEvent>
  | FrontendEventEnvelope<'message.output.delta', TalosMessageOutputDeltaEvent>
  | FrontendEventEnvelope<'message.tool_call.started', TalosMessageToolCallStartedEvent>
  | FrontendEventEnvelope<'message.tool_call.completed', TalosMessageToolCallCompletedEvent>
  | FrontendEventEnvelope<'message.completed', TalosMessageCompletedEvent>
  | FrontendEventEnvelope<'message.error', TalosMessageErrorEvent>;

export interface FrontendEventEnvelope<TType extends string, TPayload> {
  type: TType;
  subscriptionId: EventSubscriptionId;
  eventId: EventId;
  sessionId: SessionId; // derived from envelope.event.session_id
  turnId?: TurnId;
  payload: TPayload;
}
```

---

## 10. CLI / TUI 定位

### 10.1 CLI First 验证路径

`talos cli` 仍然是服务端能力最先对齐的客户端。新增 app-server 后，CLI 应支持两种验证模式：

| 模式 | 用途 |
|---|---|
| HTTP mode | 验证 Web/remote HTTP Server 能力 |
| App-server mode | 验证本地 JSON-RPC app-server 能力 |

当前本地验证入口：

```text
talos app-server
talos --transport app-server app-server-smoke --scenario turn-smoke
cargo run -p talos-app-server-test-client resource-matrix
cargo run -p talos-app-protocol --example export_fixtures
```

### 10.2 TUI 迁移方向

TUI 可以先继续使用现有 SDK/HTTP 路径。待 app-server 稳定后，再切换为本地 app-server mode：

```text
TUI
  -> JsonRpcStdioTalosClient
  -> talos app-server
  -> TalosApplicationService
  -> Coordinator
```

迁移完成后，TUI 与 Desktop 会共享本地协议语义，CLI 负责两条入口的回归。

---

## 11. 配置治理

配置相关能力必须统一进入共享配置服务。当前 HTTP 已经通过 `LlmConfigService` 管理 admin LLM 配置；app-server 不应另写配置路径：

```rust
pub struct ConfigService {
    config_manager: Arc<ConfigManager>,
}

impl ConfigService {
    pub async fn read(&self, params: ConfigReadParams) -> TalosResult<ConfigReadResponse>;
    pub async fn write_value(
        &self,
        params: ConfigValueWriteParams,
    ) -> TalosResult<ConfigValueWriteResponse>;
    pub async fn batch_write(
        &self,
        params: ConfigBatchWriteParams,
    ) -> TalosResult<ConfigBatchWriteResponse>;
    pub async fn reload(&self, params: ConfigReloadParams) -> TalosResult<ConfigReloadResponse>;
}
```

约束：

1. 默认行为与开关仍由 `config` 模块管理。
2. 请求级覆盖必须受配置开关和白名单约束。
3. 不新增只搬运参数的私有 options 结构体。
4. 多个配置型标量参数应收敛为配置对象。
5. 派生值在临近使用点计算，不层层透传。

---

## 12. Auth 与本地安全边界

### 12.1 HTTP

HTTP 保持现有鉴权模型：

- Bearer token
- browser-friendly auth flow
- SSE 使用 fetch-based streaming 携带 header

### 12.2 App Server

本地 app-server 不暴露端口，安全边界主要来自：

- 父进程持有子进程 stdin/stdout
- app-server 生命周期绑定客户端进程
- stdout 只输出协议消息
- stderr 输出日志

App-server 不自行发明身份。父进程必须在启动后通过 `initialize.actor_credentials` 传入 actor credentials，app-server 校验后构造统一 `Actor` 并传给 `TalosApplicationService`。推荐方式是父进程通过环境变量提供短生命周期 local launch token，再由客户端在 initialize 请求中提交；禁止通过命令行参数传递明文凭据。

仍需注意：

1. app-server 不能因为是本地就绕过 config policy。
2. tool approval、sandbox、network access 仍按统一策略执行。
3. Desktop main process 不应给 renderer 开任意命令执行能力。
4. app-server 启动参数必须可审计，不能允许任意未验证 metadata 改变安全策略。
5. app-server 初始化失败不得降级为匿名 actor 或 admin actor。

---

## 13. 协议版本与类型生成

`talos-app-protocol` 的当前生成命令：

```text
cargo run -p talos-app-protocol --example export_fixtures
```

该命令会同时刷新：

- `talos-rs/crates/gateway/app-protocol/schemas/app-protocol.schema.json`
- `talos-app/ts-sdk/packages/core/src/generated/app-protocol.ts`

当前结构：

```rust
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}
```

版本规则：

1. 删除字段、改变字段语义、改变 method 行为，提升 major。
2. 新增可选字段、新增 method、新增 notification，提升 minor。
3. 初始化阶段返回 server protocol version。
4. 客户端初始化时声明支持范围。
5. v1 之前 `protocol_version` 只做断言和生成物追踪，不做能力降级或旧版本兼容分支。
6. v1 之前如发生破坏性变化，必须提升 major，并更新 schema fixture 和 changelog 条目。这里不做旧版本兼容分支，但仍要让生成物追踪、协议断言和实现者预期保持一致。

类型生成约束：

1. Rust 是协议类型真源。
2. TypeScript 类型由 Rust 协议 crate 生成或校验。
3. CI 校验生成产物与源码一致。
4. 不允许手写 TS 类型长期漂移。

---

## 14. 测试方案

### 14.1 Rust 单元测试

覆盖：

- JSON-RPC message parse/serialize
- request method 到强类型 enum 的映射
- notification 序列化
- error model
- application-service command/query 映射
- `ResourceAccessService` actor 过滤和权限校验迁移后的共享行为
- HTTP handler 到 `TalosApplicationService` 的转换
- `TalosApplicationService` 到 `AgentCoordinator` 的 turn execution 调用
- malformed JSONL 行处理
- stdin EOF / stdout write failure 对 active turn 的 cancellation 行为

### 14.2 App Server E2E

新增 `app-server-test-client`，流程：

```text
1. spawn `talos app-server`
2. send initialize
3. create session
4. start turn
5. receive turn started / delta / completed notifications
6. read config
7. list models
8. submit tool approval mock flow
9. malformed line returns parse error and keeps process healthy
10. multi session interleaved notifications preserve per-session ordering
11. close stdin and verify active turn cancellation + app-server exit
12. restart app-server and replay session events from last seen event id
13. two subscribers on the same session receive the same ordered event envelope sequence
14. second `turn.start` on a session with active turn returns `CONFLICT`
```

### 14.3 Transport 等价性契约测试

新增共享契约测试，防止 HTTP 与 app-server 逐步漂移：

```text
same TalosApplicationService instance
    |
    +-- HTTP adapter scenario
    |
    +-- JSON-RPC app-server adapter scenario
```

覆盖：

- 同一 actor 创建 session 的语义等价
- 同一 `TurnStartParams` 经过 HTTP DTO 与 JSON-RPC params 后进入同一 application-service 参数
- session event replay 的 event id、event type、payload 语义等价
- tool approval 的错误码与状态转换等价
- config/model/memory/workspace docs 的成功响应与业务错误等价

### 14.4 HTTP 回归

现有 HTTP SSE 回归必须继续覆盖：

- session create/list/read/patch/delete
- message submit ack
- session event replay
- tool approval
- config read/write
- memory file read/write

### 14.5 前端测试

前端重点测试：

- `TalosClient` contract tests
- `HttpSseTalosClient` transport tests
- `JsonRpcStdioTalosClient` transport tests
- shared event reducer tests
- Web runtime 和 Desktop runtime 的 client injection

### 14.6 质量门禁

所有阶段完成后至少执行。`tests/integration/run_e2e.sh` 只在最终 hardening / rollout 阶段执行，不作为每个阶段的必跑项：

```text
cargo test -p talos-app-protocol
cargo test -p talos-application-service
cargo test -p talos-coordinator
cargo test -p talos-app-server
cargo test -p talos-gateway-http
cargo test -p talos-client
tests/integration/run_e2e.sh
```

如果前端接入：

```text
pnpm --dir talos-app lint
pnpm --dir talos-app test
pnpm --dir talos-app build
```

---

## 15. 分阶段实施计划

详细执行计划已拆分到独立目录 [spec/plan/p1/PLAN.md](plan/p1/PLAN.md)。本节只保留阶段摘要；具体子任务、测试、风险和完成记录以各 phase sub plan 为准。

### 阶段 1: Application Service 提取与 Coordinator 瘦身

**目标**: 新增 `talos-application-service`，把现有 `CoordinatorImpl` / `SessionManager` / `ResourceAccessService` / HTTP handler 中的共享应用语义迁入该层，并把 `coordinator` 收缩为下层 agent 协调组件。

这是实质重构，不是增加薄封装。当前 `coordinator.rs` 与 `session.rs` 已经承载大量 session/turn/project/title/root/progress 语义，HTTP 内也沉淀了 resource access 和部分 service 逻辑。Stage 1 应先建立 application-service 边界，再按职责迁移，满足单文件不超过 1000 行的维护约束。

**成功标准**:

- 明确 `talos-server` 继续作为 composition root
- 新增 `crates/orchestration/application-service`
- `ResourceAccessService` 从 `gateway/http` 迁入 application-service
- session lifecycle、project、resource access、config/model facade、memory/workspace docs facade 归 application-service
- `CoordinatorImpl` 只负责 agent registry、routing、turn execution、stream mapping、cancel、agent feedback bridge
- HTTP session / project / message / approval handler 通过 `TalosApplicationService` 进入共享路径
- 现有 HTTP 回归通过

**测试**:

- `cargo test -p talos-application-service`
- `cargo test -p talos-coordinator`
- HTTP handler tests

**状态**: 已完成

### 阶段 2: App Protocol

**目标**: 建立强类型 `talos-app-protocol`。

**成功标准**:

- 定义 `ClientRequest`、`ServerNotification`、`ServerRequest`
- 支持 TypeScript 与 JSON Schema 生成
- 定义 protocol version 与 error model

**测试**:

- protocol serialize/deserialize
- schema fixture 校验
- generated TS fixture 校验

**状态**: 已完成

### 阶段 3: Stdio App Server

**目标**: 实现 `talos app-server`。

**成功标准**:

- 支持 initialize
- 支持 session create/list/read
- 支持 `turn.start`、`event.subscribe` 与事件 notification
- 支持 `config.read` 与 `model.list`
- stdout 只输出协议消息

**测试**:

- app-server unit tests
- app-server-test-client smoke

**状态**: 已完成

### 阶段 4: CLI/TUI 本地验证入口

**目标**: 让 CLI/TUI 至少具备 app-server 验证能力。

**成功标准**:

- CLI 能用 app-server mode 跑通基础会话
- app-server-test-client 纳入集成测试
- TUI 接 app-server 的可行性 spike 完成，并给出 pass/fail 结论

**测试**:

- CLI smoke
- TUI smoke

**状态**: 已完成

### 阶段 5: Desktop 与 Web UI 共享接入

**目标**: 建立 Desktop shell，并复用 Web UI 主体。

**成功标准**:

- 新增 `talos-app/ts-sdk/packages/app-server`，提供 stdio JSON-RPC transport primitives 与 Electron main / renderer IPC bridge
- `JsonRpcStdioTalosClient` 通过 shared `TalosClient` contract tests
- Electron main process 能启动 app-server
- renderer 通过统一 `TalosClient` 工作
- Web UI 继续使用 HTTP/SSE transport
- 两端共享 event reducer 和主要页面组件

**测试**:

- frontend contract tests
- desktop smoke
- Playwright Web UI 回归

**状态**: 已完成

---

## 16. 风险与应对

| 风险 | 影响 | 应对 |
|---|---|---|
| application-service 抽象过大 | 变成上帝对象 | 按 session、turn、access、config、memory 等子 service 拆分 |
| coordinator 瘦身范围被低估 | Stage 1 变成长期阻塞 | 以现有测试为保护网，按职责迁移，不同时改变协议语义 |
| ResourceAccessService 留在 HTTP | app-server 复制权限逻辑 | 迁入 application-service，HTTP/app-server 只提供 actor identity |
| HTTP 与 JSON-RPC DTO 漂移 | 前端状态机难共享 | 统一 application domain 与事件模型，CI 校验生成类型 |
| coordinator 输出最终客户端事件 | replay 与多订阅者顺序源分裂 | coordinator 只输出 raw agent stream；event service 统一编号、持久化、replay 与 live broadcast bus；adapter 负责订阅过滤和 wire fan-out |
| app-server stdout 被日志污染 | 协议解析失败 | stdout 专用于 JSONL，日志全部 stderr |
| Desktop 绕过 app-server | 形成私有业务能力 | Electron main 只做生命周期和系统集成 |
| stdout back-pressure 失控 | 高频 delta 导致内存膨胀 | bounded channel + 写入失败取消客户端 active turn |
| Desktop 子进程崩溃 | UI 与 runtime 状态脱节 | main process 重启 app-server，基于 session event replay 恢复 |
| SSE 与 JSON-RPC event 命名不一致 | 前端 reducer 分裂 | 统一使用点号 canonical event id |
| 配置写入分裂 | 行为不可预测 | config service 是唯一入口 |
| 本地协议缺少反向请求 | approval/auth 等流程受限 | 首批设计 server request |
| app-server actor 来源不清 | 共享 access service 无法复用 | initialize 阶段校验 actor credentials，失败即 `UNAUTHORIZED` |

---

## 17. 待评审问题

已关闭决策：

1. 新增 `talos-application-service`，作为 HTTP 与 JSON-RPC app-server 共享应用层。
2. 事件 canonical id 统一使用点号，HTTP SSE event、JSON-RPC method、frontend event type 保持一致。
3. TypeScript 侧保留现有 `talos-app/ts-sdk`，共享内核使用 `core`，JSON-RPC / Electron adapter 使用 `app-server`，不新建平行 `talos-app/packages` 体系。
4. `coordinator` 收缩为下层 agent 协调组件；session/project/access/config/memory/workspace docs 等迁入 application-service。
5. `talos-app-server` 放在 `crates/gateway/app-server`，与 `crates/gateway/http` 平级。
6. `gateway` 在本方案中泛指 inbound protocol adapter，不限于网络网关。
7. `coordinator` 只输出未编号 `RawAgentEvent`；event service 统一生成 event id、持久化、发布与 replay。
8. `event.subscribe` 是一次 RPC ack + 后续 notification，允许 HTTP 与 app-server 多订阅者同时订阅同一 session。
9. 首批并发模型为单客户端多 session、每 session 单 active turn。
10. App-server actor 在 `initialize.actor_credentials` 中注入并校验，失败不得进入 initialized 状态。

仍需跟进：

1. Desktop 本地 account profile 与现有用户配置的产品化选择；这不影响 actor 注入机制，只影响 credentials 来源。
2. `AppServerClient::send_message_events` 已演进为后台 stdout reader + subscription 分发模型，CLI/TUI app-server 路径可增量渲染事件。
3. `tool.dynamic_result.submit`、`admin.session.list`、`admin.project.list` 的 protocol freeze 与 parity 回归。

---

## 18. 结论

本方案的核心不是新增一套 Desktop 后端，而是把 Talos 现有分散的应用能力收束到 `talos-application-service`，让 HTTP 与 JSON-RPC 共享同一套服务语义：

```text
Protocol adapters are thin.
Application semantics are shared.
Frontend domain model is shared.
Transport is replaceable.
```

当前目标形态：

```text
Web UI       -> HTTP + SSE           -> TalosApplicationService -> Coordinator
Desktop UI   -> JSON-RPC stdio       -> TalosApplicationService -> Coordinator
TUI / CLI    -> JSON-RPC stdio       -> TalosApplicationService -> Coordinator
```

这样可以同时满足：

- Web 继续保留浏览器友好协议
- Desktop 获得稳定本地 IPC
- CLI First 验证入口增强
- 服务端业务语义不分裂
- Web UI 与 Desktop UI 最大化共享
