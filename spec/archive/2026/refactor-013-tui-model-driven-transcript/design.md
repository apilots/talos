# Talos TUI Model-Driven Transcript Design

**版本**: v1.0  
**状态**: Draft  
**最后更新**: 2026-04-15  
**关联文档**:
- `spec/streaming-interleaved-execution-part1.md`
- `spec/streaming-interleaved-execution-part2.md`
- `spec/tui-improvement.md`
- `spec/tui-visual-tooling-refinement.md`
**对标参考**:
- `~/data/ailabs/claude-code/src/constants/prompts.ts`
- `~/data/ailabs/claude-code/src/query.ts`
- `~/data/ailabs/claude-code/src/services/tools/StreamingToolExecutor.ts`
- `~/data/ailabs/claude-code/src/components/Messages.tsx`
- `~/data/ailabs/claude-code/src/components/Message.tsx`
- `~/data/ailabs/claude-code/src/components/messages/AssistantToolUseMessage.tsx`

---

## 1. 背景

Talos 已经具备以下基础能力：

- 服务端能输出 `MessageReasoningDelta`
- 服务端能输出 `MessageOutputDelta`
- 服务端能输出完整的工具生命周期事件：
  - `MessageToolCallStarted`
  - `MessageToolCallArgsDelta`
  - `MessageToolCallQueued`
  - `MessageToolCallExecuting`
  - `MessageToolCallProgressDelta`
  - `MessageToolCallCompleted`
- TUI 已能在同一轮中穿插显示 Assistant 文本与工具块

但当前体验与 `claude-code` 仍有关键差距：

1. 首段通常先出现一个预创建的 `Thinking` block，而不是稳定的“短 Assistant / 短 Thinking -> action”
2. TUI 仍按粗粒度 `ChatRole` 组织内容，而不是按 transcript block 语义组织
3. `ToolProgressDelta` 仍更像 tool block 内部日志，而不是独立 progress message
4. 当前 display path 更接近“事件流回放”，而不是“消息块编排”

本方案不再改调度器设计，而是补齐一层更接近 `claude-code` 的 transcript 语义。

---

## 2. 目标

### 2.1 目标体验

目标体验是：

```text
User input
  -> short Assistant or short Thinking
  -> ToolUse block
  -> ToolProgress message(s)
  -> Assistant / Thinking
  -> next ToolUse block
  -> ToolProgress message(s)
  -> final Assistant summary
```

允许的首段形态：

1. `User -> Assistant(short) -> ToolUse`
2. `User -> Thinking(short) -> ToolUse`
3. `User -> ToolUse`

其中第 1 种是首选，第 2 种和第 3 种允许存在，但系统不应伪造文本去“补齐”它们。

### 2.2 非目标

本方案不解决：

1. Scheduler 并发策略重写
2. Provider 侧 reasoning 质量优化
3. PromptBuilder 的整体重构
4. Web UI 对齐

---

## 3. 设计原则

### 3.1 纯模型驱动

服务端只转发真实的模型输出和工具事件，不伪造：

- 不伪造首句 Assistant
- 不伪造 Thinking 文本
- 不因工具即将开始而插入假的说明句

### 3.2 Transcript 优先于日志

TUI 的一等概念不再是“收到了一条事件”，而是“当前 transcript 上形成了什么 block”。

### 3.3 Thinking 默认可见

为了研发排查和体验验证，当前阶段不隐藏历史 Thinking block。  
与 `claude-code` 的隐藏旧 thinking 策略不同，Talos 开发阶段保留完整可见性。

### 3.4 Tool Progress 独立成块

`ToolProgressDelta` 不再只是 tool block 的附属明细。  
它在 transcript 中是独立的 progress message，但仍归属于某个 `call_id`。

---

## 4. 首段规则

### 4.1 服务端先返回什么

服务端不强制首段必须是 Assistant 或 Thinking，而是遵循以下规则：

1. 如果模型先输出用户可见文本，服务端先发 `MessageOutputDelta`
2. 如果模型先输出 reasoning，服务端先发 `MessageReasoningDelta`
3. 如果模型直接发 tool call，服务端先发 `MessageToolCallStarted`

