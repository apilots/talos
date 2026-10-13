# Talos Terminal

[English](README.md)

`talos-tui` 构建公开的 `talos` 终端二进制。该二进制同时包含交互式 TUI 和非交互式
command mode，两者使用同一个 embedded Talos Runtime 和 Rust SDK adapter。

## 使用

安装最新目标平台 release bundle：

```bash
curl --proto '=https' --tlsv1.2 -fsSL \
  https://github.com/apilots/talos/releases/latest/download/install.sh | sh
```

Installer 将不可变 release 资源放在 `$TALOS_HOME` 之外。版本固定、路径和安装 E2E
参见[安装器文档](../../../deploy/install/README_zh.md)。

```bash
cargo run --manifest-path talos-rs/Cargo.toml -p talos-tui --bin talos
cargo run --manifest-path talos-rs/Cargo.toml -p talos-tui --bin talos -- tui
cargo run --manifest-path talos-rs/Cargo.toml -p talos-tui --bin talos -- exec "hello"
cargo run --manifest-path talos-rs/Cargo.toml -p talos-tui --bin talos -- exec --session <session-id> "continue"
cargo run --manifest-path talos-rs/Cargo.toml -p talos-tui --bin talos -- models
cargo run --manifest-path talos-rs/Cargo.toml -p talos-tui --bin talos -- session list
```

## Command mode

- `exec`：启动一个 Agent Turn，并把 Timeline 输出到 stdout/stderr。
- `models`：列出可用模型。
- `session list`：列出最近 Session。
- `session info <session-id>`：显示 Session metadata。
- `session set-model <session-id> <model>`：更新 Session 模型。
- `status`：显示 embedded Runtime 可用性和基础统计。

Command mode 复用 TUI `GatewayClient` adapter，不维护独立 CLI Runtime。

## Interactive TUI

TUI 支持聊天与 Timeline streaming、模型选择、Session history、工具执行状态以及
`/models`、`/history`、`/sessions`、`/status`、`/compact`、`/clear`、`/quit`
等命令。

## Runtime

终端二进制在进程内运行 embedded Talos Runtime，不启动 app-server 子进程，也不监听本地端口。
