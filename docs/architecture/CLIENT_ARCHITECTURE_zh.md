# 客户端与 SDK 架构

[English](CLIENT_ARCHITECTURE.md)

> Historical translation. The [English document](CLIENT_ARCHITECTURE.md) owns the current
> 2026-09-07 contracts, diagrams, deployment rules and verification guidance.

## SDK-first 目标

每个 Talos 产品客户端都使用受支持的 SDK。TypeScript SDK、Rust SDK 和 embedded Rust SDK
必须暴露相同的产品能力族和共享领域语义。客户端只选择 transport 实现，不能选择一套
更小或不同的产品 API。这是强制目标契约，不能替代 parity test。

## 客户端链路

```text
+------------------+  +------------------+  +------------------+  +------------------+
| Web UI           |  | Desktop         |  | TUI / Command    |  | Rust SDK client  |
+--------+---------+  +--------+---------+  +--------+---------+  +--------+---------+
         |                     |                     |                     |
         v                     v                     v                     v
+------------------+  +------------------+  +------------------+  +------------------+
| TS TalosClient   |  | TS product SDK   |  | Rust SDK        |  | Rust SDK        |
| shared types     |  | Client + types   |  | GatewayClient   |  | GatewayClient   |
+--------+---------+  +--------+---------+  +--------+---------+  +--------+---------+
         |                     |                     |                     |
         v                     v                     v                     v
+------------------+  +------------------+  +------------------+  +------------------+
| Browser adapter  |  | HTTP/SSE or IPC  |  | EmbeddedServer  |  | Rust AppServer   |
| HTTP + SSE       |  | cloud/local mode |  | Client          |  | Client           |
+--------+---------+  +--------+---------+  +--------+---------+  +--------+---------+
         |                     |                     |                     |
         +---------------------+----------+----------+---------------------+
                                          |
                                          v
+------------------------------------------------------------------------------------+
| HTTP Gateway | main-process JSON-RPC stdio | in-process Gateway | app-server       |
+-----------------------------------------+------------------------------------------+
                                          |
                                          v
+------------------------------------------------------------------------------------+
| Shared TalosService domain results, ErrorCode, and Timeline behavior               |
+------------------------------------------------------------------------------------+
```

## 统一能力面

各 SDK 必须在产品能力、强类型 Request/Response、错误和 Stream 行为上保持对齐：

```text
Authentication and profile
Models and configuration
Sessions, Turns, history, compaction, retry, and interruption
Timeline subscription, replay, recovery, and terminal state
Message queue, steering, dynamic tool results, and user actions
Projects, files, filesystem directories, memory, and workspace documents
Skills, plugins, plugin sources, and lifecycle operations
```

不同语言可以使用符合自身习惯的命名，但能力含义、生命周期、ErrorCode、顺序、分页、
optional 语义和终态必须一致。只有 TypeScript SDK facade、Rust SDK、embedded SDK
以及对应产品 transport 都可达时，能力才算完整。

## TypeScript SDK

| Package | 职责 |
| --- | --- |
| `@talos/sdk-core` | 共享能力 Client、领域类型、错误和生成协议 binding |
| `@talos/sdk-browser` | HTTP、multipart upload、认证 header 和 SSE transport 实现 |
| `@talos/sdk-app-server` | JSON-RPC transport、Electron main process 和 typed preload IPC adapter |

Web 和云模式 Desktop 通过 Browser transport 使用 `TalosClient`，访问同一控制面的
HTTP/SSE 产品 API。Desktop 的 typed preload IPC 和 Electron main-process stdio
通过 `AppServerClient` 管理已打包的 Worker 宿主、登录和节点身份；纯本地模式也使用这条
IPC/stdio 路径访问产品 API。两个 facade 共享生成领域与协议类型，但当前没有实现同一个
TypeScript client interface。因此能力对齐依赖 API review 和 parity test，而不能只依赖
TypeScript 类型系统。

## Rust SDK 与 embedded SDK

`talos-sdk` 负责 Rust `GatewayClient` 能力 contract、共享公共类型和 Rust
`AppServerClient`。`AppServerClient` 通过 JSON-RPC stdio 实现该 contract。

