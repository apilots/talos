# TUI / CLI Embedded 与多端 SDK Public API 架构设计

Status: draft
Owner: agent
Created: 2026-06-24
Updated: 2026-06-27
Related: `refactor-027-core-api-object-boundary`, `refactor-028-orchestration-store-service-layout`

## 背景

Talos 已经在 Gateway / Service / Store / Agent 执行链路上完成了较多前置分层工作。Gateway Core 作为协议无关的业务 API 门面，对外承接统一的 request / response / event / error 语义；Service 层负责业务事务和持久化编排；Store 层负责 record 和持久化访问；Coordinator / Agent / Kernel 负责执行能力。

当前还存在一个关键架构缺口：不同端形态通过不同 transport 访问同一套能力，但 SDK public API contract 还没有被明确标准化。TUI / CLI 当前主要通过 Rust SDK 的 `AppServerClient` 连接外部 app-server；Desktop 通过 TS SDK 连接 Electron 主进程托管的 app-server；app-server 当前已经是 stdio JSON-RPC API，未来演进成 MCP server 形态相对自然。如果没有统一 public API contract，`AppServerClient`、embedded client、TS SDK、Desktop adapter、测试工具会随着时间产生方法命名、错误映射、事件流、默认参数和状态语义漂移。

本设计的核心目标不是简单把 TUI 从 app-server 改成 embedded，而是建立一套长期可维护的多端 SDK 架构：不同端可以选择不同运行形态，但必须共享同一套服务端 runtime 能力、同一套公共 API 语义、同一套行为测试和同一套对象模型。

## 设计动机

Talos 有多个本地入口形态：

1. TUI / CLI：本地终端产品形态，目标是一个 `talos` 命令即可使用，默认不依赖外部 app-server。
2. Desktop：桌面产品形态，通过 TS SDK 和 Electron 主进程托管的 app-server 访问本地 runtime。
3. Rust app-server test / diagnostics：用于验证 stdio JSON-RPC app-server 协议和真实服务端行为。
4. Future MCP server shape：app-server 当前已经是 stdio JSON-RPC API，未来演进成 MCP server 形态相对自然，但仍需要补充 MCP capability、tool/resource/prompt 语义映射。

这些入口不应该导致多套业务 API。它们本质上都在访问 Talos 服务端 runtime 提供的同一套能力，只是 transport、进程边界、生命周期归属不同。

因此，本设计把“统一 public API contract”作为第一原则：

```text
Shared Gateway/API Contract
  -> Rust SDK public contract
      -> AppServerClient
      -> EmbeddedServerClient
  -> TS SDK generated / wrapped contract
      -> Desktop app-server client
      -> Browser / HTTP client
```

## 当前实现事实

当前仓库中的关键事实如下：

1. Rust SDK crate 当前位于 `talos-rs/apps/sdk`，包名为 `talos-sdk`。
2. Rust SDK 当前主要公开 `AppServerClient`、`AppServerClientConfig` 和 shared type re-export。
3. TUI 在 `talos-rs/apps/ui-tui` 内保留 UI-facing `GatewayClient` trait，并通过泛型 SDK adapter 连接 `AppServerClient` 或 `EmbeddedServerClient`。
4. CLI 在 `talos-rs/apps/cli` 中通过 Rust SDK `GatewayClient` contract 调用本地 runtime，默认 embedded，显式 app-server 模式才实例化 `AppServerClient`。
5. app-server server crate 位于 `talos-rs/crates/gateway/app-server`，binary 位于 `talos-rs/bin/app-server`。
6. app-server 是 Gateway 之上的本地 stdio JSON-RPC 能力服务器，处理 framing、method dispatch、response envelope、notification stream 和 subscription runtime。
7. `talos-base` 位于 `talos-rs/crates/app/talos-base`，已经提供 `TalosApplicationRuntime` 和 `build_application_runtime` 等进程内 runtime 构建能力。
8. Gateway Core 位于 `talos-rs/crates/gateway/core`，提供协议无关 Gateway trait / service gateway 能力。
9. Desktop 使用 TS SDK app-server package，通过 Electron 主进程托管的 app-server 访问本地 runtime。
10. TS SDK 基于服务端 JSON-RPC / app protocol 生成和封装类型，Desktop 不使用 Rust SDK client。
11. `client_kind` 当前主要存在于 Cloud Login 协议对象中，取值为 `web | desktop | tui | cli`，不是全局 request metadata。

