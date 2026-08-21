# Talos 文档

[English](README.md)

本目录保存当前有效的跨组件文档。具体变更设计仍保留在 [`spec/`](../spec/) 中，
不纳入本目录的整理范围。

## 架构

- [系统架构](architecture/ARCHITECTURE_zh.md)：系统边界和整体架构
- [运行时架构](architecture/RUNTIME_ARCHITECTURE_zh.md)：Session、Turn、Agent、Subagent 和工具执行
- [客户端架构](architecture/CLIENT_ARCHITECTURE_zh.md)：Web、Desktop、TUI、SDK 和 Gateway 边界
- [错误模型](architecture/ERROR_MODEL_zh.md)：内部错误和协议安全的错误投影

## 参考

- [工程结构](reference/PROJECT_STRUCTURE_zh.md)：当前仓库目录
- [Rust workspace 模块](reference/RUST_MODULES_zh.md)：当前 Rust workspace 清单

## 开发

- [开发流程](development/DEVELOPMENT_zh.md)：工具、检查、测试和生成产物
- [发布流程](development/RELEASE_zh.md)：版本校验、Tag 和 Release 自动化

## 文档规则

- 英文文件是主文档，使用普通 `.md` 文件名。
- 中文对应文档使用 `_zh.md` 后缀。
- 架构图使用 `text` 代码块中的纯 ASCII。
- 当前状态类文档必须与源码 manifest 核对。
- 组件细节保留在距离组件最近的 README 中。
- 历史和具体变更决策保留在 `spec/`，不在这里重复。
