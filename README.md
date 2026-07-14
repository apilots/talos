# Talos (塔罗斯) 🤖

> AI Agent Framework - 智能管家底层框架（多语言实现）

**名字来源**：希腊神话中的青铜自动巨人塔罗斯（Talos），每天绕克里特岛巡逻，守护岛屿免受入侵。象征着自动化、忠诚、坚固的守护者。

---

## 项目定位

Talos 是一个 AI 智能管家底层框架，为上层应用（如贾维斯 Jarvis）提供：

- **感知层**：多模态输入处理、环境理解、事件监听
- **认知层**：记忆系统、推理引擎、决策系统
- **执行层**：工具调用、按需工具发现、Skills、MCP server plugins、多 Agent 协作、输出通道
- **元认知层**：自我监控、持续学习

## 用户界面

Talos提供了多种用户交互界面：

### 产品入口

| 入口 | 说明 |
|------|------|
| `talos` | TUI 用户入口；未来 NPM 安装默认只暴露这个命令 |
| `app-server` | 本地 JSON-RPC over stdio 协议适配器，供 Desktop 和协议测试使用，不作为独立产品发布 |
| `talos-server` | HTTP API / WebUI 服务端入口，面向服务端 tar 包部署 |

### 终端用户界面 (TUI)
- 使用 `cargo run -p talos-tui --bin talos` 运行
- 丰富的键盘导航支持
- 支持多面板视图（聊天、历史、工具、设置）
- 默认以内嵌 runtime 运行，不需要单独启动 `app-server`，也不占用本地端口

### API接口
- HTTP API + SSE 支持
- 本地 JSON-RPC over stdio app-server 支持，供 Desktop 和协议测试使用
- 适用于与其他应用集成

### 桌面端
- Electron Desktop shell 已接入本地 app-server
- 复用 `talos-app/web` 的前端领域模型和主要 UI

### 其他语言实现计划
- 图形用户界面 (GUI) - 计划中
- 移动端应用 - 计划中

---

## 多语言实现

| 实现 | 语言 | 状态 | 说明 |
|------|------|------|------|
| **talos-rs** | Rust | 🚧 开发中 | 系统级性能，内存安全，推荐用于生产环境 |
| **talos-py** | Python | 📋 计划中 | 快速原型，AI 生态集成 |
| **talos-go** | Go | 📋 计划中 | 高并发，云原生部署 |
| **talos-js** | TypeScript/Node.js | 📋 计划中 | Web 集成，快速开发 |

---

## 为什么多语言？

- **Rust**：核心基础设施，性能敏感场景
- **Python**：AI/ML 模型集成，快速实验
- **Go**：微服务部署，云原生
- **TypeScript**：前端/Web 集成

不同语言可以混合部署；当前 Rust 服务端暴露 HTTP/SSE 与本地 JSON-RPC app-server 两类入口。

---

## 项目结构

```
talos/
├── talos-rs/          # Rust 实现
│   ├── Cargo.toml
│   ├── crates/
│   ├── apps/
│   └── bin/
├── talos-py/          # Python 实现（计划中）
├── talos-go/          # Go 实现（计划中）
├── talos-app/         # TypeScript Web / Desktop / SDK workspace
├── talos-e2e/         # 端到端测试场景与 runner
├── spec/              # 接口规范与功能设计
└── README.md
```

---

## 快速开始

### Rust 实现

```bash
cd talos

# 安装本地开发工具
just install-tools

# 启用仓库内置的 commit message 校验
just setup-git-hooks

# 格式化
just fmt

# 构建指定包
just build -p talos-server

# 运行指定包测试，避免日常全量 workspace 编译
just test -p talos-service

# 运行单个 E2E 场景
just e2e-case 01_health_and_models
```

