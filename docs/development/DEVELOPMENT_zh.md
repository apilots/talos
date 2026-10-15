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

CI 的 Rust、Node 和 pnpm 版本统一来自 `.github/release-toolchain.env`。Rust 缓存按
平台、架构、构建模式、工具链及解析后的依赖图隔离，缓存命中不替代编译或行为检查。
标签 pre-push 只核对元数据，发布脚本和发布 CI 仍要求精确源码候选证据。

## 端到端测试

```bash
just e2e-case 01_health_and_models
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
