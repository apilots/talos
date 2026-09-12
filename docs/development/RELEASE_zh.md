# 发布流程

[English](RELEASE.md)

Talos 的每个版本发布在两个仓库中。源码仓库负责 tag 和唯一的 Release Notes；
`apilots/talos-releases` 原样复用该内容，不再重复生成，并附加发布制品。

## 提交信息

非 merge commit 必须使用以下格式：

```text
<type>: <description>
<type>(<scope>): <description>
<type>!: <description>
<type>(<scope>)!: <description>
```

允许的类型为 `feat`、`fix`、`perf`、`refactor`、`docs`、`test`、`build`、
`ci`、`chore`、`style`、`revert` 和 `tool`。Git merge commit 由 commit hook
直接识别并豁免。正式版本号提交固定使用：

```text
chore(release): prepare v<version>
```

## 准备发布

先记录远端 `main` 的精确提交，再从该提交创建发布分支：

```bash
git fetch origin main --tags
release_base="$(git rev-parse origin/main)"
git switch -c release-0.1.8 "${release_base}"
test "$(git rev-parse HEAD)" = "${release_base}"
git push -u origin release-0.1.8
```

以上一个发布 tag 为起点生成英文 Release Notes 草稿，发布前必须人工审阅并编辑：

```bash
scripts/generate-release-notes.sh v0.1.8 \
  --from v0.1.7 \
  --output /tmp/talos-v0.1.8-release-notes.md
```

生成器按提交类型分类，排除 merge 和版本准备提交，将历史遗留的无类型提交归入
Other Changes，并拒绝未知的 typed subject。空分类不会输出。

所有正式版本号位置必须在一个独立提交中更新：

- Rust workspace package 版本。
- TUI 展示版本。
- Web、Desktop 和 TypeScript SDK package 版本，包括 App Server SDK 默认上报的
  client metadata 版本。
- Docker 示例和 Compose 默认镜像版本。

`talos-rs/Cargo.lock` 由 workspace manifest 生成且已明确忽略，不属于版本提交。
发布脚本会校验上述固定提交信息，并拒绝版本清单以外的文件进入该提交。

执行 `release.sh` 前，发布分支必须工作区干净、已经推送并与对应远端分支同步。

## 验证

先运行覆盖上一个版本以来所有变更的场景，再完成发布级检查：

```bash
just test-commit-messages
just test-release-notes
just fmt-check
just check
pnpm --dir talos-app ci:frontend-app
deploy/install/tests/install_test.sh
deploy/install/tests/release_contract_test.sh
```

对变更行为运行相关 E2E。使用下文准出例外时，发布机器至少要运行 `just test`、
前端流水线、安装契约测试和受影响的 E2E，并把输出保存在已忽略的
`output/release-<version>/`。无法完成的平台或真实账号验证必须记录为未验证，
不得描述为已经通过。

## 发布源码 Release

先运行 `--dry-run`。脚本会校验分支、远端同步状态、版本提交、版本一致性、源码
Release 不存在以及 Notes 标题，然后推送 annotated source tag 并创建唯一的源码
Release。如果 tag 已推送但 Release 创建失败，仅当已有 tag 仍精确指向当前 `HEAD` 时，
相同命令才可安全重试；脚本会先验证该条件。

```bash
./release.sh 0.1.8 --notes /tmp/talos-v0.1.8-release-notes.md --dry-run
./release.sh 0.1.8 --notes /tmp/talos-v0.1.8-release-notes.md --yes --watch
```

源码 Release 发布后触发的 workflow 默认先运行完整原生安装矩阵，成功后才调度公开发布仓库。
公开发布 workflow 在找不到对应源码 Release 或正文为空时必须明确失败。

## 发布准出例外

原生安装准出默认必须通过。当 GitHub 因已记录的基础设施或账单问题无法启动私有仓库
job 时，发布人员可以将本地发布验证作为一次性例外，并直接调度公开发布 workflow：

```bash
gh workflow run release.yml --repo apilots/talos-releases \
  -f tag=v0.1.8 \
  -f publish_latest=true
```

直接调度前，必须确认源码 workflow 是在产品测试运行前因账单或基础设施失败。将该
workflow 链接、本地检查命令和结果、精确的源码 tag 与 SHA、直接调度的 workflow 链接
写入 `output/release-<version>/release-evidence.md`。本地 macOS 不能替代未运行的 Windows
与 Linux 安装项，必须将这些项记录为未验证。该例外不得修改或关闭默认源码 Release 准出。

连续发布多个版本时，旧版本使用 `publish_latest=false`，等待其公开 workflow 和制品
全部完成后，再用 `publish_latest=true` 发布最新版本，避免旧任务最后完成而回退 stable
`latest` 引用。

## 验证与收尾

确认两个仓库中同一 tag 的 Release Notes 字节级一致，然后下载公开制品，运行
`sha256sum --check checksums.txt`，检查 `manifest.json`，并确认服务端镜像包含
`linux/amd64` 和 `linux/arm64`。最新版本还要检查 stable latest 引用。任何必需制品或
验证缺失时，发布都不算完成。

发布后将发布分支合回 `main`，再用独立的 `chore(release)` 提交进入下一个开发版本。
协议或数据变更还必须协调消费者升级，并执行
[部署文档](../../deploy/README_zh.md#数据升级与回滚)中的备份与回滚流程。
