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
| `17` | 三个 Worker：公开接口注册、归属隔离、路由、续租、协同与 Workspace 归属 |
| `18` | Computer 云注册：真实 Desktop 登录、持久 UUID、出站注册与续租 |

运行 `just e2e-group 17_multi_node` 会启动真实 Cloud Auth、一个 Server 和三个 Worker，
使用独立 HOME 和端口。需要相邻 `talos-cloud` 仓库（或 `TALOS_CLOUD_REPO`）、
`protoc`、Python `grpcio` / `protobuf` / `PyYAML`；执行场景还需要 `.env` 中的真实模型密钥。
附件场景默认使用 `deepseek-v4-flash-vision-exp`，可用 `TALOS_E2E_IMAGE_MODEL` 指定已配置、可访问的视觉模型。
节点创建和状态修改走公开 RPC，数据库仅用于只读验收。具体场景见
[多节点验收说明](scenarios/17_multi_node/README.md)。已知缺陷也必须报告 FAIL，不能跳过。

当前 case 清单以 `scenarios/` 为事实来源。
当前没有 Group `04`；应以场景目录为准，不要假设 Group 编号连续。

GitHub PR 门禁须经维护者批准后才运行，只执行无需仓库 Secrets 的
`01_health_and_models`、`09_app_server/smoke_e2e`、`10_talos_app`、`12_files` 和
`14_sandbox`，并运行 `talos-auth` 和 `talos-base` package 测试，覆盖公开 Cloud SDK
适配层与 Desktop enrollment 边界。`15_tui_install` 由独立门禁覆盖四个平台原生安装和
Linux Docker。PR 工作流不接收仓库 Secrets，也不 checkout 私有仓库。

维护者可从 `main` 手动运行provider集成工作流 `trusted-e2e.yml`，包含需要真实模型
凭据的完整 `09_app_server` 执行、MCP、Memory和Skills场景。公开app-server smoke
复用原客户端，实际验证模型列表、Session创建、配置及Workspace/Memory API读取，
不执行模型。`11_auth`、
`17_multi_node` 和 `18_computer_workspace` 仍需构建私有 Cloud 服务源码；在该 fixture
可通过公开、可复现方式获得之前，须在相邻 `talos-cloud` checkout 上本地运行。
`19_computer_sync` 的大型拓扑也继续本地验收。上述排除项均明确记录，不得声称已在
GitHub CI 通过。

PR 子集保留有代表性的门禁，同时避免 Fork 代码接触 Secrets。真实 Provider 和私有
服务验收由维护者负责，不会随每个外部贡献自动运行。

## 环境要求

| 范围 | 额外要求 |
| --- | --- |
| 基础 Server、CLI 和 TUI | Rust toolchain 和仓库测试工具 |
| Web 和 Desktop | Node.js、pnpm 依赖和平台 UI 前置条件 |
| 真实模型或 MCP 链路 | 网络访问以及场景 preflight 要求的 Provider credential |
| Cloud auth 和 S3 | Cloud auth 需相邻私有 `talos-cloud` 源码、显式测试 credential 和隔离资源；S3 使用本地 fixture |
| Linux Sandbox 补充验证 | 受支持主机上的 Docker |
| Release 安装 | 原生目标环境；Docker 只补充验证 Linux |

每个场景负责自己的 preflight，并报告缺失变量或工具。运行带 credential 或外部服务的
Group 前先阅读所选脚本。

Computer 注册与主动执行验收组：`18_computer_workspace`。仅注册运行
`just e2e-case 18_computer_workspace/01_registration`，需要 Caddy（可用
`TALOS_E2E_CADDY_BIN` 指定）、Node.js 和 pnpm 依赖，不需要模型 key。
完整执行组还需要可用且有额度的执行/inner/vision Provider；该组不在公开 PR 门禁中运行。
覆盖范围与证据见
[场景说明](scenarios/18_computer_workspace/README.md)。

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