## 目标

1. 建立统一 SDK public API contract，避免不同端和不同 transport 长期漂移。
2. Rust SDK 对本地 Rust clients 提供一致 API：`AppServerClient` 和 `EmbeddedServerClient` 必须实现同一套 public contract。
3. TUI / CLI 默认使用 `EmbeddedServerClient`，在当前进程内构建 runtime，不要求用户启动或指定 app-server。
4. TUI / CLI 不提供 app-server transport 选择，不读取 app-server binary 参数或环境变量。
5. Rust app-server test / diagnostics 工具可继续直接使用 `AppServerClient` 验证 stdio JSON-RPC app-server。
6. Desktop 继续使用 TS SDK app-server path，不使用 Rust SDK client。
7. app-server 定位为本地 stdio JSON-RPC 能力服务器，不是 Desktop 专用后端。
8. Gateway/Core、Service、Store、Coordinator、Agent 分层继续作为唯一业务语义来源。
9. embedded 和 app-server 两条路径不能产生两套业务语义、两套错误语义或两套事件流语义。
10. 通过共享 contract tests 保证 `AppServerClient`、`EmbeddedServerClient` 和 TS SDK 语义长期一致。

## 非目标

1. 不要求 TS SDK 在本轮支持 embedded mode。
2. 不移除 app-server。
3. 不把 app-server 等价声明为 MCP server。
4. 不让 embedded client 复用 JSON-RPC 协议自调用自身。
5. 不在 TUI / CLI command handler 中散落 transport 判断。
6. 不让 Desktop 使用 Rust SDK client。
7. 不为不同端复制 request / response / event / error 对象。
8. 不把 Store `XxxRecord`、Service 内部对象或 runtime 内部对象外泄到 SDK / UI。

## 总体架构

目标架构分为多个入口形态，但共享同一组 Gateway/API 语义：

```text
TUI / CLI
  -> Rust SDK public API contract
    -> EmbeddedServerClient
      -> TalosApplicationRuntime
        -> Gateway Core
          -> Talos Service
            -> Store / Coordinator / Agent

Rust app-server test / diagnostics
  -> Rust SDK public API contract
    -> AppServerClient
      -> app-server stdio JSON-RPC
        -> Gateway Core
          -> Talos Service
            -> Store / Coordinator / Agent

Desktop
  -> TS SDK generated / wrapped from app protocol
    -> app-server stdio JSON-RPC
      -> Gateway Core
        -> Talos Service
          -> Store / Coordinator / Agent

Future MCP server shape
  -> app-server JSON-RPC server facade with MCP-compatible capability mapping
    -> Gateway Core
      -> Talos Service
        -> Store / Coordinator / Agent
```

关键原则：

1. Gateway/Core 之下的业务链路只有一套。
2. SDK public API contract 是端侧长期稳定契约。
3. transport 只决定调用路径，不决定业务语义。
4. app-server 是本地 stdio JSON-RPC 能力服务器，可供 Desktop、Rust app-server 测试/诊断工具和其他本地协议入口使用。
5. embedded 是 Rust 进程内 runtime 调用形态，不经过 JSON-RPC 自调用。
6. Desktop 使用 TS SDK，不使用 Rust SDK 的 `AppServerClient` 或 `EmbeddedServerClient`。

## Public API Contract

`talos-sdk` 和 `talos-sdk-embedded` 可以是不同 crate，但不能是两套 API。两者必须共享同一个 public API contract。

这个 contract 包括：

1. 强类型 `GatewayClient` trait 或等价 facade。
2. 与 Gateway API 对齐的强类型方法。
3. 共享 request / response / event / domain object。
4. 共享错误类型和错误映射规则。
5. 共享 stream abstraction，例如 `TimelineEventStream`。
6. 共享 shutdown / lifecycle 语义。
7. 共享 behavior tests。

Rust 侧不建议只暴露一个弱类型接口：

```rust
async fn request(request: XRequest) -> TalosResult<XResponse>;
```

该接口可以作为底层协议测试/diagnostics 能力保留，但 TUI / CLI 不应依赖它作为主要业务 API。Rust SDK public contract 应保持强类型方法，避免调用方手动拼 method、params 或 JSON-RPC envelope。

