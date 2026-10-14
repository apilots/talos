# 发布流程

[English](RELEASE.md)

Talos 负责版本 tag、唯一的 Release Notes、准出检查、产品制品、容器镜像和 GitHub
Release。`apilots/talos-releases` 中已经发布的版本和下载制品继续作为历史归档保留；
后续发布不再依赖该仓库。

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

每次推送 `release-*` 分支都会通过 `release-source.yml` 运行完整源码门禁。正式标签复用同一门禁和已验证的 Cargo 锁文件。分支预检不创建标签、发布产物或镜像，也不部署服务；发布脚本应在候选预检通过后运行。

release 分支只接受 bug 修复，以及修复所需的回归测试、发布文档或打包纠错。
准备提交之后统一使用 `fix` 提交；不增加特性、不做无关重构，也不将持续开发中的
main 整体合入。需要共享修复时 cherry-pick 对应提交。

版本准备提交不必是 HEAD，最终 tag 指向包含全部修复的 release 分支末端。
共享校验器拒绝准备提交之后的非 fix 提交；提交分类只是守卫，review 仍须判断内容
是否偷偷引入新特性。

从上一个正式 tag 到最终 release 提交生成英文 Notes，并人工编辑。annotated tag 中的
Notes 是唯一发布正文。生成器按提交类型归类，排除 merge、正式版本准备和下一开发版本提交，
历史无类型提交保留在 Other Changes，未知类型报错，空分类不输出。

```bash
scripts/generate-release-notes.sh v0.1.9 --from v0.1.8 --output /tmp/talos-v0.1.9.md
./release.sh 0.1.9 --notes /tmp/talos-v0.1.9.md --dry-run --yes
./release.sh 0.1.9 --notes /tmp/talos-v0.1.9.md --yes
```

工作区必须干净，分支必须与同名远端同步。脚本创建并推送 annotated tag，tag 正文就是
唯一的 Release Notes。推送 tag 后由 Talos 的 release workflow 执行准出；只有源码校验、
质量检查、产品构建、TUI 原生安装验收和镜像发布都成功后，才创建 GitHub Release。
若 workflow 失败，修复工作流或 runner 问题后可对同一个不可变 tag 重跑 `release.yml`；
不得移动已发布的 tag，也不能用修改后的代码重出同名版本。

## 统一准出与发包

`apilots/talos` 中的 `release.yml` 在推送 `v*` tag 时执行以下流程，也支持维护者手动重跑：

1. 将不可变 tag 解析为精确源码 SHA，校验正式版本、准备与修复历史，以及 annotated tag
   中非空的唯一 Notes。
2. 从该源码运行 `bash scripts/check-release.sh`：提交与 Notes 契约、Rust 格式/检查/测试、
   前端流水线、安装器契约测试。
3. 从验证后的 SHA 构建原有 Desktop、WebUI 和四平台 TUI 产品。TUI 上传前都必须通过原生
   安装 E2E，并通过 `TUI_INSTALL_PRODUCT_DIR` 复用刚构建的包。
4. 构建并发布 `linux/amd64` 与 `linux/arm64` 的 Server、Worker 镜像，生成 checksums 和
   release manifest，再验证最终安装器制品。
5. 所有必需 job 成功后才创建 Talos GitHub Release，正文原样取自 tag Notes；可选 R2
   镜像上传继续保留。

所有 release job 使用 GitHub-hosted runner。没有跳过准出的输入，也不能用本地验证代替。
要重跑已有 tag 的 release workflow 时执行：

每个产品归档和 Desktop 安装包都携带 Talos `LICENSE` 与 `NOTICE`；Server 和 Worker
镜像将三份许可文件安装到 `/usr/share/licenses/talos`。其中 `THIRD_PARTY_NOTICES`
列出第三方依赖的许可类别和固定版本源码位置，包括 MPL 组件的源码。Release job 在上传前
检查各平台制品内容，GitHub Release 也并列提供这些文件及校验和、产品制品。

```bash
gh workflow run release.yml --repo apilots/talos \
  -f tag=v0.1.9 -f publish_latest=true
```

连续发布时先完成旧版，旧版使用 `publish_latest=false`，最新版使用 true。工作流串行执行，
并以当前 GitHub latest 版本为基准，阻止旧版倒退 latest。日常小修仅验证受影响场景，发布准出统一
运行完整检查。本地验证输出保存在忽略目录 `output/release-<version>/`；未运行的
真实账号场景必须记录为未验证，不能由单测或构建成功推断其通过。

## 发布后收尾

下载公开资产，Linux 使用 `sha256sum --check checksums.txt`，macOS 使用
`shasum -a 256 -c checksums.txt`。验证 Release Notes 与 annotated tag 一致、manifest
源码 SHA 对应不可变 tag、镜像包含 `linux/amd64` 与 `linux/arm64`，最新版本还要验证 latest 引用。

发布成功后将 release 分支合回 main。发生版本冲突时保留 main 已进入的下一开发
版本，不能将 main 降回正式版本，也不能用 release 整棵文件树覆盖正在进行的开发。
Docker 默认值更新为新发布镜像。如果 main 尚未切下一版本，再用独立提交进入开发周期。
已发布 tag 始终不变。协议和数据升级还需执行
[部署文档](../../deploy/README_zh.md#数据升级与回滚)中的流程。
