# Talos Rust workspace

[English](README.md)

> Historical translation. The [English document](README.md) owns the current
> 2026-09-07 contracts, diagrams, deployment rules and verification guidance.

`talos-rs` 包含 Talos Runtime、Application Service、Gateway、Rust SDK、终端产品和
服务端二进制。它负责 Web、Desktop、TUI、HTTP 和 embedded client 共享的产品语义。

## 目录

```text
talos-rs/
|-- crates/
|   |-- common/          Shared types, LLM providers, prompts, and filesystem
|   |-- infra/           Error, config, logging, storage, auth, and automation
|   |-- perception/      External channel adapters
|   |-- cognition/       Memory capabilities
|   |-- action/          Tools, Skills, MCP, policy, and sandboxing
|   |-- orchestration/   Store, service, routing, Agents, and coordination
|   |-- gateway/         Shared Gateway, HTTP, app-server, and protocol
|   `-- app/             Runtime composition and upgrade capability
|-- apps/                Rust SDKs, TUI, and app-server test client
`-- bin/                 app-server and talos-server binaries
```

当前 workspace member 参见 [Rust workspace 模块](../docs/reference/RUST_MODULES_zh.md)，
跨组件边界参见[系统架构](../docs/architecture/ARCHITECTURE_zh.md)。

## 入口

| Binary | Package | 用途 |
| --- | --- | --- |
| `talos` | `talos-tui` | 使用 embedded runtime 的 TUI 和 command mode |
| `app-server` | `app-server` | 供 Desktop 和协议测试使用的本地 JSON-RPC over stdio adapter |
| `talos-server` | `talos-server` | HTTP API、SSE 和 Web UI server |

## 运行

从仓库根目录运行：

```bash
just dev-server
cargo run --manifest-path talos-rs/Cargo.toml -p talos-tui --bin talos
cargo run --manifest-path talos-rs/Cargo.toml -p talos-tui --bin talos -- exec "hello"
```

`app-server` 是协议 adapter，不是可部署产品服务端。TUI 使用 `talos-sdk-embedded`，
不会启动 app-server 子进程。

## 验证

```bash
just fmt
just check -p talos-service
just test -p talos-service
just clippy -p talos-service
```

优先使用能够证明变更行为的最小 package 或 test filter。协议结构变化还需要运行
`just gen-app-protocol`。

## 边界规则

- Gateway 映射 transport；`TalosService` 负责产品生命周期语义。
- Store Record 不跨越 Service、Protocol、SDK 或 UI 边界。
- 多个 Agent Definition 注册为独立 Harness；Coordinator 按 Agent id 解析其中一个。
- 每个 Harness 通过 `AgentProvider` SPI 委托给 Provider 持有的 `AgentSession`。
- 本地 composition 注册 `KernelAgentProvider`；新增 Provider 复用同一 SPI，不改变 Service 或 Gateway 语义。
- Rust `AppServerClient` 与 `EmbeddedServerClient` 实现同一个 `GatewayClient` 能力 contract。
- Tool、Skill、MCP、权限和沙箱错误必须显式保留。

## 相关文档

- [运行时架构](../docs/architecture/RUNTIME_ARCHITECTURE_zh.md)
- [错误模型](../docs/architecture/ERROR_MODEL_zh.md)
- [开发流程](../docs/development/DEVELOPMENT_zh.md)
- [TUI 说明](apps/tui/README.md)
- [App Server 协议](crates/gateway/app-protocol/README.md)