服务端唯一需要保证的是：

- 顺序忠实于上游模型和 framework 的真实输出
- 不因为 TUI 期待某种首段形态而篡改顺序

### 4.2 TUI 首段展示规则

TUI 必须遵循以下首段处理策略：

1. 不再在发起请求时预创建空 `Thinking` block
2. 只有收到首个非空 `MessageReasoningDelta` 时，才创建 `Thinking` block
3. 只有收到首个非空 `MessageOutputDelta` 时，才创建 `AssistantText` block
4. 如果首个有效事件是 `MessageToolCallStarted`，直接插入 `ToolUse` block

这意味着：

- `Thinking` 不再是默认占位符
- 首屏结构由真实增量决定
- 首段既可能是 Assistant，也可能是 Thinking，也可能直接是 ToolUse

补充规则：

5. 如果当前 live `AssistantText` block 存在但内容为空，且首个有效工具事件已经到达，则该空 block 不提升为正式 transcript block
6. 对应实现可以选择“延迟创建 AssistantText block”或“创建后在首个 tool call 到达时删除空 block”，两种策略都允许，但最终 transcript 中不能残留空 AssistantText block

### 4.3 首段优先级

如果在首个 tool call 前既出现了 `Thinking` 也出现了 `Assistant`：

```text
User
  -> Thinking
  -> Assistant
  -> ToolUse
```

TUI 按实际到达顺序渲染，不做重排。  
当前阶段不把 reasoning 改写为 Assistant，也不把 Assistant 折叠回 Thinking。

---

## 5. Transcript Block 语义

### 5.1 Block 类型

TUI 从粗粒度 `ChatRole` 升级到更明确的 transcript block 语义。

建议新增：

```rust
pub enum TranscriptBlockKind {
    UserInput,
    AssistantText,
    Thinking,
    ToolUse,
    ToolProgress,
    SystemNotice,
}
```

### 5.2 Block 职责

| Block | 作用 | 是否可增量更新 |
|------|------|----------------|
| `UserInput` | 用户输入 | 否 |
| `AssistantText` | 用户可见回复正文 | 是 |
| `Thinking` | reasoning / framework thinking | 是 |
| `ToolUse` | 工具调用本体与终态摘要 | 是 |
| `ToolProgress` | 工具执行过程中的独立进度消息 | 是 |
| `SystemNotice` | 流错误、审批提示、状态提示 | 是 |

### 5.3 ToolUse 与 ToolProgress 分工

`ToolUse` block 负责：

- 工具名称
- 参数摘要
- 生命周期状态
- 最终结果摘要
- 展开后的结构化输入/输出

`ToolProgress` block 负责：

- 状态进展
- stdout 摘要
- stderr 摘要
- 等待审批 / 等待外部返回这类中间态

换句话说：

- `ToolUse` 是“这次调用是什么”
- `ToolProgress` 是“这次调用现在发生了什么”

---

## 6. Event 到 Block 的映射

### 6.1 总体流程

```text
SSE stream event
  -> transcript reducer
  -> create / update / finalize transcript blocks
  -> render blocks in left pane
```

### 6.2 映射表

| Event | Transcript 行为 |
|------|------------------|
| `MessageReasoningDelta` | 追加到当前 `Thinking` block，若不存在则创建 |
| `MessageOutputDelta` | 追加到当前 `AssistantText` block，若不存在则创建 |
| `MessageToolCallStarted` | 创建或更新 `ToolUse` block |
| `MessageToolCallArgsDelta` | 更新对应 `ToolUse` block 的参数流摘要 |
| `MessageToolCallQueued` | 更新 `ToolUse.status = queued` |
| `MessageToolCallExecuting` | 更新 `ToolUse.status = executing` |
| `MessageToolCallProgressDelta` | 创建或追加 `ToolProgress` block |
| `MessageToolCallCompleted` | 更新 `ToolUse.status = completed/error`，并终结对应 live progress |
| `MessageToolCallPendingApproval` | 生成 `SystemNotice` 或 `ToolProgress(waiting_approval)` |
| `MessageError` | 生成 `SystemNotice(error)` |

