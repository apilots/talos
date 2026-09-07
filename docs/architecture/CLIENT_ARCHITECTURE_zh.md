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
| TS TalosClient   |  | TS AppServer     |  | Rust SDK        |  | Rust SDK        |
| shared types     |  | Client + types   |  | GatewayClient   |  | GatewayClient   |
+--------+---------+  +--------+---------+  +--------+---------+  +--------+---------+
         |                     |                     |                     |
         v                     v                     v                     v
+------------------+  +------------------+  +------------------+  +------------------+
| Browser adapter  |  | Typed preload    |  | EmbeddedServer  |  | Rust AppServer   |
| HTTP + SSE       |  | IPC -> main      |  | Client          |  | Client           |
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

Web 通过 Browser transport 使用 `TalosClient`。Desktop 通过 typed preload IPC 和
负责 JSON-RPC stdio 的 Electron main-process transport 使用 TypeScript
`AppServerClient`。两个 facade 共享生成领域与协议类型，但当前没有实现同一个
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