目标形态示例：

```rust
pub trait GatewayClient {
    async fn list_models(
        &self,
        request: ModelListRequest,
    ) -> TalosResult<ModelListResponse>;

    async fn list_sessions(
        &self,
        request: SessionListRequest,
    ) -> TalosResult<SessionListResponse>;

    async fn start_turn(
        &self,
        request: StartTurnRequest,
    ) -> TalosResult<StartTurnResponse>;

    async fn subscribe_timeline(
        &self,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<TimelineEventStream>;

    async fn recover_timeline(
        &self,
        request: RecoverTimelineRequest,
    ) -> TalosResult<TimelineRecoveryResponse>;

    async fn shutdown(&self) -> TalosResult<()>;
}
```

约束：

1. `AppServerClient` 和 `EmbeddedServerClient` 必须实现同一 `GatewayClient` contract。
2. 同一方法名、参数类型、返回类型、错误语义和 stream 语义必须一致。
3. 新增 Gateway 能力时，必须同时更新 public contract、app-server client、embedded client、TS SDK generated/wrapped contract 和行为测试。
4. SDK 不暴露 Store record、app-server JSON-RPC envelope 或 runtime 内部 handle 给 TUI / CLI。
5. TUI / CLI 除统一 bootstrap / factory 外，不直接依赖具体 client 类型。

## Rust SDK Crate Boundary

为了控制依赖图，embedded 能力不应无条件进入当前轻量 `talos-sdk` 默认依赖。

目标 crate 形态：

```text
talos-rs/apps/sdk                       # package: talos-sdk
  -> shared type re-export
  -> GatewayClient public contract
  -> AppServerClient
  -> ClientMode / factory
  -> shared error re-export / transport error mapping
  -> TimelineEventStream type

talos-rs/apps/sdk-embedded              # package: talos-sdk-embedded
  -> EmbeddedServerClient
  -> depends on talos-sdk public contract
  -> depends on talos-base / runtime
```

可接受替代形态：

```text
talos-sdk
  -> default: public contract + AppServerClient
  -> feature embedded: EmbeddedServerClient + talos-base/runtime deps
```

本 spec 推荐优先使用独立 `talos-sdk-embedded` crate。原因是 embedded client 会引入 `talos-base`、runtime、store、coordinator、agent、provider、tooling 等完整服务端依赖；默认把它们放入 `talos-sdk` 会扩大依赖图，并让 app-server protocol 测试工具承担不需要的 runtime 依赖。

无论使用独立 crate 还是 feature，架构约束不变：

1. `talos-sdk-embedded` 只能实现 `talos-sdk` 定义的 public contract。
2. `talos-sdk-embedded` 不能 fork 方法、类型、错误或 stream 语义。
3. `talos-sdk` 可以在 feature 开启时 re-export `EmbeddedServerClient`，但 contract 仍只有一套。
4. `AppServerClient` 不依赖 embedded runtime。

## Rust Client Types

### AppServerClient

`AppServerClient` 是 Rust SDK 对外提供的外部 app-server client。它连接本地或外部 app-server JSON-RPC server，适用于：

1. Rust app-server test / diagnostics。
2. 协议兼容性验证。
3. 独立部署或远程 app-server 连接。

职责：

1. 管理 JSON-RPC stdio framing。
2. 处理 request / response correlation。
3. 处理 notification stream。
4. 把 app-server protocol error 映射为共享 SDK 错误类型。
5. 实现 `GatewayClient` public contract。

ownership 必须显式建模：

```rust
pub enum AppServerOwnership {
    OwnedChild,
    ExternalTransport,
    BorrowedProcess,
}
```

shutdown 语义：

1. `OwnedChild`：client shutdown 可以关闭 stdin、等待子进程退出，并清理本地资源。
2. `ExternalTransport`：client shutdown 只关闭连接，不负责杀进程。
3. `BorrowedProcess`：client shutdown 不得误杀由其他 owner 托管的 app-server。

### EmbeddedServerClient

`EmbeddedServerClient` 是 Rust SDK embedded crate 对外提供的进程内 runtime client。它服务于 TUI / CLI 默认本地路径，也可供需要本地进程内 runtime 的 Rust 工具使用。

