# Talos

[English](README.md)

Talos 是一个 AI Agent 平台，在终端、Web、Desktop、HTTP 和嵌入式客户端之间
提供一致的运行时语义。Rust workspace 负责 Agent Runtime、Session/Turn 生命周期、
模型接入、记忆、工具、Skills、MCP 和沙箱；TypeScript workspace 提供 Web UI、
Desktop shell 和客户端 SDK。

## 产品入口

| 入口 | 类型 | 用途 |
| --- | --- | --- |
| `talos` | 产品 | 使用 embedded runtime 的 TUI 和非交互式终端命令 |
| `talos-server` | 产品 | HTTP API、SSE Timeline 流和托管 Web UI 服务端 |
| Web UI | 产品界面 | 开发环境独立运行或由 `talos-server` 托管的 React UI |
| Desktop | 产品 | 由本地 app-server 支撑的 Electron shell |
| `app-server` | 内部适配器 | 供 Desktop 和协议测试使用的 JSON-RPC over stdio adapter |

## 核心能力

- Session、Turn、Project 和 Timeline 生命周期管理
- 模型无关的 Agent 执行与多 LLM Provider 支持
- 持久化记忆和有界上下文构建
- 内置工具、Skills、MCP server plugins、权限策略和沙箱执行
- 通过同一 Runtime 分发链实现父子 Subagent 协作
- Web、Desktop、TUI 和 Gateway 通过 SDK-first 入口共享领域语义
- TypeScript、Rust 和 embedded SDK 演进以能力对齐为强制契约

## 架构演进方向

Talos 既是承载第一方产品的 Agent 平台，也是供上层应用复用的底层框架。它通过稳定、
语言无关的产品契约，能力对齐的 SDK，清晰的 Runtime 分层和可组合的能力域持续演进。

```text
+----------------------------------------------------------------------------+
| Talos products and higher-level applications                               |
+----------------------------------------------------------------------------+
| Talos framework                                                            |
| Language-neutral contracts | Client SDKs | Embedded SDK | Gateways         |
+------------------------+------------------------+--------------------------+
| Perception             | Cognition              | Action                   |
| Inputs | Channels      | Memory | Reasoning     | Tools | Skills | MCP     |
+------------------------+------------------------+--------------------------+
| Agent orchestration and runtime                                            |
| Agent routing | Session / Turn | Multiple runtimes | Subagent collaboration|
+----------------------------------------------------------------------------+
| Infrastructure                                                             |
| Store | Storage | Config | Auth | Automation | Logging | Observability     |
+----------------------------------------------------------------------------+
| Metacognition [cross-cutting evolution]                                    |
| Evaluation | Self-monitoring | Learning | Identity and personality         |
+----------------------------------------------------------------------------+
```

这是长期分层方向，不是执行时序。同一组层次会在[系统架构](docs/architecture/ARCHITECTURE_zh.md)
中展开，并精确标注当前实现与规划状态。

## 仓库结构

```text
talos/
|-- talos-rs/       Rust runtime, gateways, SDKs, TUI, and server binaries
|-- talos-app/      Web, Desktop, and TypeScript SDK workspace
|-- talos-e2e/      Cross-component integration and release scenarios
|-- deploy/         Server deployment and TUI installation assets
|-- conf/           Repository-managed configuration and migrations
|-- docs/           Current architecture, reference, and development docs
|-- spec/           Active and archived change specifications
|-- build/          Product packaging and branding resources
`-- scripts/        Repository automation
```

## 快速开始

前置依赖为 Rust 和 Cargo、Node.js 24、pnpm 9.14.4 以及 `just`。初始化仓库工具和
前端依赖：

```bash
cargo install --locked just
just install-tools
just setup-git-hooks
pnpm --dir talos-app install
```

默认模型使用 DeepSeek。导出 API key 后，在两个终端分别启动 Web UI 和服务端：

```bash
export TALOS_LLM_DEEPSEEK_API_KEY="your-api-key"
just dev-server
just dev-web
```

运行终端客户端：

```bash
cargo run --manifest-path talos-rs/Cargo.toml -p talos-tui --bin talos
```

## 常用开发命令

```bash
just fmt-check
just check -p talos-service
just test -p talos-service
just e2e-case 01_health_and_models
```

日常开发优先运行 package 范围内的检查。完整流程参见
[开发指南](docs/development/DEVELOPMENT_zh.md)。

## 文档

- [文档索引](docs/README_zh.md)
- [系统架构](docs/architecture/ARCHITECTURE_zh.md)
- [运行时架构](docs/architecture/RUNTIME_ARCHITECTURE_zh.md)
- [客户端架构](docs/architecture/CLIENT_ARCHITECTURE_zh.md)
- [错误模型](docs/architecture/ERROR_MODEL_zh.md)
- [工程结构](docs/reference/PROJECT_STRUCTURE_zh.md)
- [Rust workspace 模块](docs/reference/RUST_MODULES_zh.md)
- [Rust workspace](talos-rs/README_zh.md)
- [Web、Desktop 和 TypeScript SDK](talos-app/README_zh.md)
- [端到端测试](talos-e2e/README_zh.md)
- [部署](deploy/README_zh.md)

具体变更设计位于 [`spec/`](spec/)，其中
[`spec/REGISTRY.md`](spec/REGISTRY.md) 是活跃和归档规格的权威索引。

## 许可证

Talos 使用 MIT License。