### 6.3 关键约束

1. `ToolProgress` 不写回 `ToolUse.tool_progress: Vec<String>` 作为唯一展示来源
2. `ToolUse` 和 `ToolProgress` 共享 `call_id`
3. 一个 `ToolUse` 可以对应多个 `ToolProgress` block
4. `ToolUse` 完成后，对应 live `ToolProgress` block 必须终态化
5. `MessageToolCallArgsDelta` 的主关联键是 `stream_key`
6. `call_id` 在 `ArgsDelta` 阶段只是可选补充字段，不应假定始终存在

---

## 7. Tool Progress 独立消息模型

### 7.1 目标

对齐 `claude-code` 的核心体验：

- tool card 表示“调用本体”
- progress message 表示“执行过程”

而不是所有进度都塞进同一张 tool card。

### 7.2 数据结构

```rust
pub enum ToolProgressDisplayKind {
    Status,
    Stdout,
    Stderr,
}

pub struct ToolProgressBlock {
    pub call_id: String,
    pub tool_name: String,
    pub kind: ToolProgressDisplayKind,
    pub content: String,
    pub is_live: bool,
}
```

### 7.3 聚合规则

TUI reducer 对 progress 做有限聚合：

1. 相同 `call_id`
2. 相同 `kind`
3. 相邻到达

满足以上条件时，追加到同一个 live `ToolProgress` block。  
否则创建新的 `ToolProgress` block。

这样可以避免：

- 每一行 stdout 都生成独立卡片
- transcript 被极细颗粒度事件打碎

### 7.4 默认渲染

建议渲染形态：

```text
⏳ exec_command
  running cargo test -p talos-ui-tui

stdout
  running 48 tests
  test stream::tests::...

stderr
  warning: unused import ...
```

其中：

- `Status` 默认高优先级显示
- `Stdout` / `Stderr` 默认折叠为最多 N 行预览
- 展开态显示完整聚合内容

---

## 8. Thinking 可见性策略

### 8.1 当前阶段策略

Talos 当前阶段采取：

1. 不隐藏历史 Thinking
2. 不丢弃中间 Thinking
3. 不将 Thinking 自动改写为 Assistant

这样做的原因是：

- 方便排查模型输出节奏
- 方便判断 prompt 是否真的在发挥作用
- 方便对比 `claude-code` 与 Talos 的实际差异

### 8.2 展示要求

尽管不隐藏 Thinking，也要避免视觉噪音：

1. 空 reasoning 不创建 block
2. 仅有 spinner、没有文本时，不生成永久 transcript 行
3. 连续 `Thinking` delta 追加到同一 live block
4. 当下一个 block 是 `ToolUse` 或 `AssistantText` 时，前一个 live `Thinking` block 终结

### 8.3 后续演进

后续如果进入产品化阶段，可再评估：

- 默认隐藏旧 thinking
- Transcript 模式显示全部 thinking
- Debug 模式显示 provider/framework thinking 来源和阶段

但这不属于本轮范围。

---

## 9. TUI Reducer 设计

### 9.1 核心思路

TUI 需要一个显式 transcript reducer，而不是在 `handle_stream_event()` 里直接散落地改多个 UI 状态。

### 9.2 伪代码

