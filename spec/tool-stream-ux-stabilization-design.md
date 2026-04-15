# Tool Stream UX Stabilization Design

## 1. 背景

当前 Talos 已经具备流式工具执行能力，但真实 TUI 体验仍存在几个明显问题：

- 模型会在多轮中重复发起等价的只读探索，最后触发 `tool-call rounds exceeded`
- TUI 将同一个工具调用拆成过多 block，造成“同一个工具出现很多次”的观感
- `queued/executing` 生命周期事件会覆盖真实工具参数，导致命令、路径、pattern 等关键信息丢失
- stale / out-of-order tool lifecycle event 可能污染序号状态，导致最终 assistant 输出被忽略
- 工具执行反馈粒度不合理，暴露了太多低价值细节，但又没有稳定保住真正有用的信息

本 spec 的目标是把工具调用体验收敛到一套更稳定、更像 `claude-code`、但更适合 Talos 当前架构的统一方案。

## 2. 目标

本次整改后的客户端体验应收敛为：

`User -> Assistant/Thinking -> Tool started -> Tool live status/progress -> Tool completed -> Assistant/Thinking -> ... -> Final answer`

具体要求：

- 客户端只需要稳定看到三类工具反馈：
  - 开始
  - 实时状态 / 命令行执行进度
  - 完成
- 不需要把完整 lifecycle 细节全量暴露给用户
- 不需要把所有中间 payload 都渲染成独立 transcript block
- 工具调用仍保持 model-driven，不通过 flow 层硬编码强制选工具
- 服务端必须对明显重复、无新增信息的只读探索做收敛保护
- 到达 tool round budget 时，不能直接把预算错误暴露为最终回答

## 3. 非目标

- 不在本轮引入新的强制工具路由规则
- 不在本轮重写 planner 或替换模型
- 不在本轮为每个工具都做 Claude Code 等级的专门 UI
- 不要求完整暴露每一条 tool args delta
- 不要求在 transcript 中永久保留所有 progress 历史

## 4. 设计原则

### 4.1 Model-driven，执行层只做护栏

工具选择仍由模型决定。服务端不做“用户说了 grep_files 就强制 grep_files”的硬拦截。  
但服务端可以做以下护栏：

- 阻止明显等价的重复只读调用
- 在预算耗尽前强制收束到总结
- 为客户端保留最小但稳定的调用元数据

### 4.2 用户只看高价值反馈

用户关注的是：

- 现在在做什么
- 是否正在执行
- 命令行是否有进展
- 做完了什么

用户不需要看到：

- 每个生命周期事件的完整内部状态机
- 重复的 `queued/executing/status` 文案
- 每次参数增量拼接过程
- 大量重复、无上下文价值的 JSON 细节

### 4.3 单个工具调用只应有一个主卡片

一个 `call_id` 在 transcript 中必须对应一个主 Tool 卡片。  
状态和进度应附着在这张卡片上，不能继续扩散成多个长期残留的 block。

## 5. 目标体验

### 5.1 Transcript 语义

每个工具调用只在主 transcript 中表现为一个 Tool block，状态演进如下：

1. `started`
2. `running`
3. `completed`

如果工具支持实时进度：

- `running` 期间可持续附加短状态行
- 对 `exec_command` / `write_stdin` 这类命令执行工具，允许显示 stdout/stderr 进度摘要

### 5.2 不再单独渲染的内容

以下事件不再直接变成独立 transcript block：

- `queued`
- `executing`
- `status progress`
- `args delta`

它们只用于更新当前 Tool block 的内部状态。

### 5.3 对用户可见的最小信息

每张 Tool 卡片最少保留：

- tool name
- 一条简短 input summary
- 当前状态
- 若适用：最近几条命令行进度
- 完成后的结果摘要

默认不展示完整输入 JSON，不展示完整 lifecycle 日志。  
仅在展开时展示更完整内容。

补充要求：

- 所有相关工具默认必须显示一行稳定的调用摘要，统一采用 `tool_name(...)` 风格
- 例如：
  - `exec_command(ls -la /path)`
  - `read_file(/abs/path/file.rs)`
  - `list_dir(/abs/path/project)`
  - `grep_files("AgentKernel", path=crates/orchestration)`
- 该命令摘要属于高价值上下文，不允许在 `queued/executing/progress/completed` 生命周期流转中丢失
- 调用摘要可以截断，但必须保留 `tool_name(` 前缀和主要参数，不能退化成空 `{}` 或完全不可见

