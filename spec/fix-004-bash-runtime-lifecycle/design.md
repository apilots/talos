# Bash Runtime 生命周期修复方案

Status: partially implemented
Owner: agent
Created: 2026-07-06
Updated: 2026-07-06

## 背景与问题

当前 Bash 工具执行链路把长任务能力建模为 `Bash -> TaskOutput/TaskStop`。这导致 Bash 的进程生命周期、模型可见工具、TUI 展示块之间边界混乱。

已确认的问题：

- 短命令可能在 stdout 先到、exit code 后到的竞态窗口内被记录为 `running`。
- 如果模型没有后续调用 `TaskOutput`，历史 Bash block 会长期停留在 running 状态。
- `TaskOutput` 和 `TaskStop` 当前是默认可见工具，但它们实际只是在操作 Bash runtime 的内部会话。
- `TaskOutput.task_id` 实际对应 Bash `session_id`，命名和语义不一致。
- TUI 被迫兼容 `TaskOutput` block，泄漏了底层轮询实现，而不是展示一个连续的 Bash 执行块。

Codex 的 `unified_exec` 提供了可参考的方向：进程启动后由 runtime 持续推送输出增量和结束事件，模型不需要通过另一个工具轮询 Bash 状态。

## 目标

- Bash 自己管理一次命令执行的完整生命周期。
- Bash 输出、完成、失败、超时、取消都通过同一个 `tool_call_id` 更新同一个 TUI block。
- `TaskOutput` / `TaskStop` 不再作为模型可见工具参与正常 Bash 流程。
- `run_id` 仅作为 Bash runtime 内部执行句柄，不进入模型上下文、不进入 TUI 文案、不作为客户端 API。
- 用户 Esc / cancel turn 能传递到 Bash runtime，并停止当前 turn 下仍在运行的 Bash 进程。
- 修复短命令被误判为 running 的竞态。
- 新 session 和 TUI replay 都不再依赖用户可见 `TaskOutput` 流程。

## 非目标

- 不引入 `BashStop` 作为新的模型可见工具。
- 不向模型暴露 `run_id`、OS `process_id` 或 Bash 内部进程句柄。
- 不把 Bash runtime 重写成通用任务平台。
- 不改变 Read、Grep、Glob、Edit、Write 等工具的业务语义。
- 不在 TUI 中继续强化 Task 概念。
- 不依赖模型主动调用轮询工具来闭合 Bash 状态。

## 当前实现事实

设计阶段审计到的相关实现位置：

- `talos-rs/crates/action/tools/src/tools/bash.rs`
- `talos-rs/crates/action/tools/src/runtimes/bash.rs`
- `talos-rs/crates/action/tools/src/lib.rs`
- `talos-rs/crates/action/tools/src/registry.rs`
- `talos-rs/apps/tui/src/stream_events.rs`
- `talos-rs/apps/tui/src/render/tool/bash.rs`
- `talos-rs/apps/tui/src/render/tool_lines.rs`

设计阶段事实：

- `TaskOutputTool` 和 `TaskStopTool` 注册在 core local tools 中。
- 两者 `visibility()` 均为 `ToolVisibility::DefaultVisible`。
- Bash runtime 对外响应字段为 `session_id: Option<i64>`。
- Bash runtime 内部持有 `process_id: Option<u32>`，仅用于进程停止。
- `TaskOutputInput.task_id` 映射到 Bash runtime 的 `session_id`。
- 旧实现曾在 TUI 中对 `TaskOutput` 做内部兼容判断，但这只是展示层修补，无法修复后端没有完成事件的问题。

## 本轮落地范围

本轮完成的修复：

- Bash tool schema 移除 `run_in_background`，`timeout` 表示命令最大运行时。
- Bash tool 默认等待到命令完成或 timeout，不再要求模型继续调用 `TaskOutput`。
- Bash runtime 拆分 `yield_time_ms` 与 `timeout_ms`，并修复短命令 stdout 先到、exit code 后到导致的 running 残留竞态。
- Bash runtime 增加 timeout 状态，超时返回 exit code `124`，并按 process group 终止子进程树。
- 模型可见 `TaskOutput` / `TaskStop` 工具从 core local tools、tool policy 和默认可见工具列表中移除。
- 新 session 不再生成 `Bash -> TaskOutput` 的正常流程。
- Tool runtime 获得 `ToolProgressEmitter`，Bash stdout/stderr reader 直接发出工具 progress。
- Streaming tool scheduler 将 tool progress 转换为现有 `ToolProgressDelta` timeline event。
- TUI 删除 `TaskOutput` 特判、`active_bash_sessions` 合并表和旧 replay 测试，只消费统一 timeline 的 tool progress / result。

本轮未完成的目标：

- Esc/cancel turn 到 Bash process group 的完整取消链路尚未接入。

## 设计决策

### 1. 对外关联只使用 `tool_call_id`

TUI 和 timeline 更新同一个 Bash block 时使用 `tool_call_id`。

`tool_call_id` 表示模型本次工具调用，是 UI block 和事件流的稳定归属 ID。

### 2. `run_id` 只在 Bash runtime 内部使用

Bash runtime 每次执行命令生成内部 `run_id`，用于管理：

- 进程句柄
- stdout/stderr reader
- timeout
- cancellation
- transcript buffer
- exit watcher

