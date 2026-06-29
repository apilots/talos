# Talos-RS 模块清单

**最后更新**: 2026-06-21
**当前挂载模块数**: 37 个 crate
**说明**: 本文档是实时 workspace 清单；目标架构与未来扩展方向请看 `ARCHITECTURE.md`。

## 分层总览

```text
┌─────────────────────────────────────────────────────────────────┐
│ Common        7 crates  | LLM abstractions, prompts, fs, types    │
├─────────────────────────────────────────────────────────────────┤
│ Infra         5 crates  | config, auth, logging, storage, errors  │
├─────────────────────────────────────────────────────────────────┤
│ Perception    1 crate   | channel integrations                    │
├─────────────────────────────────────────────────────────────────┤
│ Cognition     1 crate   | memory services                         │
├─────────────────────────────────────────────────────────────────┤
│ Action        6 crates  | tools, skills, plugin, MCP, policy      │
├─────────────────────────────────────────────────────────────────┤
│ Orchestration 6 crates  | agent runtime, services, coordination   │
├─────────────────────────────────────────────────────────────────┤
│ Gateway       4 crates  | gateway core, HTTP API, app-server      │
├─────────────────────────────────────────────────────────────────┤
│ App           1 crate   | shared runtime base                     │
├─────────────────────────────────────────────────────────────────┤
│ Apps          4 crates  | SDK, CLI, TUI, app-server test client   │
├─────────────────────────────────────────────────────────────────┤
│ Binary        2 crates  | app-server and talos-server             │
└─────────────────────────────────────────────────────────────────┘
```

## Common

| Crate | 路径 | 说明 |
|------|------|------|
| `talos-llm-provider` | `crates/common/llm/provider` | 统一 LLM provider trait、消息类型与工具调用协议 |
| `talos-llm-anthropic` | `crates/common/llm/anthropic` | Anthropic provider 实现 |
| `talos-llm-bailian` | `crates/common/llm/bailian` | Bailian provider 实现 |
| `talos-llm-openai` | `crates/common/llm/openai` | OpenAI provider 实现；也承载 Ollama-compatible 接入 |
| `talos-fs` | `crates/common/fs` | 通用文件系统封装，提供目录创建/删除、原子写入和文件锁等安全文件操作 |
| `talos-prompt` | `crates/common/prompt` | `PromptBuilder`、prompt 预算和动态上下文组装 |
| `talos-types` | `crates/common/types` | Session、Talos item、usage、source/provider 等共享类型 |

## Infra

| Crate | 路径 | 说明 |
|------|------|------|
| `talos-error` | `crates/infra/error` | 统一错误类型与结果别名 |
| `talos-config` | `crates/infra/config` | 强类型配置、校验、默认值和加载逻辑 |
| `talos-logging` | `crates/infra/logging` | tracing/tracing-subscriber 集成 |
| `talos-storage` | `crates/infra/storage` | SQLite 存储抽象与 schema 初始化 |
| `talos-auth` | `crates/infra/auth` | JWT 与设备配对能力 |

## Perception

| Crate | 路径 | 说明 |
|------|------|------|
| `talos-channel` | `crates/perception/channels` | Channel server、Telegram 接入与 channel policy |

## Cognition

| Crate | 路径 | 说明 |
|------|------|------|
| `talos-memory` | `crates/cognition/memory` | session/workspace memory、摘要与检索能力 |

## Action

| Crate | 路径 | 说明 |
|------|------|------|
| `talos-mcp` | `crates/action/mcp` | 基于 `rmcp` 的 MCP client runtime、tool discovery 和 tool execution 封装 |
| `talos-plugin` | `crates/action/plugin` | Plugin marketplace/source/scan/install/enable/disable/update/uninstall 生命周期管理，统一管理 Skills 与 MCP server plugins |
| `talos-skills` | `crates/action/skills` | Skill manifest、skill document 和 prompt 注入相关领域逻辑 |
| `talos-tools` | `crates/action/tools` | tool registry、built-in tools、runtime 路由 |
| `talos-tools-policy` | `crates/action/tools-policy` | 工具权限与 allow/deny/elevated 策略 |
| `talos-tools-sandbox` | `crates/action/tools-sandbox` | 工具执行计划与安全沙箱编排 |

## Orchestration

| Crate | 路径 | 说明 |
|------|------|------|
| `talos-agent-kernel` | `crates/orchestration/agent-kernel` | `general_turn`、planner、processor、runtime 主执行流 |
| `talos-agent` | `crates/orchestration/agent` | Agent facade、kernel binding、flush manager |
| `talos-store` | `crates/orchestration/store` | project/session DB record、store trait、持久化实现和测试 mock |
| `talos-service` | `crates/orchestration/service` | 统一服务层 crate，包含 `TalosService` facade 以及 session、timeline、history、turn、project、file、plugin、skills 等业务服务 |
| `talos-router` | `crates/orchestration/router` | routing rule、binding、优先级匹配 |
| `talos-coordinator` | `crates/orchestration/coordinator` | session -> agent 编排、多 turn 协调和工具审批转发 |

## Gateway

| Crate | 路径 | 说明 |
|------|------|------|
| `talos-gateway` | `crates/gateway/core` | gateway trait 与基础请求/响应模型 |
| `talos-gateway-http` | `crates/gateway/http` | `/v1/sessions` HTTP API、streaming 和 lifecycle gate |
| `talos-app-protocol` | `crates/gateway/app-protocol` | App Server JSON-RPC protocol 类型、schema 与 fixtures |
| `talos-app-server` | `crates/gateway/app-server` | JSON-RPC over stdio App Server runtime |

## App / Apps / Binary

| Crate | 路径 | 说明 |
|------|------|------|
| `talos-base` | `crates/app/talos-base` | 共享 runtime base，供 `app-server` 与 `talos-server` binary 复用 |
| `talos-sdk` | `apps/sdk` | Rust SDK library |
| `talos-app-server-test-client` | `apps/app-server-test-client` | App Server protocol e2e 测试客户端 |
| `talos-tui` | `apps/tui` | Terminal package，构建统一的 TUI 与 command mode binary `talos` |
| `app-server` | `bin/app-server` | 本地 App Server binary，供 Desktop 和协议测试使用 |
| `talos-server` | `bin/talos-server` | HTTP/WebUI Server binary |

## 文档边界

- `MODULES.md` 只描述当前存在并挂载到 workspace 的 crate。
- `ARCHITECTURE.md` 可以描述未来能力与目标分层，但不承担实时清单职责。
- 当 workspace member 变化时，优先更新本文件与 `PROJECT_STRUCTURE.md`。