## 6. 服务端设计

### 6.1 工具流事件契约

服务端继续维护现有内部事件，但对客户端的语义目标收敛为：

- `tool_started`
- `tool_progress`
- `tool_completed`

现有 `queued / executing / args_delta` 仍可保留在内部流中，但客户端不应再把它们当成一等可见 transcript 事件。

### 6.2 Canonical invocation snapshot

服务端必须为每个工具调用保留稳定的 invocation snapshot：

- `call_id`
- `tool_name`
- `arguments`
- `stream_key`

要求：

- `arguments` 在 `ToolCallStart` 确认后成为该调用的 canonical input
- 后续生命周期事件不能再覆盖该 input
- `ToolResult` 和 `ToolProgress` 若未重复携带 `arguments`，客户端也必须能通过 `call_id` 找回该 snapshot

### 6.3 重复只读探索收敛

服务端新增只读调用指纹判断：

`fingerprint = tool_name + canonicalized(arguments)`

适用范围：

- `list_dir`
- `read_file`
- `grep_files`
- 其他明确标记为 read-only 的 inspection/search 工具

当满足以下条件时，认定为“等价重复探索”：

- 同一 turn 内
- 近期窗口内已执行过相同 fingerprint
- 结果未发生变化，且没有新的外部状态输入

命中后不直接再次执行；而是进入收敛逻辑：

- 记录一次 convergence warning
- 触发一次“基于当前结果总结或换方向”的模型收束机会

这不是工具选择硬规则，而是执行层的防抖护栏。

### 6.4 Tool round budget 到达时的收尾

不允许把 `"[tool-call rounds exceeded]"` 直接作为用户最终回答。

整改后流程：

1. 达到 round budget
2. 停止继续接受新的 tool call
3. 用已有 `state.messages + tool results + reasoning log` 发起最后一次“禁止工具、仅总结”的收束请求
4. 若收束成功，向用户返回最终总结
5. 若收束失败，再返回显式的系统级错误说明，而不是原始 budget 文本

### 6.5 Progress 事件约束

对客户端下发的 progress 只保留高价值内容：

- `status`: 短状态，如 `waiting`, `running`, `approval needed`
- `stdout`: 命令执行输出摘要
- `stderr`: 错误输出摘要

要求：

- 相邻重复状态不重复发
- progress message 需要短、可读、与用户任务相关
- 不要求输出完整原始 stdout/stderr
- 允许仅发送最近窗口摘要而不是全量日志

## 7. TUI 设计

### 7.1 单 call_id 单主卡片

TUI transcript reducer 必须保证：

- 一个 `call_id` 只对应一个主 Tool block
- `started` 事件创建 block
- `progress` 更新 block 内的状态区域
- `completed` 更新结果区域并终态化

禁止以下行为：

- 为同一个 `call_id` 额外创建长期存在的 `ToolProgress` block
- `queued/executing` 重新创建或覆盖主 Tool block

### 7.2 状态显示

TUI 只展示以下用户级状态：

- `Started`
- `Running`
- `Completed`
- `Error`
- `Pending Approval`

内部 `queued/executing` 如有必要，应映射到上述状态，而不是原样外露。

建议映射：

- `queued` -> `Started`
- `executing` -> `Running`

### 7.3 命令行进度渲染

对命令型工具：

- 主卡片内显示最近若干行 stdout/stderr 摘要
- 默认只保留有限窗口，例如最近 5-10 行
- 完成后保留精简摘要，不保留整段冗长输出

对非命令型工具：

- progress 只显示状态短句
- 不展示 args delta 拼接过程

### 7.4 Input summary 渲染

每张 Tool 卡片都要有稳定 input summary。

最少覆盖：

- `exec_command`: `cmd`, `workdir`
- `read_file`: `path`
- `list_dir`: `path`
- `grep_files`: `pattern`, `path`, `include`

即使 renderer 没有专门适配，也必须有 generic summary fallback，不能退化成空 `{}`。

统一格式要求：

- 默认摘要优先显示为 `tool_name(...)`
- 摘要内容应尽量贴近用户心智中的“这次调用在做什么”
- 若参数较多，优先保留最能标识目标对象的核心参数
- 非结构化 JSON 不能直接原样替代摘要

对各类工具的最低格式要求：