`talos-sdk-embedded` 提供 `EmbeddedServerClient`，通过进程内 Gateway 实现同一个
`GatewayClient` contract。因此 TUI 和 command mode 与协议型 Rust client 使用相同且
由编译器检查的 SDK operation、Response、错误和 Timeline 行为，同时不需要子进程或本地端口。

## 共享类型流水线

```text
Rust domain and current protocol definitions
                  |
                  v
       App Protocol generator
             /          \
            v            v
 JSON Schema        generated/app-protocol.ts
                         |
                         v
             @talos/sdk-core aliases and exports
                         |
                         v
          Browser SDK | App Server SDK | Web | Desktop
```

协议生成产物来源于 Rust 定义。客户端代码不能手改生成文件、复制 Request/Response DTO，
也不能把 transport envelope 暴露为领域对象。

## 客户端职责

Web/Desktop 进入 Session 时先读取全部 history 分页，已完成轮次由 history 展示，
未完成轮次再接入增量事件。`lastStableEventId` 是当前历史页最后一个已完成轮次的
事件游标；读取所有分页后使用最后一页的值。它不是可能包含未完成轮次的最新事件位置，
也不能与 history 的分页游标混用。该响应字段要求后端与生成客户端同步部署，HTTP 和
JSON-RPC 使用相同契约；Rust SDK/TUI 的历史读取方可忽略该字段。

持续查看时保留已消费的事件游标，轮次结束后与 history 衔接，不重播已完成内容。
空闲 Session 通过查询发现运行中轮次和队列变化，不保持执行事件订阅；显式维护操作
仍可观察 Session 事件。active-turn 响应的 `latestEventId` 表示 Session 最新已落库事件，
包含失败轮次和产品通知；空闲时快照变化才刷新 history，可发现轮询间完成的远端轮次。
该字段是变化信号，不能替代订阅所用的 `lastStableEventId`，后端和生成客户端须同步升级。
底层 Session 订阅继续支持跨轮次推送，由 UI 决定何时关闭。

- SDK 负责完整产品 API、强类型领域对象、结构化错误、Stream contract 和
  transport-neutral 生命周期语义。
- Transport adapter 负责序列化、认证传输、连接生命周期、上传编码和事件送达机制。
- Client 负责展示、交互、乐观状态和重连体验。
- Server 负责搜索、匹配、解析、权限和持久化生命周期规则。

## 对齐要求

- API 变更同时审查 Rust SDK、TypeScript SDK、embedded SDK、HTTP、JSON-RPC、Web、
  Desktop 和 TUI 影响。
- 当前协议变化通过 `just gen-app-protocol` 重新生成 schema 和 TypeScript binding。
- SDK parity test 比较可观察行为，不比较 transport 实现。
- Embedded execution 不能引入公共 SDK contract 不可达的私有能力。
- HTTP 和 JSON-RPC envelope 可以不同，但领域 Result 和 `ErrorCode` 语义必须一致。
- 用户可见 Timeline 变化必须覆盖每个受影响客户端的渲染。

Rust trait 当前强制 app-server 与 embedded 实现对齐。TypeScript Browser 和 App Server
facade 是独立 API，因此需要显式 parity review。相关路径和测试未对齐前，文档不能把
新能力描述为完成。

## Computer 选择与执行归属

联网 Desktop 与 Web 共用 HTTP/SSE 产品 SDK，连接同一控制面。Desktop main 仍通过
现有 IPC/stdio 管理已打包 app-server 内的一个 TalosWorker，保存 UUID/注册凭据并主动
Connect；renderer 只读取控制面配置和安全身份，不持有节点密钥。明确选择纯本地模式时，
产品 API 使用原 IPC/stdio 路径，离线不会自动切回本地。

features/computers 提供独立栏、完整搜索/分页、选择与管理页；本机匹配控制面地址和 UUID，
Web 多台时明确选择。环境和列表携带目标 nodeId，已有 Session 保持原归属。缓存按账号、
控制面、资源隔离，环境/列表再按 Computer 隔离；草稿和最近选择由客户端保存，切换不取消
运行中的 Turn。控制面保存业务元数据与 Timeline，Worker 保存配置、MCP 凭据和 Workspace
文件；Harness/WorkerEnvironment 屏蔽传输，远端 OAuth 回到原 Worker attempt。
