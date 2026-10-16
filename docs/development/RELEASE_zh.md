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
统一由 `scripts/check-release-version.sh` 校验，包括已提交的 `talos-rs/Cargo.lock`
内本地 workspace 包版本。修改 workspace 版本后，运行
`cargo metadata --offline --format-version 1 --manifest-path talos-rs/Cargo.toml > /dev/null`
更新相应锁文件条目，审阅 registry 版本、来源及校验和均未改变；锁文件纳入独立版本提交。
只有本地包版本变化时跳过 PR/main 重型任务，真实依赖图变化仍触发对应检查。

依赖修改必须一起提交并审阅锁文件。CI 使用 `cargo fetch --locked` 获取已提交依赖图，
不重新生成依赖图。

分支建立后，`main` 可以立即用独立的 `chore(release): start v<next-version> development`
提交进入下一版本 `-SNAPSHOT` 开发，不必等待 release 发布。Docker 默认值继续指向
已发布的最新正式镜像，直到新版本发布成功。

通用发布规范与自动化调整提交到 main，供后续版本生效；不作为产品 bug 修复
回补到已有 release 分支。

## 稳定阶段只修 bug

每次推送 `release-*` 分支，通过 `release-source.yml` 分阶段执行一次源码门禁：契约、Rust 格式/检查/测试、前端质量和安装器契约。源码门禁不生成开发版产品包。候选按分支隔离，更新后取消旧候选；正式发布仍串行执行。发布 runner 使用固定系统标签，Rust/Node/pnpm 版本统一放在 `.github/release-toolchain.env`，升级通过独立 PR 审核。

成功候选保留精确 SHA、run/attempt、前一已发布 tag、Cargo.lock、工具链指纹和标准生成的 Notes，保留期 14 天。标签流程验证并复用该源码门禁和依赖图，不重复执行源码套件或重新解析依赖。缺失、过期或不匹配时明确失败；先对同一源码重跑候选门禁，审阅证据，再创建标签。候选锁文件必须与已提交源码完全一致，源码检查若改动锁文件则拒绝记录；正式构建和制品校验继续复用同一份已验证依赖图。

release 分支只接受 bug 修复，以及修复所需的回归测试、发布文档或打包纠错。
准备提交之后统一使用 `fix` 提交；不增加特性、不做无关重构，也不将持续开发中的
main 整体合入。需要共享修复时 cherry-pick 对应提交。

版本准备提交不必是 HEAD，最终 tag 指向包含全部修复的 release 分支末端。
共享校验器拒绝准备提交之后的非 fix 提交；提交分类只是守卫，review 仍须判断内容
是否偷偷引入新特性。

候选阶段通过现有 `scripts/generate-release-notes.sh`，从前一已发布 tag 到精确候选 SHA 生成英文 Notes。保持脚本的提交分类和标准内容；本地审阅文件、tag 注释和公开 Release 正文使用相同字节。通用升级/回滚说明放在部署文档。

```bash
./release.sh 0.1.10 --dry-run --yes
# 审阅 output/release-notes/v0.1.10.md，再发布同一候选。
./release.sh 0.1.10 --yes
```

工作区必须干净、分支与远端同步，候选源码门禁成功。可选的 `--notes <file>` 必须与候选生成内容一致。脚本在创建标签前验证 Release 尚不存在及候选来源，使用 verbatim 注释。标签 pre-push 核对注释、源码 commit 与版本，不重复分支源码检查；发布脚本及标签 CI 仍强制核验精确源码候选。不构建产品或本地检查包，不自动修正标签或加入补救分支。

首次从一个仓库发布，或迁移发布仓库时，检查已有 GHCR 包的 **Manage Actions access**，确认发布仓库具备 **Write**。登录成功及读取包信息都不能证明此权限；GitHub 没有对应 Actions ACL 的只读 API。使用正常 GITHUB_TOKEN，不假上传、不用个人 Token 补救。发布前确认 immutable release 设置并审阅最终 Notes；正式发布后关联标签和制品锁定，标签注释同样不能修改。

