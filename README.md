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
- 使用 `cargo run -p talos-ui-tui --bin talos` 运行
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
cd talos-rs

# 构建
cargo build

# 运行测试
cargo test

# 运行示例
cargo run --example basic
```

### 其他语言实现

待开发...

### TUI使用示例
```bash
# 启动 TUI 界面（默认 embedded runtime）
cd talos/talos-rs
cargo run -p talos-ui-tui --bin talos

# TUI/CLI 直接使用 embedded runtime，不需要额外启动 app-server

# 运行 CLI（默认 embedded runtime）
cargo run -p talos-cli --bin talos-cli -- models

# 启动 HTTP/WebUI 服务端
cargo run -p talos-server --bin talos-server
```

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