命名说明：`EmbeddedServerClient` 中的 `Server` 指 “server capability facade”，不指独立 server 进程。如果后续团队更倾向精确命名，可以改为 `EmbeddedRuntimeClient`，但同级关系和 contract 约束不变。

职责：

1. 读取 runtime 配置。
2. 通过 `talos-base` 的窄 builder API 构建 `TalosApplicationRuntime`。
3. 获取 Gateway/Core 或 ServiceGateway 能力。
4. 实现同一 `GatewayClient` public contract。
5. 管理 embedded runtime 生命周期。
6. 暴露与 `AppServerClient` 一致的 stream、错误和 shutdown 语义。

禁止事项：

1. 不调用 `run_json_rpc_stdio`。
2. 不经 JSON-RPC 自己调用自己。
3. 不直接绕过 Gateway/Core 调 Store、Coordinator 或 AgentHarness。
4. 不暴露 runtime internal handle 给 TUI / CLI。
5. 不在初始化失败时自动 fallback 到 `AppServerClient`。

## Client Factory

TUI / CLI 的 transport 选择只能发生在统一 bootstrap / factory 层，不能散落在 UI state 或 command handler 中。

目标形态：

```rust
pub enum ClientMode {
    Embedded {
        profile: Option<String>,
        config: RuntimeConfig,
    },
    AppServer {
        command: AppServerCommand,
        env: AppServerEnv,
        cwd: Option<PathBuf>,
        ownership: AppServerOwnership,
    },
}

pub async fn create_gateway_client(
    mode: ClientMode,
) -> TalosResult<Box<dyn GatewayClient>>;
```

约束：

1. TUI / CLI 默认 `ClientMode::Embedded`。
2. 显式 app-server 参数才使用 `ClientMode::AppServer`。
3. command handler 和 UI component 只依赖 `dyn GatewayClient` 或等价 facade。
4. Rust app-server test / diagnostics 可以直接构建 `AppServerClient`，因为测试目标就是 app-server。

## App-Server 定位

app-server 是本地 stdio JSON-RPC 能力服务器，不是 Desktop 专用后端。

职责：

1. 暴露服务端 JSON-RPC / app protocol。
2. 处理 stdio framing。
3. 处理 method dispatch。
4. 处理 request / response envelope。
5. 处理 notification 和 subscription runtime。
6. 调用 Gateway/Core。

app-server 不负责：

1. 定义独立业务模型。
2. 复制 Gateway request / response。
3. 访问 Store record 作为对外 DTO。
4. 承担 Desktop 专属逻辑。
5. 作为 embedded client 的内部实现捷径。

Desktop 使用 app-server 的原因：

1. TS SDK 可以基于服务端 JSON-RPC 契约生成/封装类型。
2. Electron 主进程可以托管 app-server 生命周期。
3. stdio JSON-RPC 不占用本地端口。
4. 避免本地 HTTP port 冲突、防火墙、CORS 和 localhost 暴露问题。
5. 与本地能力服务器和未来协议 adapter 的形态一致。

## TS SDK / Desktop

Desktop 使用 TS SDK，不使用 Rust SDK client。

TS SDK 目标：

1. 基于服务端 JSON-RPC / app protocol 生成和封装类型。
2. 通过 app-server package 连接 Electron 主进程托管的 app-server。
3. 暴露与 Rust SDK public contract 同语义的 request / response / event / error。
4. 保持 Browser / HTTP client path 现状，不纳入 TUI embedded 改造。

约束：

1. TS SDK 不能为 Desktop fork 一套业务对象。
2. TS SDK app-server client 不能把 JSON-RPC envelope 暴露给 Desktop UI 作为业务对象。
3. Desktop 拥有 app-server 进程生命周期。
4. TS SDK 不需要在本轮实现 embedded。
5. 如果 TS 侧未来引入 `GatewayClient` 统一接口，也必须对齐同一 public API contract。

## Gateway / Service / Store 分层

Gateway/Core、Service、Store、Coordinator、Agent 的职责边界保持不变。

目标链路：

```text
SDK / Transport
  -> Gateway Core
    -> Talos Service
      -> Store / Coordinator / Agent
```

约束：

