# 开发流程

[English](DEVELOPMENT.md)

> Historical translation. The [English document](DEVELOPMENT.md) owns the current
> 2026-09-07 contracts, diagrams, deployment rules and verification guidance.

仓库命令从仓库根目录运行。根 `justfile` 在 `talos-rs` 中执行 Rust 命令，并提供
共享 E2E 和生成流程入口。

## 环境准备

```bash
just install-tools
just setup-git-hooks
```

前端依赖单独管理：

```bash
pnpm --dir talos-app install
```

## 范围化验证

```bash
just fmt-check
just check -p talos-service
just test -p talos-service
just clippy -p talos-service
```

只在检查受影响 package 后使用 `just fix -p <package>`。共享层或高影响变更在范围化
验证后，可能还需要运行更广泛的检查。

前端验证从 `talos-app` 运行：

```bash
pnpm --dir talos-app lint
pnpm --dir talos-app test
pnpm --dir talos-app build
```

使用 `pnpm --dir talos-app --filter <package> <script>` 运行范围化前端检查，例如
`pnpm --dir talos-app --filter @talos/sdk-core typecheck`。

## Runtime 配置

Talos 使用 `${TALOS_HOME:-$HOME/.talos}` 作为 Runtime home。主要文件为：

```text
$TALOS_HOME/conf/talos.yaml
$TALOS_HOME/.env
$TALOS_HOME/data/talos.db
$TALOS_HOME/logs/talos.log
```

Provider secret 保存在 `.env` 中，配置引用对应的 `TALOS_LLM_*_API_KEY` 环境变量。
仓库默认配置位于 `conf/`，由 Runtime 复制或升级，不直接编辑用户 home 中的文件。

CI 的 Rust、Node 和 pnpm 版本统一来自 `.github/release-toolchain.env`。
`pr-e2e.yml` 负责 PR/main 调度，命名复用工作流分别展示 WebUI、Desktop、TUI、
Server 内部模块和轻量安装契约。`talos-webui.yml` 与 `talos-desktop.yml` 共享
`ci-category.yml`，各自接收对应模块计划。既有五个必需检查名保留为汇总门禁；文档和纯版本
变更跳过重型任务，policy 与汇总门禁仍正常回报。
模块和汇总 job 使用 `!cancelled()`，失败仍进入门禁，而新 push 可以取消过时执行。
清理与证据上传 step 仍可使用 `always()`。

检测器解析 Cargo/pnpm 包归属与反向依赖，每个 owner 独立选择检查，混合变更取并集；
删除和重命名前后的路径都纳入判断。Server 内部按 client、SDK、skills、MCP、tools、
actions、runtime 和 binary 选择受影响包及代表场景；公共运行时和协议变更沿消费者展开，
生成的 wire owner 同时覆盖前端消费者。未知可执行/构建输入保守扩大，运行时 Markdown
资源不能仅凭后缀当作文档跳过。

| 归属 | 托管验证 |
| --- | --- |
| WebUI（含构建配置） | Web 包检查、Computer 删除和资源页；共享 renderer 保留 Desktop 原生资源验收 |
| Desktop | Desktop 包及原生资源/Browser/MCP；媒体、preload、IPC、controller 和打包输入增加 Docker Worker 媒体验收 |
| 仅 TUI 展示 | `just check/test -p talos-tui` 与无 secrets 的 `08_tui/controlled_chat`，不选择 Server/Browser/全平台安装 |
| Server/共享运行时 | 受影响 Rust 包、公开模型/app-server/文件/沙箱/output-log 场景及相关消费者 |
| 安装脚本、打包契约、release-install 工具 | 轻量 shell/归档契约；全平台安装、容器安装和跨架构矩阵只在 release 或手动预检运行 |
| E2E 场景/helper | 拥有者场景与 harness 回归；共享 runner/fixture 保留宽范围代表场景 |

Docker Worker 的浏览器、文件、鉴权、控制链路及平台 MCP/沙箱业务验收仍在相关模块
PR 运行，发行打包检查不能替代这些场景。重型安装仅在发布前或 `release-preflight.yml`
手动预检运行；容器直接消费同次运行的 TUI 包，不安装 Rust、不创建编译卷。

所有 Linux E2E 所需二进制通过一次 `just build --locked` 编译，并以保留执行权限的压缩
artifact 交给同一 workflow run 的场景。普通产品 E2E 使用 debug；安装与发布仍验证 release。
显式提供的二进制缺失时直接失败，不临时重编。Cargo 依赖缓存先恢复，再执行 locked fetch。
二进制 artifact 在同一 workflow run 内使用固定名称，单独重跑失败 job 时可复用已经成功的
上游构建；完整重跑则覆盖该 run 的产物。产物过期后需重新运行构建。

依赖缓存不是测试证据。独立成功记录只在同一 PR、runner 镜像、架构和 UTC 日期内，完整
任务输入指纹匹配时复用。指纹包含任务命令/范围、当前合并树源码、传递依赖、manifest/锁、
fixture/资源、配置及 CI 定义；全仓格式/错误/仓库检查使用更宽的输入集。构建复用还校验
压缩包摘要。没有前缀成功匹配：证据缺失、无效、失败或取消均重跑；还须通过只读 API 确认
来源 workflow 最新 attempt 已完成且成功，API 不可用也重跑。手动 dispatch 和显式 rerun
始终重跑测试。当前 HEAD 门禁仍要求
所有被选任务成功，并在 summary 标明被复用的原 run/SHA。相关 base 输入、编译标志或
工作流变更会失效，不能只比较最后一次 push。外部模型、多机拓扑和发布验收仍按原入口执行。

## 端到端测试

```bash
just e2e-group 01_models
just e2e-group 09_app_server
```

使用能够证明变更行为的最小场景。场景职责和环境要求参见
[`talos-e2e/README_zh.md`](../../talos-e2e/README_zh.md)。

## 生成产物

修改当前 App Server 协议结构后，重新生成 schema 和 TypeScript 协议产物：

```bash
just gen-app-protocol
```

生成文件只能由其 owner generator 修改，并与源定义一起提交。

## 构建存储

```bash
just target-size
just clean-incremental
```

只有确实需要从头构建 debug artifact 时才使用 `just clean-debug`。删除整个 target
目录不是常规验证步骤。

版本化产品发布参见[发布流程](RELEASE_zh.md)。