Talos 的 Rust workspace 使用根目录 `justfile` 统一开发入口。`just test`
通过 `cargo-nextest` 运行测试；日常开发应优先按 package 或测试名缩小范围。
提交标题使用分类关键词时，必须采用 `type: description` 或
`type(scope): description` 格式；没有分类关键词的普通标题允许提交，并在 release
notes 中归入 Other。运行 `just setup-git-hooks` 后，本地 Git 会使用仓库版本化的
`commit-msg` hook 执行校验。
`just check` 使用独立的 `target/talos-check` 目录，避免日常类型检查继续膨胀
`target/debug`。
E2E 也可以通过 root `justfile` 运行：`just e2e` 会转发到
`talos-e2e/run_e2e.sh`，`just e2e-case <case>` 用于单 case 重跑，
`just e2e-group <group>` 用于按分组执行。
如果 `talos-rs/target` 体积过大，先使用 `just target-size` 查看分布，再按需运行
`just clean-incremental` 或 `just clean-debug`。不要把清理 target 当作常规验证步骤；
它会牺牲后续增量编译速度。

### 其他语言实现

待开发...

### TUI使用示例
```bash
# 启动 TUI 界面（默认 embedded runtime）
cd talos/talos-rs
cargo run -p talos-tui --bin talos

# TUI/CLI 直接使用 embedded runtime，不需要额外启动 app-server

# 运行 command mode（默认 embedded runtime）
cargo run -p talos-tui --bin talos -- exec --session demo "hello"

# 终端管理命令同样走 embedded runtime
cargo run -p talos-tui --bin talos -- models

# 启动 HTTP/WebUI 服务端
cargo run -p talos-server --bin talos-server
```

---

## 发布 Release

根目录提供 `release.sh` 用于发布 GitHub Release。脚本只负责打 tag 和创建 Release notes；
构建、打包和产物发布由 tag 触发的 CI release workflow 自动完成。

发布前需要先通过 PR 把版本号升级合并到 `main`，并准备好外部 release notes 文件。

```bash
# 在最新 main 分支执行
./release.sh 0.1.3 --notes /tmp/talos-v0.1.3-release-notes.md

# 自动确认并监控 tag 触发的 workflow
./release.sh v0.1.3 --notes /tmp/talos-v0.1.3-release-notes.md --yes --watch

# 只做本地校验和命令预览，不创建 tag / release
./release.sh 0.1.3 --notes /tmp/talos-v0.1.3-release-notes.md --dry-run
```

脚本会校验：

- 当前目录是仓库根目录
- 当前分支是 `main`
- `main` 与 `origin/main` 同步
- 工作区干净
- `gh` 已登录
- 本地和远端不存在同名 tag
- GitHub Release 不存在
- Rust / TUI / Web / Desktop / TS SDK 版本号与指定版本一致

脚本不会自动修改版本号，也不会上传本地编译产物。

---

## 架构设计

```
┌─────────────────────────────────────────┐
│           上层应用 (Jarvis)              │
├─────────────────────────────────────────┤
│              Talos 框架                   │
├─────────────┬─────────────┬─────────────┤
│   感知层    │   认知层    │   执行层    │
│ Perception  │ Cognition   │   Action    │
│   (TUI/CLI) │ (Memory/LLM)│ (Tools/MCP) │
├─────────────┴─────────────┴─────────────┤
│            编排层 / 网关层               │
│         (Agent/Session/Gateway)         │
├─────────────────────────────────────────┤
│            元认知层 (Meta)               │
│     自我监控 / 持续学习 / 人格管理        │
└─────────────────────────────────────────┘
```

---

## 开发状态

🚧 **早期开发中** - 核心架构设计阶段

- [x] 项目结构搭建
- [x] Rust 实现骨架
- [ ] 核心模块实现
- [ ] 多语言接口规范
- [ ] 第一个可用版本

---

## 许可证

MIT License

---

## 相关项目

- **Jarvis**：基于 Talos 构建的上层 AI 智能管家应用
- **OpenClaw**：灵感来源，Node.js 版本的助手框架