1. Gateway/Core 是业务 API 语义唯一入口。
2. Service 负责业务事务、权限、幂等、session / turn / timeline 状态编排。
3. Store 负责持久化访问和 `XxxRecord`。
4. Coordinator / Agent / Kernel 负责执行能力。
5. `Record -> Info` 转换在 Service 层完成。
6. SDK、app-server、TUI、CLI、Desktop 不接触 Store record。
7. embedded 和 app-server 必须落到同一 Gateway/Core 能力，不能各自调用不同下层路径。

## Subscription / Event Streaming

event subscription 不是普通同步 Gateway method call。terminal、replay、recovery、active turn、close-on-terminal、close-when-idle、start-turn-and-stream association 等语义都属于服务端 runtime / Gateway / Service contract。客户端不能重新实现这些业务语义。

`AppServerClient` 和 `EmbeddedServerClient` 可以有本地 subscription sender / receiver routing，但不能拥有 turn filtering、pending buffer flush、terminal 判断或 replay / recovery 解释。turn 过滤必须通过服务端 contract 表达，例如 `SubscribeTimelineRequest.turn_id`；stream 关闭必须由服务端 subscription close / termination contract 驱动。

服务端必须负责：

1. active turn 与 timeline stream 的关联语义。
2. terminal event 判断和 stream 终止语义。
3. replay / recovery cursor 解释。
4. close-on-terminal / close-when-idle 行为。
5. start_turn、interrupt_turn、submit_tool_result 对 timeline stream 的影响。
6. turn-scoped filtering，例如 `SubscribeTimelineRequest.turn_id` 对 replay 和 live events 的过滤。
7. 如果 TUI / CLI 需要 start-turn-and-stream 这类组合流程，可以在 TUI / CLI 自身层面组合 SDK contract；SDK 层不应混入 TUI / CLI 自身业务范围或 convenience workflow。

SDK client 只允许负责：

1. transport framing，例如 JSON-RPC notification / in-process event channel。
2. typed decode，把 payload 解成 `TimelineEventEnvelope`。
3. minimal subscription routing，例如按服务端返回的 subscription id 把事件送到对应 `TimelineEventStream`。
4. local resource cleanup，例如 drop stream、unsubscribe、根据服务端 termination notification 关闭 receiver。
5. 将 transport failure 映射到共享错误类型。

SDK client 不允许负责：

1. turn_id 业务过滤。
2. terminal event 业务判断。
3. pending buffer 的业务 flush 语义。
4. replay / recovery 语义解释。
5. 自行发明 `start_turn + subscribe + filter` 作为核心 SDK 语义。
6. 暴露 app-server notification envelope 给 TUI / CLI。

目标：

1. `AppServerClient` 和 `EmbeddedServerClient` 都返回同一 `TimelineEventStream` public type。
2. stream 语义来自服务端 contract，而不是 client-side cache / filter。
3. TUI / CLI 只消费 `TimelineEventStream` 和 typed events。
4. SDK public contract 只暴露 `start_turn`、`open_timeline_stream`、`subscribe_timeline`、`unsubscribe_timeline`、`recover_timeline`、`interrupt_turn`、`submit_tool_result` 等服务端原子能力；TUI / CLI 如需组合 start-turn-and-stream 流程，应在端内 workflow 层组合 SDK API。

必须建立 parity tests，分别跑 `AppServerClient` 和 `EmbeddedServerClient`，确保事件序列、终止、replay、cancel、shutdown 行为一致，并证明 client 没有重新解释业务事件含义。

## Shared Error Types

当前错误来源包括：

1. Rust `TalosError`。
2. app-server protocol error body / numeric code。
3. TS SDK protocol / HTTP error class。

本设计要求在共享错误模块建立统一错误类型，而不是在各 SDK 或协议层各自新增平行错误层，也不是要求立刻新增一个名为 `GatewayError` 的类型。

`talos-error::TalosError` 是 Rust 侧错误语义的唯一来源。app-server protocol 可以保留 JSON-RPC numeric code 作为协议 envelope，但它必须映射自 `TalosError`，不能再派生一套平行错误概念。TS SDK 的 `AppServerProtocolError` 暴露 protocol code 和 message；业务判断应优先基于 SDK wrapper 提供的共享语义，而不是在业务层解析 raw JSON-RPC envelope。

映射要求：