```rust
fn apply_stream_event_to_transcript(
    transcript: &mut TranscriptState,
    event: &TalosEventPayload,
) {
    match event {
        MessageReasoningDelta(payload) => {
            transcript.append_thinking(payload.delta.clone());
        }
        MessageOutputDelta(payload) => {
            transcript.append_assistant_text(payload.delta.clone());
        }
        MessageToolCallStarted(payload) => {
            transcript.upsert_tool_use(payload.call_id.clone(), payload.name.clone(), payload.arguments.clone());
        }
        MessageToolCallArgsDelta(payload) => {
            transcript.append_tool_input_delta(
                payload.stream_key.clone(),
                payload.call_id.clone(),
                payload.delta.clone(),
            );
        }
        MessageToolCallQueued(payload) => {
            transcript.mark_tool_status(&payload.call_id, ToolLifecycleStatus::Queued);
        }
        MessageToolCallExecuting(payload) => {
            transcript.mark_tool_status(&payload.call_id, ToolLifecycleStatus::Executing);
        }
        MessageToolCallProgressDelta(payload) => {
            transcript.append_tool_progress(
                payload.call_id.clone(),
                payload.tool_name.clone(),
                payload.progress_kind,
                payload.message.clone(),
            );
        }
        MessageToolCallCompleted(payload) => {
            transcript.complete_tool(payload.call_id.clone(), payload.content.clone(), payload.is_error);
        }
        MessageToolCallPendingApproval(payload) => {
            transcript.mark_tool_waiting_approval(payload.call_id.clone(), payload.tool_name.clone(), payload.prompt.clone());
        }
        MessageError(payload) => {
            transcript.push_system_notice(payload.error.clone());
        }
        _ => {}
    }
}
```

其中 `append_tool_input_delta` 的约束是：

- 以 `stream_key` 作为 streaming 期的主键
- 若 `call_id` 已经可用，则把 `stream_key -> call_id` 关联写回 state
- 不要求 `ArgsDelta` 阶段必须已有稳定 `call_id`

### 9.3 Reducer 约束

1. reducer 是唯一的 transcript 组装入口
2. UI 渲染层只消费 `TranscriptState`
3. 右侧诊断面板只读，不再主导消息编排
4. `finish_active_stream` 不直接散落清理 live block，而是调用 `TranscriptState::finalize()`

---

## 10. 模块接口清单

### 10.1 建议状态结构

```rust
pub struct TranscriptState {
    pub blocks: Vec<TranscriptBlock>,
    live_thinking_idx: Option<usize>,
    live_assistant_idx: Option<usize>,
    live_tool_use_idx: HashMap<String, usize>,
    live_progress_idx: HashMap<(String, ToolProgressDisplayKind), usize>,
    stream_key_to_call_id: HashMap<TalosToolStreamKey, String>,
}
```

说明：

- `blocks` 保存最终 transcript 顺序
- `live_thinking_idx` / `live_assistant_idx` 负责连续 delta 追加
- `live_tool_use_idx` 负责 `call_id -> ToolUse block`
- `live_progress_idx` 负责 `(call_id, kind) -> ToolProgress block` 的相邻聚合
- `stream_key_to_call_id` 负责处理 tool input streaming 阶段的 key 升级

建议额外提供：

```rust
impl TranscriptState {
    pub fn finalize(&mut self);
}
```

`finalize()` 负责：

1. 终结所有 live `Thinking` / `AssistantText`
2. 终结所有 live `ToolProgress`
3. 清理空的临时 block
4. 清空 live 索引

| 模块 | 接口 | 说明 |
|------|------|------|
| `ui-tui::transcript` | `TranscriptState` | 保存 transcript block 序列与 live block 索引 |
| `ui-tui::transcript` | `apply_stream_event_to_transcript` | 将 Talos stream event 归并为 transcript block |
| `ui-tui::transcript` | `append_assistant_text` | 追加或创建 AssistantText block |
| `ui-tui::transcript` | `append_thinking` | 追加或创建 Thinking block |
| `ui-tui::transcript` | `upsert_tool_use` | 创建或更新 ToolUse block |
| `ui-tui::transcript` | `append_tool_progress` | 创建或聚合 ToolProgress block |
| `ui-tui::transcript` | `complete_tool` | 工具终态收口 |
| `ui-tui::render` | `render_transcript_blocks` | 按 block 语义渲染左侧主视图 |
| `ui-tui::render` | `render_tool_progress_block` | 渲染独立 progress block |

---

## 11. 与现有实现的关系

### 11.1 服务端

服务端当前事件协议已经基本满足本方案，不要求新增一整套新 event。  
本方案主要要求：

