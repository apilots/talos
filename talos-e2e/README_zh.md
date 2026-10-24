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
just e2e --list                     # 核对全部发现范围
just e2e --check                    # 只检查依赖，不构建、不跑 case
just e2e                            # 执行全部发现的场景
just e2e-group 01_models
just e2e-case 01_models/health_and_models
just e2e-group 09_app_server
just e2e-group 14_sandbox
```

也可以直接使用 runner：

```bash
talos-e2e/run_e2e.sh --case 09_app_server/protocol_e2e
talos-e2e/run_e2e.sh --group 15_tui_install
```

默认运行、`--case` 和 `--group` 统一从同一份排序后的目录发现结果筛选：`scenarios/`
根目录的 `NN_name.sh`，以及编号 `NN_name/` 目录内直接放置的 `*.sh`。不包含嵌套
helper 或未编号目录；新增用例、编号组不需要修改 runner。精确相对路径优先于 basename；
重名 basename 显式失败，selector 不能混用或重复；仍支持省略 `.sh`。

`--list` 可以组合单 case/group，只输出相对路径，不加载 `.env`、不探测依赖、不生成
报告、不构建或执行。`--check` 使用相同范围并加载正常 `.env`，只检查执行前依赖。
缺失项连同受影响 case 一并写入 `reports/preflight_*.log`，在任何构建或场景执行前
阻断所选运行；Docker case 不再被静默排除。预检不证明凭据有效性、网络、浏览器策略或
运行时行为，这些仍由原场景断言负责。

每个组旁边的 `requirements.json` 声明依赖：`default` 是当前目录默认值，`cases`
按文件名替换个别字段，可用空列表表示无需密钥等例外。根目录旧用例使用根目录的声明，
组不会继承根声明。这不是另一份 case 注册表：新增 case 自动继承组默认值；新增组
需补本组依赖声明才能执行。缺失声明、过期 case override、未知字段均显式失败。
字段为 `commands`、`python_modules`、`checks`、`platforms`、`env` 和 `binaries`；
`checks` 检查 Docker、Cloud 可执行文件、复用 `has_real_llm_key` 的模型密钥检测、Chrome、
Linux Desktop 显示、Caddy、Worker 镜像是否存在及原生 sandbox 依赖。`binaries` 声明
`server`、`cli`、`worker`、`app_server`、`test_client`、`sandbox` 的统一预构建；专门构建仍由原场景
wrapper 负责。Linux launcher 只为选中的原生二进制／sandbox case 预构建；纯容器
case 不构建宿主机 launcher。
不会输出密钥和环境值。声明示例：

```json
{"default":{"commands":["cargo","protoc"],"checks":["model"],"binaries":["cli"]},
 "cases":{"catalog.sh":{"checks":[]}}}