- `exec_command`: 默认显示 `exec_command(...)`
- 若同时存在 `workdir`，显示为第二行或附加摘要
- `stdout/stderr` 进度是补充信息，不能替代 `exec_command(...)`
- `read_file`: 默认显示 `read_file(...)`
- `list_dir`: 默认显示 `list_dir(...)`
- `grep_files`: 默认显示 `grep_files(...)`
- 完成后主卡片仍必须保留这行 `tool_name(...)` 摘要，不能只剩状态、exit code 或输出 preview

### 7.5 事件有效性

TUI 必须对 tool lifecycle event 也执行当前 stream 有效性判断。

要求：

- stale tool event 不能污染 `last_stream_sequence`
- 非当前 message / stream 的 tool event 必须被忽略
- 被忽略事件不得影响后续 `assistant delta` 和 `message completed`

## 8. 数据模型要求

客户端 reducer 至少需要维护：

- `call_id -> tool block idx`
- `call_id -> canonical invocation snapshot`
- `call_id -> latest status`
- `call_id -> recent progress lines`

约束：

- `ToolCallStarted.arguments` 写入 canonical snapshot 后不可被 `{}` 覆盖
- `ToolCompleted` 只能更新结果，不得清空 input summary
- stale event 不得改写 snapshot

## 9. 验收标准

### 9.1 用户体验验收

针对“分析一下当前项目架构，请使用 grep_files 工具”这类问题，整改后应满足：

- transcript 中每个工具调用只出现一张主 Tool 卡片
- 卡片必须显示足够的 input summary
- `grep_files` / `list_dir` / `read_file` 不再频繁丢失参数
- 中途可以看到 `Running` 和命令执行进度
- 不再出现大量重复 status block
- 最终必须有 assistant 总结，不允许直接停在 `tool-call rounds exceeded`

针对 `exec_command` 场景，额外应满足：

- 主卡片默认可见 `exec_command(...)`
- 同一次调用在开始、运行中、完成后都能看到相同的调用摘要
- 运行中可追加 stdout/stderr 摘要，但不能把 `tool_name(...)` 摘要顶掉

针对所有相关工具，额外应满足：

- 主卡片默认可见 `tool_name(...)`
- 同一次调用在开始、运行中、完成后都能看到相同的调用摘要
- 状态和进度只能补充在摘要下方，不能替代摘要本身

### 9.2 服务端行为验收

- 相同只读 fingerprint 的重复调用会被收敛，而不是无限继续
- round budget 耗尽时会进入 forced finalization，而不是直接暴露 budget 文本
- tool progress 只发送有限、高价值、去重后的消息

### 9.3 TUI 行为验收

- stale tool event 不会吞掉最终 assistant 输出
- `queued/executing` 不再覆盖已记录的工具参数
- `ToolProgress` 不再以长期独立 block 泛滥出现
- 右侧工具状态面板按 `call_id` 聚合，只显示最新状态

## 10. 测试计划

### 10.1 单测

- stale tool event 不推进全局 sequence
- `queued/executing` 不覆盖 canonical input
- 同一 `call_id` 仅对应一个 Tool block
- progress 仅更新已有 Tool block 的内部状态
- round budget 命中后触发 forced finalization

### 10.2 集成测试

- 重复 `list_dir/read_file` 场景触发 convergence，而不是无限继续
- `exec_command` 能流式显示 stdout/stderr 摘要
- 最终 assistant summary 在混合 tool lifecycle event 下仍能正常显示

### 10.3 手工验证

使用 `tmux` 启动真实 server + TUI，验证至少三类问题：

1. 代码结构分析
2. 目录/文件探索
3. 命令执行与进度反馈

截图和 pane dump 应重点确认：

- 有没有重复工具卡片
- 命令/路径/pattern 是否稳定可见
- 最终回答是否稳定落地

## 11. 实施顺序

1. 先修 TUI 事件有效性与 canonical input 保留
2. 再收敛 Tool block / Progress block 语义
3. 再做服务端重复探索收敛
4. 最后改 round-limit forced finalization
5. 完成后跑 e2e + `tmux` 手测

## 12. 风险

- 若只修 TUI、不修服务端收敛，用户仍会看到真实的重复工具调用
- 若只修服务端、不修 TUI，输入丢失和状态噪音仍会存在
- 若 forced finalization 做得过早，可能压制合理的后续工具探索

因此本次整改必须把服务端收敛和 TUI 语义作为同一轮工作交付。
