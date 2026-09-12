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

## 创建 release 分支时切正式版本

从最新远端 `main` 的精确提交创建 `release-<version>`。创建后立即将所有版本位置
切为正式版本，并以 `chore(release): prepare v<version>` 单独提交、推送。
版本切换发生在进入 release 阶段时。

版本位置包括 Rust workspace、TUI 展示、七个 Web/Desktop/SDK package、App Server SDK
默认客户端版本及 Docker 示例与 Compose 镜像默认值。精确清单、版本和准备提交历史
统一由 `scripts/check-release-version.sh` 校验。Cargo.lock 保持忽略，由 manifest 生成。

分支建立后，`main` 可以立即用独立的 `chore(release): start v<next-version> development`
提交进入下一版本 `-SNAPSHOT` 开发，不必等待 release 发布。Docker 默认值继续指向
已发布的最新正式镜像，直到新版本发布成功。

通用发布规范与自动化调整提交到 main，供后续版本生效；不作为产品 bug 修复
回补到已有 release 分支。

## 稳定阶段只修 bug

release 分支只接受 bug 修复，以及修复所需的回归测试、发布文档或打包纠错。
准备提交之后统一使用 `fix` 提交；不增加特性、不做无关重构，也不将持续开发中的
main 整体合入。需要共享修复时 cherry-pick 对应提交。

版本准备提交不必是 HEAD，最终 tag 指向包含全部修复的 release 分支末端。
共享校验器拒绝准备提交之后的非 fix 提交；提交分类只是守卫，review 仍须判断内容
是否偷偷引入新特性。

从上一个正式 tag 到最终 release 提交生成英文 Notes，并人工编辑。Talos 源码 Release
是唯一 Notes 来源；发布仓库原样复用。生成器按提交类型归类，排除 merge、正式版本准备和下一开发版本提交，历史无类型提交保留在 Other Changes，未知类型报错，空分类不输出。

```bash
scripts/generate-release-notes.sh v0.1.9 --from v0.1.8 --output /tmp/talos-v0.1.9.md
./release.sh 0.1.9 --notes /tmp/talos-v0.1.9.md --dry-run --yes
./release.sh 0.1.9 --notes /tmp/talos-v0.1.9.md --yes
```

工作区必须干净，分支必须与同名远端同步。脚本创建 annotated tag 和源码 Release。
源码 Release 保存源码与 Notes；可下载产品要等公开流水线通过后才发布。
若 tag 已创建但源码 Release 创建失败，只要 tag 仍指向 HEAD 且源码 Release 不存在，
可重跑原命令继续发布。已经发布的 tag 不得移动，也不能以修改后的代码重新出同名包。

## 统一准出与发包

`apilots/talos-releases` 的自动调度和手动调度执行同一条流程：

1. 将 tag 解析为精确源码 SHA，校验正式版本、准备与修复历史、源码 Release 和非空 Notes。
2. 从该源码运行 `bash scripts/check-release.sh`：提交与 Notes 契约、Rust 格式/检查/测试、
   前端流水线、安装器契约测试。
3. 从验证后的 SHA 构建产品和镜像。四个平台的 TUI 上传前必须通过原生安装 E2E；
   通过 `TUI_INSTALL_PRODUCT_DIR` 复用刚构建的包，不重复打包。
4. 所有产品构建与原生安装通过后，才发布镜像 tag、准备最终资产；资产契约通过后
   发布制品 Release。
5. 比较发布后的双仓 Notes。下载校验、manifest 和 latest 核验按收尾步骤人工完成。

Talos 维护检查脚本和 E2E，公开仓库只负责 runner 环境和编排。源码仓库的调度任务
不重复跑准出。没有 skip-gate 参数，也不再用本地验证替代公开准出。
若私有 runner 无法启动轻量调度任务，可直接调用同一公开 workflow：

```bash
gh workflow run release.yml --repo apilots/talos-releases \
  -f tag=v0.1.9 -f publish_latest=true
```

这只改变触发方式，全部公开准出仍必须通过。连续发布时先完成旧版，旧版使用
`publish_latest=false`，最新版使用 true。公开工作流串行执行发布，并以当前公开
GitHub latest 版本为基准，阻止旧版倒退 latest。日常小修仅验证受影响场景，发布准出统一
运行完整检查。本地验证输出保存在忽略目录 `output/release-<version>/`；未运行的
真实账号场景必须记录为未验证，不能由单测或构建成功推断其通过。

## 发布后收尾

下载公开资产，Linux 使用 `sha256sum --check checksums.txt`，macOS 使用
`shasum -a 256 -c checksums.txt`。验证双仓 Notes 字节一致、manifest 精确源码 SHA、
镜像包含 `linux/amd64` 与 `linux/arm64`，最新版本还要验证 latest 引用。

发布成功后将 release 分支合回 main。发生版本冲突时保留 main 已进入的下一开发
版本，不能将 main 降回正式版本，也不能用 release 整棵文件树覆盖正在进行的开发。
Docker 默认值更新为新发布镜像。如果 main 尚未切下一版本，再用独立提交进入开发周期。
已发布 tag 始终不变。协议和数据升级还需执行
[部署文档](../../deploy/README_zh.md#数据升级与回滚)中的流程。