1. 继续忠实输出 `MessageReasoningDelta`
2. 继续忠实输出 `MessageOutputDelta`
3. 保持 `MessageToolCallProgressDelta` 为独立一等事件

### 11.2 TUI

TUI 是本方案的主改造对象，重点包括：

1. 移除请求发起时预创建空 `Thinking` block 的行为
2. 从 `ChatRole` 驱动，逐步迁移到 `TranscriptBlockKind` 驱动
3. 将 `ToolProgressDelta` 从 tool card 附属内容改为独立 progress message
4. 保留完整 Thinking 可见性

### 11.3 History Replay

历史回放路径本轮不强制接入新 reducer。

阶段 8 的默认策略是：

1. 先只改 live streaming path
2. `replay_history_item` 可暂时保留现有 `push_block` 路径
3. 等 transcript reducer 稳定后，再决定是否统一历史回放到同一组装路径

这样做的目的是控制本轮改动面，避免“流式 transcript 重构”和“历史重放统一”同时进行。

---

## 12. ASCII 流程图

```text
+-----------+        +-------------------+        +------------------+
| UserInput | -----> | Talos SSE Events  | -----> | TranscriptReducer |
+-----------+        +-------------------+        +------------------+
                             |                               |
                             |                               v
                             |                    +----------------------+
                             |                    | Transcript Blocks    |
                             |                    | - UserInput          |
                             |                    | - AssistantText      |
                             |                    | - Thinking           |
                             |                    | - ToolUse            |
                             |                    | - ToolProgress       |
                             |                    | - SystemNotice       |
                             |                    +----------------------+
                             |                               |
                             v                               v
                    +-------------------+          +----------------------+
                    | Right Debug Panel |          | Left Main Transcript |
                    +-------------------+          +----------------------+
```

---

## 13. 验收标准

满足以下条件，才认为本方案完成：

1. 发起新一轮请求时，TUI 不再默认插入空 `Thinking` block
2. 如果模型首个可见增量是 `Assistant`，左侧 transcript 首段必须先出现 Assistant
3. 如果模型首个可见增量是 `Thinking`，左侧 transcript 首段必须先出现 Thinking
4. 如果模型直接 tool call，左侧 transcript 必须直接出现 ToolUse，而不是伪造前置文案
5. `ToolProgressDelta` 在左侧 transcript 中以独立 progress block 出现
6. Thinking 历史默认全部可见
7. 一个真实回合能够稳定呈现：

```text
User
  -> Assistant / Thinking
  -> ToolUse
  -> ToolProgress
  -> Assistant / Thinking
  -> ToolUse
  -> ToolProgress
  -> Final Assistant summary
```

---

## 14. 测试计划

### 14.1 单元测试

1. reducer 首段为 `MessageOutputDelta`
2. reducer 首段为 `MessageReasoningDelta`
3. reducer 首段为 `MessageToolCallStarted`
4. `ToolProgressDelta` 相邻聚合
5. `ToolProgressDelta` 在不同 `call_id` 下不串线
6. tool completed 后 live progress 正确终结

### 14.2 集成测试

1. PlanReAct 回合：`Thinking -> Tool -> Thinking -> Tool -> Final`
2. BuiltIn 回合：`Assistant -> Tool -> Assistant -> Final`
3. 直接 tool-first 回合：`Tool -> Assistant -> Final`

### 14.3 手工验证

在 TUI 中至少验证三类问题：

1. 仓库探索问题
2. shell 命令执行问题
3. 需多轮调查后总结的问题

验证点：

- 首段 block 是否符合真实输出顺序
- ToolProgress 是否独立显示
- Thinking 是否完整保留
- 最终 summary 是否自然收束

---

## 15. 建议实施顺序

1. 先抽 `TranscriptState` 和 reducer
2. 再迁移 `Thinking` / `AssistantText` 的 live block 管理
3. 再把 `ToolProgressDelta` 改造成独立 progress block
4. 阶段 8 暂不改 history replay path
5. 最后做渲染样式和手工 TUI 校验

这样可以先稳定语义，再打磨外观，避免继续在现有 `stream.rs` 上叠 patch。