`run_id` 不进入：

- 模型 tool result
- prompt context
- TUI 文案
- SDK/API wire payload
- 用户可复制 transcript

如果跨层必须关联，后端内部维护：

```text
tool_call_id -> run_id -> process/process_group
```

### 3. 不暴露 OS `process_id`

OS `process_id` 只用于 Bash runtime 内部进程控制。

原因：

- OS pid 可能被复用。
- OS pid 不是协议稳定对象。
- kill 单个 pid 不一定能停止完整子进程树。
- 暴露 OS pid 会把平台实现细节泄漏给 UI 和模型。

### 4. Tool 自己推送 progress，final output 仍走统一 result

Tool runtime 通过 `ToolContext` 中的 `ToolProgressEmitter` 推送 best-effort 进度：

- Bash stdout/stderr reader 持续写入 runtime transcript。
- stdout/stderr chunk 同步发出 `ToolProgressDelta`。
- final tool output 仍通过 `ToolResult` 返回权威结果。
- 同一个 `tool_call_id` 的 progress / result 更新同一个 TUI tool block。

当前落地事件语义：

```text
tool_call_started(Bash, tool_call_id)
tool_progress_delta(tool_call_id, stdout/stderr/status chunk)
tool_result(tool_call_id, content, is_error)
```

已复用现有 timeline / stream event 抽象，没有新增 Task 类工具或新的客户端协议对象。

### 5. `yield_time_ms` 和 `timeout_ms` 分离

必须区分两种时间语义：

- `yield_time_ms`：首个 Bash tool response 等待窗口。
- `timeout_ms`：命令最大运行时间，到期后 runtime 主动停止进程。

当前把 timeout 当成 yield 使用会导致行为不清晰，应在 Bash 输入模型中明确拆分。

### 6. 修复短命令竞态

当前短命令可能因为 stdout notify 先到而 exit code 尚未写入，导致返回 running。

修复策略：

- 收到输出后不能立即判定为 running。
- 增加 early exit grace，例如 100-150ms。
- 进程退出后等待 output drained grace，确保尾部输出进入 transcript。
- 只有确认进程仍存活且超出初始等待窗口，才返回 running。

### 7. Esc / cancel turn 必须停止 Bash

用户 Esc 中断不应只停止模型流。

目标链路：

```text
TUI Esc -> cancel active turn -> turn runtime cancellation -> active tool runs -> Bash runtime kill process group
```

Bash runtime 应尽量停止 process group，而不是只 kill shell pid，避免子进程残留。

### 8. 移除模型可见 `TaskOutput` / `TaskStop`

目标态：

- `TaskOutputTool` 已从代码中删除。
- `TaskStopTool` 已从代码中删除。
- 新 session 不再通过 `TaskOutput` 推进 Bash 状态。
- TUI 不保留 `TaskOutput` 特判或 session_id 合并路径。

如果未来仍需要 transcript 查询能力，应作为内部 runtime/debug API，而不是模型 tool。

### 9. TUI 展示策略

TUI 不展示 `run_id`、`process_id`、Task 概念。

展示规则：

- Bash 运行中显示同一个 block 的 running 状态。
- stdout/stderr progress 更新同一个 block。
- 完成后原 block 更新为 completed/failed/cancelled/timeout。
- 输入框上方运行态提示由 active tool runs 决定，完成后清理。
- 最终回答继续按原始 Markdown/纯文本流式输出，不做 Markdown renderer 结构化渲染。

## 影响范围

后端：

- Bash runtime 生命周期管理。
- Tool registry 中 Task 工具可见性与注册。
- turn cancellation 到 active tool run 的传播。
- timeline / stream event 的工具输出增量和结束事件。

TUI：

- Bash block 状态更新。
- 历史 TaskOutput 兼容。
- 运行态提示清理。
- 快照测试更新。

测试：

- Bash runtime 单元测试必须移动或保留在独立 `*_tests.rs` 文件。
- Agent/tool 集成测试覆盖 Bash 不再依赖 TaskOutput。
- TUI snapshot 覆盖 running、delta、completed、failed、timeout、cancelled。

## 验证策略

必须覆盖以下行为：

- `echo hi` 直接 completed，不遗留 running。
- `sleep 0.2 && echo done` 自动 completed，无需 TaskOutput。
- 循环输出命令持续更新同一个 Bash block。
- `exit 1` 显示 failed 和 exit code。
- 超过 `timeout_ms` 的命令被停止并显示 timeout。
- Esc/cancel turn 能停止当前 Bash 进程组。
- 新工具目录不包含模型可见 `TaskOutput` / `TaskStop`。
- TUI 源码无 TaskOutput 特判、active bash session 合并表或旧反向测试残留。

## 风险与后续

- 取消链路可能涉及 turn runtime、tool runtime、TUI 三层，需要保持边界清晰，避免把 UI 状态塞进 Bash runtime。
- 如果现有 timeline event 类型不足，应先补最小强类型事件，不要用松散 JSON 字段临时传递。
- 移除可见 Task 工具会改变模型可用工具列表，需要同步相关测试和 prompt/tool catalog 期望。
- 历史 session 兼容应限制在读取层，不应让新逻辑继续生成 TaskOutput。
