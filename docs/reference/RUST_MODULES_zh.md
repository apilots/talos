# Rust workspace 模块

[English](RUST_MODULES.md)

Historical translation. The current 50-member inventory and ownership map are
maintained in [RUST_MODULES.md](RUST_MODULES.md); the older inventory below is
retained for historical context, not as the current manifest reference.

| 区域 | 数量 | Packages |
| --- | ---: | --- |
| Common | 10 | `talos-llm-provider`、Provider 实现、`talos-fs`、`talos-prompt`、`talos-types` |
| Infrastructure | 6 | `talos-error`、`talos-config`、`talos-logging`、`talos-storage`、`talos-auth`、`talos-automation` |
| Perception | 1 | `talos-channel` |
| Cognition | 1 | `talos-memory` |
| Action | 10 | `talos-mcp`、`talos-plugin`、`talos-skills`、Tools、Policy、Sandbox SPI 和平台沙箱 |
| Orchestration | 6 | `talos-agent-kernel`、`talos-agent`、`talos-store`、`talos-service`、`talos-router`、`talos-coordinator` |
| Gateway | 4 | `talos-gateway`、`talos-gateway-http`、`talos-app-protocol`、`talos-app-server` |
| Application composition | 2 | `talos-base`、`talos-upgrade` |
| Apps and SDKs | 4 | `talos-sdk`、`talos-sdk-embedded`、`talos-app-server-test-client`、`talos-tui` |
| Binaries | 2 | `app-server`、`talos-server` |

## Common

- `crates/common/llm/`：Provider contract 以及 Anthropic、Bailian、DeepSeek、
  Ollama、OpenAI 和 OpenRouter 实现。
- `crates/common/fs/`：有界且安全的文件系统操作。
- `crates/common/prompt/`：Prompt 和动态 Context 构建。
- `crates/common/types/`：共享领域与 API 对象。

## Infrastructure 与 Capabilities

- `crates/infra/`：错误、配置、日志、存储、认证和自动化。
- `crates/perception/channels/`：外部 Channel adapter。
- `crates/cognition/memory/`：记忆持久化和检索行为。
- `crates/action/`：工具、Skills、Plugin、MCP、权限策略和沙箱执行。

## Orchestration 与 Gateway

- `crates/orchestration/store/`：数据库 Record 和 Store 接口。
- `crates/orchestration/service/`：面向产品的 Application Service facade。
- `crates/orchestration/router/` 和 `coordinator/`：Agent binding 和分发。
- `crates/orchestration/agent/` 和 `agent-kernel/`：Runtime contract 和模型驱动执行。
- `crates/gateway/`：共享 Gateway API、HTTP、JSON-RPC app-server 和协议定义。

## 组装与入口

- `crates/app/talos-base/`：供产品二进制和 embedded client 复用的 Runtime 组装。
- `crates/app/talos-upgrade/`：Release upgrade 能力。
- `apps/sdk/` 和 `apps/sdk-embedded/`：协议与进程内 Rust SDK。
- `apps/tui/`：`talos` 终端产品。
- `bin/app-server/`：本地 JSON-RPC adapter 二进制。
- `bin/talos-server/`：HTTP 和 Web UI server 二进制。

Workspace member 变化时，应在更新 `Cargo.toml` 后，同步更新本文和英文版本。