```text
TalosError
  -> AppServerErrorBody
    -> TS SDK Error
```

约束：

1. 错误类型属于共享 error 模块；SDK client 可以 re-export 并保留底层 cause，但不能定义另一套错误层。
2. app-server transport error 与 Gateway business error 必须区分。
3. TUI / CLI / Desktop UI 不解析 JSON-RPC raw error 判断业务状态。
4. embedded runtime init failure 映射为 runtime unavailable。
5. app-server connection failure 映射为 transport unavailable。
6. session / project / resource not found 映射为 not found。
7. active turn conflict 映射为 conflict 或 invalid state，必须统一。

## Client Kind / Request Metadata

`client_kind` 取值限定为：

```text
client_kind = tui | cli | desktop | web
```

当前 `client_kind` 主要存在于 Cloud Login 请求语义中，不是全局 request metadata。本设计不允许把 Cloud Login 的字段直接泛化成所有 API 的隐式上下文。

如果需要全局 trace / audit metadata，应新增明确 request context / protocol envelope：

```text
request_id
correlation_id
client_kind
```

约束：

1. `client_kind` 表示调用来源，不表示 transport。
2. 不引入 `transport_kind` 公共字段。
3. 业务逻辑不能根据 `client_kind` 改变语义。
4. metadata 不进入业务 domain object。

## MCP Server 演进边界

app-server 当前已经是 Talos app protocol over stdio JSON-RPC。它具备未来演进为 MCP server 形态的基础，因为 MCP 也以 JSON-RPC 作为核心协议形态之一，且 app-server 已经具备本地 stdio server、request/response、notification、schema/codegen 和 Gateway/Core adapter 等基础设施。

可以成立的判断：

1. stdio JSON-RPC 形态与 MCP 的部分 transport 形态相近。
2. app-server 的 runtime、notification、schema/codegen 和 Gateway/Core 分层可以支撑未来 MCP server 形态演进。
3. 未来可以在 app-server 上增加 MCP-compatible capability mapping，并继续落到同一 Gateway/Core。

仍需补充的能力：

1. MCP tools/resources/prompts 的 namespace 和 schema 映射。
2. MCP initialize / capability negotiation。
3. Talos timeline event 与 MCP notification / progress 语义的映射。
4. 权限、workspace、tool execution 和 resource access 的 MCP 边界。

因此，本 spec 认为 app-server 未来演进成 MCP server 形态是自然方向，但 MCP 兼容不是本轮目标。本轮只要求 app-server 继续保持清晰的 stdio JSON-RPC 能力服务器边界，并复用 Gateway/Core 语义。

## TUI / CLI 启动语义

必达目标：

```text
talos
  -> 启动 TUI，默认 EmbeddedServerClient

local CLI command
  -> 默认 EmbeddedServerClient

explicit app-server mode
  -> AppServerClient

app-server binary
  -> 独立 stdio JSON-RPC capability server 继续保留
```

是否把独立 `app-server` binary 合并为 `talos app-server` 子命令属于单独 CLI surface 设计，不作为本 spec 的前提。

约束：

1. 默认 TUI 文案不再显示正在连接 app-server。
2. embedded mode 日志明确当前 client 为 `EmbeddedServerClient`。
3. embedded 初始化失败直接报错，不静默 fallback 到 app-server。
4. TUI / CLI 不保留 app-server binary 参数或环境变量入口。
5. TUI / CLI 不直接 import `AppServerClient`。

## Lifecycle

`EmbeddedServerClient` lifecycle：

```text
create client
  -> load config
  -> build TalosApplicationRuntime
  -> obtain Gateway/Core handle
  -> serve SDK requests
  -> open / close TimelineEventStream
  -> shutdown client
  -> stop background tasks
  -> flush state
  -> release runtime resources
```

`AppServerClient` lifecycle：

```text
create client
  -> start or attach app-server according to ownership
  -> connect JSON-RPC stdio transport
  -> serve SDK requests
  -> open / close remote subscriptions
  -> shutdown client according to ownership
```

约束：

1. embedded shutdown 必须关闭 active stream。
2. embedded shutdown 必须触发 runtime cleanup。
3. app-server client shutdown 必须尊重 ownership。
4. TUI / CLI 退出必须走 SDK shutdown。
5. Desktop app-server 进程生命周期仍由 Desktop 主进程负责。

