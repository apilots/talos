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

## Goal、Loop 与 Automation

`/goal`、`/loop` 复用其他客户端的同一命令合同。slash 建议展开时 Enter / Tab
选择一次性待发送模式，在原输入框填写任务后发送；也可直接提交 `/goal <提示词>`
或 `/loop <提示词>`，沿原 Turn / Queue 输入携带同一个 System Skill 选择。
Alt+Backspace 只移除模式并保留提示词。发送或入队成功后消费模式，失败保留草稿。
裸 `/goal` / `/loop` 查看状态时，先 Esc 关闭建议再 Enter；`/loop 5m 检查 CI`
等完整命令直接提交。

已完成、停止或清除的任务自动离开输入框上方面板；只保留进行中 / 暂停的 Goal
及已启用的 Loop。已完成记录仍可通过 `/goal` 或 `/automation ended` 查看。

Ctrl+G 展开任务与等待消息的共用列表。选中任务后 Enter 暂停 / 恢复 Goal 或停止
Loop，`e` 编辑，`d` 清除。Goal 编辑保留预算；Loop 编辑通过 Tab 切换固定 / 模型
决定，Ctrl+↑↓ 调整固定分钟数（至少 1），只影响后续执行。原 Ctrl+Q 队列操作
保留：Goal / Loop 运行时普通输入先入队，逐行 Steer 干预同一运行 Turn；等待人工
动作时仍遵守原准入限制。

`/automation` 查看自己可访问的任务，`/automation ended` 查看已结束项；
`/jobs COMPUTER_ID list|pause|resume|run-once [JOB_ID]` 经真实电脑 / 执行实例归属
API 管理内置任务，不增加管理员 / 用户分层，也不把这些任务暴露为模型工具。

来源仅依据已准入 Input 的精确消息 ID，在底部添加轻量 Goal / Loop 标记，后续
普通 Steer 不误标。已经输出的 native scrollback 不能原位补 metadata，也不会因此
重印整条输入或新增来源卡片；正常重载历史会恢复准确 footer。新的自动 Input 在原
重连路径允许的最早时点读取已有 active metadata。

CLI 命名入口必须绑定已有 Session，例如：

```bash
talos goal --session SESSION_ID start "修复失败的检查"
talos goal --session SESSION_ID edit "补充验收要求"
talos goal --session SESSION_ID pause
talos goal --session SESSION_ID resume
talos loop --session SESSION_ID start --every 5 "检查 CI"
talos loop --session SESSION_ID start --dynamic "观察有意义的变化"
talos loop --session SESSION_ID edit LOOP_ID --every 2 "检查 CI 并报告变化"
talos loop --session SESSION_ID stop LOOP_ID
talos automation list --ended
talos automation jobs --computer COMPUTER_ID list
```

`goal start`、`goal resume`、`loop start` 在前台保持原 embedded owner 存活，直到
精确目标被观察为暂停、完成、停止或移除。新建目标取 owner 返回的类型化
创建引用，不按提示词猜测。需保持进程运行：定义持久化不表示离线执行，也不增加
后台 daemon。其他管理命令只更新状态，不提供进程退出后的执行宿主。正常退出
复用原 client shutdown；SIGINT 保持原进程终止行为。内置 Job 运行一次沿原
scheduler shutdown / drain 完成，不另加轮询或调度器。

Goal 总耗时为当前时间减创建时间，包含暂停且跨 Turn 不归零；完成后按稳定的更新时间定格。Loop 只在本 Loop 精确触发 Input 对应的当前 Turn 显示本轮计时，空闲显示下次时间或结束结果。固定 Loop 无统一到期限制，模型决定模式可在正常结算后停止；这些只是时间展示，不运行客户端调度。

Slash 只选模式并原样发送提示词，`/loop 5m 检查 CI` 保留 `5m 检查 CI`，由模型判断周期，不在创建时增加周期表单。命名 CLI 的 `--every` / `--dynamic` 直接构造类型化动作，不拼接 slash 解析；模型决定且无已知下次时间时仅显示该模式，不承诺已有唤醒。