## 统一准出与发包

`apilots/talos` 中的 `release.yml` 在推送 `v*` tag 时执行以下流程，也支持维护者手动重跑：

1. 将不可变 tag 解析为精确 SHA，校验正式版本、提交历史及注释。
2. 核验该 SHA 的成功候选、run/attempt、Notes、锁文件和工具链，导入已验证结果，不重复跑源码套件。
3. 使用已验证锁文件，各构建一次八个正式产品目标，核验构建未改变依赖图。上传前校验实际包；TUI 原生验收通过 `TUI_INSTALL_PRODUCT_DIR` 检查并安装同一份包，不为检查另打开发包。
4. 各架构的 Server/Worker 镜像复用已验证 Linux Server 包内的程序、launcher 和 WebUI，`PRODUCT_STAGE=packaged` 选择正常发布输入；本地源码镜像仍默认使用 `source` 阶段。镜像上传前保留运行时依赖、版本、launcher 和许可探测。
5. 所有产品及镜像门禁成功后发布多架构清单，验证最终安装器制品，创建 GitHub Release，正文使用精确 tag Notes。将 Cargo.lock 和 source-validation.json 作为带校验和的公开资产保存，manifest.validation 记录候选 run 和锁文件摘要；保留可选 R2 上传。

所有 job 使用 GitHub-hosted runner 并设置超时；不提供跳过门禁、未验证结果回退或自动修标签。未完成发布可在同一源码标签上新建一次工作流运行：

```bash
gh workflow run release.yml --repo apilots/talos \
  -f tag=v0.1.10 -f publish_latest=true
```

已存在正式 Release 时，在构建前拒绝执行，不能借此重出旧版。发布自动化修改只需针对性的契约和工作流/配置检查，不要求本地重建整套产品；打包行为变更选择对应验证，CI 保留正式包内容和原生安装门禁。Docker 配置可以用 `docker buildx build --check` 检查，不生成镜像。

每个产品归档和 Desktop 安装包都携带 Talos `LICENSE` 与 `NOTICE`；Server 和 Worker
镜像将三份许可文件安装到 `/usr/share/licenses/talos`。其中 `THIRD_PARTY_NOTICES`
列出第三方依赖的许可类别和固定版本源码位置，包括 MPL 组件的源码。Release job 在上传前
检查各平台制品内容，GitHub Release 也并列提供这些文件及校验和、产品制品。

连续发布时先完成旧版，旧版使用 `publish_latest=false`，最新版使用 true。工作流串行执行，
并以当前 GitHub latest 版本为基准，阻止旧版倒退 latest。日常小修仅验证受影响场景；精确候选执行一次完整源码检查，正式发布复用已验证结果。本地验证输出保存在忽略目录 `output/release-<version>/`；未运行的
真实账号场景必须记录为未验证，不能由单测或构建成功推断其通过。

## 发布后收尾

下载公开资产，Linux 使用 `sha256sum --check checksums.txt`，macOS 使用
`shasum -a 256 -c checksums.txt`。验证 Release Notes 与 annotated tag 一致、manifest
源码 SHA 对应不可变 tag、镜像包含 `linux/amd64` 与 `linux/arm64`，最新版本还要验证 latest 引用。

发布成功后将精确 tag Notes 原样归档到 `docs/releases/<tag>.md`，再将 release 分支合回 main。发生版本冲突时保留 main 已进入的下一开发
版本，不能将 main 降回正式版本，也不能用 release 整棵文件树覆盖正在进行的开发。
Docker 默认值更新为新发布镜像。如果 main 尚未切下一版本，再用独立提交进入开发周期。
已发布 tag 始终不变。协议和数据升级还需执行
[部署文档](../../deploy/README_zh.md#数据升级与回滚)中的流程。
