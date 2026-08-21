# Talos 端到端测试

[English](README.md)

`talos-e2e` 验证跨 Rust Runtime、Gateway、终端客户端、Web/Desktop adapter、SDK、
外部服务、打包和安装的真实产品链路。单元测试保留在 owner package 中；本 workspace
负责证明跨组件行为。

## 目录

```text
talos-e2e/
|-- scenarios/        Executable scenarios grouped by product flow
|-- scripts/          Shared runner, server, fixture, and assertion helpers
|-- tests/            Runner and helper regression tests
|-- docker/           Fixed container environments
|-- reports/          Generated reports and logs
`-- run_e2e.sh        Scenario discovery and execution entry point
```

## 运行

从仓库根目录运行：

```bash
just e2e-case 01_health_and_models
just e2e-group 09_app_server
just e2e-group 14_sandbox
```

也可以直接使用 runner：

```bash
talos-e2e/run_e2e.sh --case 09_app_server/protocol_e2e
talos-e2e/run_e2e.sh --group 15_tui_install
```

Runner 只预构建所选场景需要的二进制。开发时优先运行单个 case，交付前运行所属 group。

## 场景职责

| Groups | 范围 |
| --- | --- |
| `01`-`03` | Server health、models、Session lifecycle、HTTP 和 SSE |
| `05`-`08` | CLI、Tools、Memory 和 embedded TUI |
| `09`-`10` | App Server 协议、Skills、MCP、Web、Desktop 和 SDK regression |
| `11`-`13` | Cloud auth、Files 和持久化用户交互 |
| `14` | 统一 Sandbox 和权限执行 |
| `15` | Release bundle 安装和已安装 TUI 启动 |

当前 case 清单以 `scenarios/` 为事实来源。
当前没有 Group `04`；应以场景目录为准，不要假设 Group 编号连续。

## 环境要求

| 范围 | 额外要求 |
| --- | --- |
| 基础 Server、CLI 和 TUI | Rust toolchain 和仓库测试工具 |
| Web 和 Desktop | Node.js、pnpm 依赖和平台 UI 前置条件 |
| 真实模型或 MCP 链路 | 网络访问以及场景 preflight 要求的 Provider credential |
| Cloud auth 和 S3 | 显式 Cloud 测试 credential 和隔离资源 |
| Linux Sandbox 补充验证 | 受支持主机上的 Docker |
| Release 安装 | 原生目标环境；Docker 只补充验证 Linux |

每个场景负责自己的 preflight，并报告缺失变量或工具。运行带 credential 或外部服务的
Group 前先阅读所选脚本。

## 证据和临时数据

生成证据写入 `talos-e2e/reports/`。临时 Talos home 和场景数据默认使用
`talos-e2e/.tmp/`，可通过 `TALOS_E2E_TMP_ROOT` 覆盖。这些目录是生成输出，
不是源码文档。

## 测试规则

- 验证真实用户链路和可观察效果，不只验证返回 `OK`。
- 确定性测试不依赖不稳定时间和外部网络。
- 只有明确负责集成的场景才使用真实服务。
- 复用共享 runner helper 和 fixture，不重复 setup。
- 需要手工证据时，保留能够证明关键行为的日志或报告。