```

按组要求配置 `TALOS_CLOUD_SERVER_BIN`、`TALOS_E2E_CHROME_BIN`、
`TALOS_E2E_CADDY_BIN`、`TALOS_E2E_WORKER_IMAGE` 等。19 组保留原生 macOS 与固定
公开 Sync release 的本地验收要求；默认全量在不支持的平台会明确阻断。
默认全量为每个 case 传入所属组，与 `--group` 一致，避免 native sandbox case 在专门
Docker case 前重复隐式运行 Docker；显式单个 sandbox case 保留原 macOS + Docker 语义。

开发时优先运行单个 case，交付前运行所属 group。runner 自身的轻量回归不执行产品场景：

```bash
python3 -m unittest discover -s talos-e2e/tests -p '*_test.py' -v
```

发现逻辑需要 Python3.9+；`--help` 只需要 Bash。

## 场景职责

| Groups | 范围 |
| --- | --- |
| `01_models` | 无密钥的 Server health、模型 catalog、自定义 Provider 持久化与协议工具闭环 |
| 根目录 `02`-`03` cases | Session lifecycle、HTTP 和 SSE |
| `05`-`08` | CLI、Tools、Memory 和 embedded TUI |
| `09` | App Server 协议、Skills、MCP 和 Memory 集成 |
| `11`-`13` | Cloud auth、Files 和持久化用户交互 |
| `14` | 统一 Sandbox 和权限执行 |
| `15` | Release bundle 安装和已安装 TUI 启动 |
| `17` | 三个 Worker：公开接口注册、归属隔离、路由、续租、协同与 Workspace 归属 |
| `18` | Computer workspace：真实 Desktop 登录、持久 UUID、出站注册续租及产品 UI |
| `19` | 多管理端 Sync：原生 Mac Desktop、Mac/Docker Worker、真实认证、NAT 与模型任务 |
| `20` | Browser MCP 与资源工作区：Web/Desktop Tabs、Markdown、原生浏览器与 Docker 桌面/媒体 |

运行 `just e2e-group 17_multi_node` 会启动 Cloud Auth fixture、一个 Server 和三个 Worker，
使用独立 HOME 和端口。通过 `TALOS_CLOUD_SERVER_BIN` 指定 fixture 可执行文件，或将
`cloud-server` 放入 `PATH`；不需要 Cloud 源码仓库。还需要 `protoc`、Python `grpcio` /
`protobuf` / `PyYAML`；执行场景还需要 `.env` 中的真实模型密钥。
附件场景默认使用 `deepseek-v4-flash-vision-exp`，可用 `TALOS_E2E_IMAGE_MODEL` 指定已配置、可访问的视觉模型。
节点创建和状态修改走公开 RPC，数据库仅用于只读验收。具体场景见
[多节点验收说明](scenarios/17_multi_node/README.md)。已知缺陷也必须报告 FAIL，不能跳过。

当前 case 清单以 `scenarios/` 为事实来源。
当前 Group `04` 和 `10` 未分配；应以场景目录为准，不要假设 Group 编号连续。前端单元测试由前端质量工作流负责，
不再作为 E2E 场景重复执行。

GitHub PR/main 调度器选择受影响模块检查的并集，以 WebUI、Desktop、TUI、Server
（client/SDK/skills/MCP/tools/runtime）分类展示，安装单独运行轻量契约。共享一次变更检测和当前 run
二进制编译。Server/公共运行时保留 `01_models`、公开 app-server smoke/recovery、
Computer 删除、`12_files`、`14_sandbox`、`06_tools/output_logs` 及相关 Browser/Worker
验收；Rust 包测试包含不依赖 Provider 凭据的受控 DeepSeek 传输回归。叶子模块选择相关
包和代表场景，具体见[CI 归属与复用](../docs/development/DEVELOPMENT_zh.md)。

`08_tui/controlled_chat` 是无 secrets 的 TUI 门禁，复用受控协议上游，运行真实终端、
Agent 和 Bash 工具，验证流式完成、工具/答案事件持久化、窗口缩放及 `/quit` 退出。
使用 `just e2e-case 08_tui/controlled_chat` 运行；原有依赖真实模型的 TUI/交互分组继续
保留在维护者入口。

`15_tui_install` 只在 release 和手动 `release-preflight.yml` 运行。Linux AMD64/ARM64、
macOS ARM64 原生验收复用本机已构建产品；Linux Docker 必须通过 `TUI_INSTALL_PRODUCT_DIR`
提供同次运行的发行包，不重新编译，失败阻断镜像和 GitHub Release 发布。PR/main 仅选择
轻量安装契约，相关 Docker Worker 和平台业务 E2E 仍按模块触发。PR 不接收仓库 Secrets，也不
checkout 私有仓库。完整输入匹配的成功任务/构建复用独立于 Cargo 依赖缓存，复用来源写入
各 job summary。

`01_models/protocol_compatibility` 使用真实 Gateway、Agent 和一个本地受控上游。
Chat、Responses、Messages 的不同 Provider 使用同一个 opaque 模型 ID，各自在同一
Session 中执行 On→Off 的实际 Bash 工具闭环；Chat 额外覆盖四种 `reasoning_replay`
策略。场景检查协议原生鉴权、根级 KV、思考及工具历史续接，
不需要模型密钥或外部模型网络。Agent 上游为流式；`ProtocolProvider` 的 Rust 受控
传输测试覆盖流式和非流式工具交互。辅助标题生成不计作非流式 Agent 工具覆盖。
请求记录和事件日志随标准场景证据保留。

额外的 Messages 连接保留显式 adaptive 思考配置，并在同一 Session 将强度从 medium
切换到 high。工具闭环的两轮请求都必须携带所选 `output_config.effort`，保留 thinking
的兄弟字段及有签名的历史。这不表示 Chat/Responses 服务支持 adaptive。

维护者可从 `main` 手动运行provider集成工作流 `trusted-e2e.yml`，包含需要真实模型
凭据的完整 `09_app_server` 执行、MCP、Memory和Skills场景。公开app-server smoke
复用原客户端，实际验证模型列表、Session创建、配置及Workspace/Memory API读取，
不执行模型。`11_auth`、`17_multi_node` 和 `18_computer_workspace` 使用配置好的 Cloud fixture
可执行文件启动本地隔离服务；`19_computer_sync` 的大型拓扑也继续本地验收。上述排除项
均明确记录，不得声称已在 GitHub CI 通过。

`09_app_server/computer_deletion` 使用真实 Chrome、HTTP Gateway/Service/Store 和
node gRPC，显式选择无密钥的上游 Auth 契约 fixture。覆盖所有者权限、离线详情、
停用/启用、取消/确认、中英文提示、清空选择和旧凭证拒绝。直接运行
`talos-e2e/scripts/computer_workspace/deletion.py`（不传 `--contract-auth`）可使用
真实隔离 Cloud Auth 验收，需要 `TALOS_CLOUD_SERVER_BIN`。两种模式均需要 Python
Playwright 和已安装的 Chrome（`TALOS_E2E_CHROME_BIN`）。该场景不代替本地 group 19
多管理端 Sync 拓扑验收；删除后的历史导入另有两个 peer 的 Store/CRDT 回归。

`09_app_server/desktop_recovery` 使用真实 controller、SDK、Server 和 app-server 进程，
以受控的本地 Auth/模型 HTTP fixture 验证认证要求、SIGTERM 后自动恢复、已交付事件与
持久历史一致、Computer/Session 不变、后续回合和原取消句柄，不需要私有源码或付费凭据。
打包 Desktop 与 Server/Web 接续仍需使用真实 Auth、Sync 进行本地产品验收。

PR 子集保留有代表性的门禁，同时避免 Fork 代码接触 Secrets。真实 Provider 和
Cloud fixture 验收由维护者负责，不会随每个外部贡献自动运行。

## 环境要求

| 范围 | 额外要求 |
| --- | --- |
| 基础 Server、CLI 和 TUI | Rust toolchain 和仓库测试工具 |
| Web 和 Desktop | Node.js、pnpm 依赖和平台 UI 前置条件 |
| 真实模型或 MCP 链路 | 网络访问以及场景 preflight 要求的 Provider credential |
| Cloud auth 和 S3 | Cloud auth 需 `TALOS_CLOUD_SERVER_BIN`（或 PATH 中的 `cloud-server`）、显式测试 credential 和隔离资源；S3 使用本地 fixture |
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

多控制面同步运行 `just e2e-group 19_computer_sync`，需要真实 macOS、Docker、
Cloud 测试服务和相同的 Python/Node 依赖。Sync 固定为公开 v0.1.3 Release 的
不可变镜像 digest，与锁定 SDK/types 版本对齐；不再默认编译旁边的源码仓库。
该组属于本地拓扑验收，不能称为已有 GitHub PR 门禁通过；构建目录隔离的轻量
回归已纳入 CI。执行类场景需要有额度的模型，注册场景不需要，详见
[场景说明](scenarios/19_computer_sync/README.md)。

## 证据和临时数据

生成证据写入 `talos-e2e/reports/`。临时 Talos home 和场景数据默认使用
`talos-e2e/.tmp/`，可通过 `TALOS_E2E_TMP_ROOT` 覆盖。这些目录是生成输出，
不是源码文档。

所有自动发现的 case 统一经过前后清理边界，各自使用私有临时目录、默认 home、
原生验收构建目录、shell 进程组和 tmux 服务。创建 Docker 资源或独立进程的 fixture
必须使用[生命周期接口](LIFECYCLE.md)登记归属。成功、失败及 INT/TERM 都触发清理；
清理失败会使验收失败并保留归属记录，报告继续保留。只删除本轮资源，不清共享镜像、
调用方二进制、包缓存和构建缓存，也不执行全局 Docker prune。

本轮构建镜像使用唯一 run tag 和 label，跨 case 复用，整轮结束后删除；显式提供的
镜像视为外部输入，不重建、不删除。二进制可通过 `TALOS_E2E_SERVER_BIN`、
`TALOS_E2E_CLI_BIN`、`TALOS_E2E_WORKER_BIN`、`TALOS_E2E_APP_SERVER_BIN`、
`TALOS_E2E_APP_SERVER_TEST_CLIENT_BIN`、`TALOS_E2E_SANDBOX_BIN` 指定可执行文件。
runner 统一解析、导出并复用；指定路径无效直接失败，不回退。未指定时按已有
locked Cargo 流程构建。Linux 优先复用所选二进制旁的 sandbox launcher；缺失时只构建
匹配的 debug/release launcher，避免为 release 验收额外构建 debug 依赖图。

## 测试规则

- 验证真实用户链路和可观察效果，不只验证返回 `OK`。
- 确定性测试不依赖不稳定时间和外部网络。
- 只有明确负责集成的场景才使用真实服务。
- 复用共享 runner helper 和 fixture，不重复 setup。
- 需要手工证据时，保留能够证明关键行为的日志或报告。

`06_tools/cli_bash` 接受上游没有思考文本的响应，即使请求已开启思考；仍要求 Bash 成功执行及工具调用身份一致，不要求伪造思考事件。空思考正常结束、非空思考和工具参数原样传递，由确定性的 Provider 传输回归验证。`13_interaction/user_action_http` 保留精确命令、审批、副作用及终态历史检查，不把实体转义后的命令视为等价命令。

## 20 浏览器与资源工作区

`20_browser_workspace` 包含 Web 公共资源、Desktop 公共资源、Desktop 浏览器宿主、Desktop
MCP、Docker Worker 五个用例，分别验证独立产品边界。完整运行使用 `just e2e-group 20_browser_workspace`。Web 改动选择 01、Computer 删除和共享 renderer 的 Desktop 资源
验收 02；仅 Desktop 源码/资源改动选择 02、03、04；媒体、preload、IPC、controller 和
打包输入增加 05 与同次运行的 Worker 镜像。共享 SDK/runtime/build 输入沿消费者展开，
混合变更取并集，每个用例只执行一次。WebUI 与 Desktop 由不同命名入口共享基础 workflow，
固定汇总门禁验证实际选中的任务。

前置条件、断言、隔离清理、截图归档和真实模型验收边界见[组说明](scenarios/20_browser_workspace/README.md)。公共网页与本地 Markdown 同时覆盖 Web/Desktop；远程电脑 Tab 只由 Docker Worker 能力提供。受控模型链路验证不等同于真实模型自主浏览与产品分析。