## Behavior Tests

统一 public API contract 必须通过行为测试保护。

目标测试矩阵：

```text
GatewayClient behavior tests
  -> AppServerClient
  -> EmbeddedServerClient

TS SDK contract tests
  -> app-server JSON-RPC
```

覆盖内容：

1. session create / list / get。
2. project / model / config 基础 API。
3. start turn。
4. subscribe timeline。
5. recover timeline。
6. interrupt / conflict。
7. not found / invalid request / permission / transport unavailable 错误语义。
8. stream terminal event。
9. cancellation / shutdown。

原则：

1. 新增 public API 必须补同一组 contract tests。
2. behavior tests 面向 `GatewayClient` contract，不面向具体实现内部。
3. app-server protocol E2E 可以继续直接使用 `AppServerClient`。
4. embedded tests 不依赖真实外部 app-server。

## Impacted Areas

预计受影响区域：

1. `talos-rs/apps/sdk`：定义 public contract、保留 `AppServerClient`、新增 factory / mode、re-export shared error types、提供 stream type。
2. `talos-rs/apps/sdk-embedded`：新增 `EmbeddedServerClient`，依赖 `talos-base` 和 `talos-sdk` contract。
3. `talos-rs/apps/ui-tui`：移除或迁移本地 `GatewayClient` trait，使用 `EmbeddedServerClient`。
4. `talos-rs/apps/cli`：使用 `EmbeddedServerClient`，不暴露 app-server transport 选择。
5. `talos-rs/apps/app-server-test-client`：继续直接使用 `AppServerClient` 验证 app-server protocol。
6. `talos-rs/crates/gateway/app-server`：保持本地 stdio JSON-RPC capability server 定位，必要时抽取 subscription runtime。
7. `talos-rs/bin/app-server`：继续作为独立 app-server binary。
8. `talos-rs/crates/app/talos-base`：提供窄 runtime builder API，避免 embedded client 依赖 app-server binary/server 入口。
9. `talos-app/ts-sdk`：保持 TS SDK generated / wrapped app protocol，确认错误和事件语义与 Rust SDK contract 对齐。
10. `talos-app/desktop`：继续通过 Electron 主进程托管 app-server。

## Open Risks

1. 如果 `talos-sdk-embedded` 未共享 `talos-sdk` public contract，两个 Rust client 会长期漂移。
2. 如果 embedded 直接复用 JSON-RPC 自调用，会形成错误分层，并引入不必要 envelope。
3. 如果 subscription runtime 不共享或缺少 parity tests，事件流语义会漂移。
4. 如果共享错误类型和 app-server protocol code 没有明确映射表，TUI / CLI / Desktop 会用不同方式判断错误。
5. 如果 app-server ownership 不建模，shutdown 可能误杀外部进程或泄漏 child。
6. 如果 `client_kind` 被当作业务字段滥用，会污染 domain object。
7. 如果 MCP server 演进表述过度，会误导实现把当前 Talos app protocol 直接等同于 MCP。
8. 如果 CLI/TUI command handler 直接依赖具体 client 类型，后续 transport 切换会继续扩散。

## 设计结论

本设计的核心是建立长期稳定的多端 SDK public API contract。

`talos-sdk` 和 `talos-sdk-embedded` 可以拆 crate 来控制依赖图，但它们必须实现同一套 public API。`AppServerClient` 和 `EmbeddedServerClient` 是同级 client：前者连接本地 stdio JSON-RPC app-server，后者在进程内构建 Talos runtime。两者都服务于同一套 Gateway/Core 能力，不能产生两套业务语义。

TUI / CLI 默认使用 `EmbeddedServerClient`，解决本地终端使用门槛。Rust app-server test / diagnostics 继续使用 `AppServerClient` 验证 app-server。Desktop 使用 TS SDK app-server path，不使用 Rust SDK client。app-server 是本地 stdio JSON-RPC 能力服务器，不是 Desktop 专用后端；未来演进成 MCP server 形态相对自然，但需要补齐 MCP capability、tool/resource/prompt、notification 和权限边界映射，并继续复用 Gateway/Core。

统一 public API、统一对象模型、统一错误类型、统一事件流语义和统一行为测试，是多端形态下保持长期迭代和可维护性的基础约束。
