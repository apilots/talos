# 发布流程

[English](RELEASE.md)

根目录 `release.sh` 创建 annotated Git tag 和 GitHub Release。由 Tag 触发的 CI
workflow 负责构建、打包和发布产品 artifact。

## 前置条件

- 版本变更已经合并到 `main`。
- Rust、TUI、Web、Desktop 和 TypeScript SDK 版本一致。
- 本地 `main` 工作区干净，并与 `origin/main` 同步。
- GitHub CLI 已安装并完成认证。
- Release notes 位于非空 Markdown 文件中。

## 运行

```bash
./release.sh 0.1.7 --notes /tmp/talos-v0.1.7-release-notes.md
./release.sh v0.1.7 --notes /tmp/talos-v0.1.7-release-notes.md --yes --watch
./release.sh 0.1.7 --notes /tmp/talos-v0.1.7-release-notes.md --dry-run
```

脚本校验仓库状态、版本一致性、已有 Tag 和已有 GitHub Release。它不修改版本文件，
也不上传本地构建 artifact。

## 验证

使用 `--dry-run` 检查命令路径而不创建 Tag 或 Release。创建 Release 后检查
Tag-triggered workflow 和所有目标平台 artifact。安装验收由 E2E group
`15_tui_install` 负责。
