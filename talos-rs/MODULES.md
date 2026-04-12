# Talos-RS 模块清单

**最后更新**: 2026-04-14
**当前挂载模块数**: 28 个 crate
**说明**: 本文档是实时 workspace 清单；目标架构与未来扩展方向请看 `ARCHITECTURE.md`。

## 分层总览

```text
┌─────────────────────────────────────────────────────────────────┐
│ Common        6 crates  | LLM abstractions, prompts, shared types │
├─────────────────────────────────────────────────────────────────┤
│ Infra         5 crates  | config, auth, logging, storage, errors  │
├─────────────────────────────────────────────────────────────────┤
│ Perception    1 crate   | channel integrations                    │
├─────────────────────────────────────────────────────────────────┤
│ Cognition     1 crate   | memory services                         │
├─────────────────────────────────────────────────────────────────┤
│ Action        3 crates  | tools, policy, sandbox                  │
├─────────────────────────────────────────────────────────────────┤
│ Orchestration 5 crates  | agent runtime and coordination          │
├─────────────────────────────────────────────────────────────────┤
│ Gateway       2 crates  | gateway core and HTTP API               │
├─────────────────────────────────────────────────────────────────┤
│ App           1 crate   | server composition                      │
├─────────────────────────────────────────────────────────────────┤
│ Apps          3 crates  | SDK, CLI, TUI                           │
├─────────────────────────────────────────────────────────────────┤
│ Binary        1 crate   | executable entrypoint                   │
└─────────────────────────────────────────────────────────────────┘
```

## Common

| Crate | 路径 | 说明 |
|------|------|------|
| `talos-llm-provider` | `crates/common/llm/provider` | 统一 LLM provider trait、消息类型与工具调用协议 |
| `talos-llm-anthropic` | `crates/common/llm/anthropic` | Anthropic provider 实现 |
| `talos-llm-bailian` | `crates/common/llm/bailian` | Bailian provider 实现 |
| `talos-llm-openai` | `crates/common/llm/openai` | OpenAI provider 实现；也承载 Ollama-compatible 接入 |
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
| `talos-tools` | `crates/action/tools` | tool registry、built-in tools、runtime 路由 |
| `talos-tools-policy` | `crates/action/tools-policy` | 工具权限与 allow/deny/elevated 策略 |
| `talos-tools-sandbox` | `crates/action/tools-sandbox` | 工具执行计划与安全沙箱编排 |

## Orchestration

| Crate | 路径 | 说明 |
|------|------|------|
| `talos-agent-kernel` | `crates/orchestration/agent-kernel` | `general_turn`、planner、processor、runtime 主执行流 |
| `talos-agent` | `crates/orchestration/agent` | Agent facade、kernel binding、flush manager |
| `talos-session-store` | `crates/orchestration/session-store` | session id/scope/title 和持久化 |
| `talos-router` | `crates/orchestration/router` | routing rule、binding、优先级匹配 |
| `talos-coordinator` | `crates/orchestration/coordinator` | session -> agent 编排、多 turn 协调和工具审批转发 |

## Gateway

| Crate | 路径 | 说明 |
|------|------|------|
| `talos-gateway` | `crates/gateway/core` | gateway trait 与基础请求/响应模型 |
| `talos-gateway-http` | `crates/gateway/http` | `/v1/sessions` HTTP API、streaming 和 lifecycle gate |

## App / Apps / Binary

| Crate | 路径 | 说明 |
|------|------|------|
| `talos-server` | `crates/app/talos-server` | 组装 config、provider、gateway、coordinator 的服务入口 |
| `talos-client` | `apps/sdk` | Rust SDK client |
| `talos-cli` | `apps/cli` | CLI 客户端 |
| `talos-ui-tui` | `apps/ui-tui` | 终端交互界面 |
| `talos` | `bin/talos` | 可执行程序入口 |

## 文档边界

- `MODULES.md` 只描述当前存在并挂载到 workspace 的 crate。
- `ARCHITECTURE.md` 可以描述未来能力与目标分层，但不承担实时清单职责。
- 当 workspace member 变化时，优先更新本文件与 `PROJECT_STRUCTURE.md`。
