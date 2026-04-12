# 流式交替执行 Part 2：实时反馈、交互体验与回合节奏

## 1. 本部分目标

Part 1 解决“什么时候执行工具”。  
Part 2 解决“用户最终感受到的是否像 Claude Code”。

如果没有 Part 2，本轮实现最多只能做到：

- 无死窗
- 工具执行提早
- UI 有开始/结束状态

但还做不到：

- 模型先说一句接下来要做什么
- 工具执行过程中持续反馈
- 工具结果回来后继续自然探索
- 最后自然收束总结

因此 Part 2 把这些体验写成明确契约。

---

## 2. 目标体验

目标体验是下面这种回合节奏：

```text
assistant short text
  -> "我先检查 X"

tool starts
  -> queued / executing
  -> progress keeps updating

tool result arrives
  -> assistant short text
     "我看到 Y，还需要继续查 Z"

next tool starts
  -> progress keeps updating

enough information collected
  -> assistant final summary
```

重点不是必须完全复制 Claude Code 文案，而是满足以下 4 个用户感知要求：

1. 在首次工具调用前，尽量先出现一小段用户可见说明
2. 工具执行过程中有持续反馈，而不是静态 spinner
3. 工具结果回来后，模型还能继续输出下一段思考/说明，而不是生硬跳转
4. 当探索足够时，有明确收束总结

---

## 3. 模型输出节奏契约

### 3.1 回合内文本节奏

本次迭代要求模型遵守以下回合内节奏：

1. 首次工具调用前，优先输出 1 句简短说明
2. 每次发现关键信息或改变方向时，补 1 句简短说明
3. 工具结果足够后，输出最终总结

建议风格：

- 短句
- 高层说明
- 不泄漏冗长推理
- 不写成日志

例如：

- “我先检查一下配置加载链路。”
- “已经确认入口没问题，我继续看工具调度部分。”
- “问题根因在于工具执行仍然在流结束后批量触发。”

### 3.2 这不是纯架构问题

这一点不能靠 scheduler 自动达成，必须同步补 prompt 约束。  
否则模型完全可能直接出 tool call，不给任何用户可见短文本。

因此本部分明确要求：

- 更新 `PromptBuilder` 对应的 agent 行为提示
- 把“首次工具调用前简短说明”和“关键节点进度说明”写成硬约束

---

## 4. 工具执行过程实时反馈协议

仅有 `ToolQueued` 和 `ToolExecuting` 不够。  
若要接近 Claude Code 体验，必须新增 progress 流。

### 4.1 新增事件

```rust
pub enum KernelStreamEventType {
    OutputDelta,
    ThinkingDelta,
    ExecutionSummary,
    ToolCallStart,
    ToolArgsDelta,
    ToolQueued,
    ToolExecuting,
    ToolProgressDelta,
    ToolResult,
    PromptWarning,
    ConfirmationRequest,
}
```

### 4.2 事件结构

```rust
pub struct ToolProgressDeltaStreamData {
    pub stream_key: ToolStreamKey,
    pub call_id: String,
    pub tool_name: String,
    pub message: String,
    pub progress_kind: ToolProgressKind,
}

pub enum ToolProgressKind {
    Stdout,
    Stderr,
    Status,
}
```

### 4.3 设计原则

1. `ToolProgressDelta` 是可选能力，不要求所有工具实现
2. 无 progress 的工具至少仍有 `ToolQueued -> ToolExecuting -> ToolResult`
3. 有 progress 的工具必须尽量发结构化短消息，而不是原样倾倒大量文本

---

## 5. 工具类型的反馈要求

### 5.1 Shell / Bash 类工具

目标：

- 支持输出阶段性状态
- 必要时支持 stdout/stderr 摘要流

建议行为：

- 命令启动时：`Status`
- 有关键输出时：`Stdout`
- 出错时：`Stderr`

### 5.2 MCP / 外部服务类工具

目标：

- 至少支持状态流，而不是只有最终结果

建议行为：

- “connecting”
- “request sent”
- “waiting response”
- “received response”

### 5.3 Dynamic / Approval 类工具

目标：

- 明确告诉用户当前不是“卡住”，而是在等待外部确认或结果

建议事件：

- `ToolQueued`
- `ToolProgressDelta(Status: waiting_approval / waiting_dynamic_result)`
- `ConfirmationRequest` 或对应 pending result

---

## 6. TUI 状态机

TUI 卡片状态从简单两态升级为：

```text
streaming_input
  -> finalized
  -> queued
  -> executing
  -> progress_updating
  -> completed | error | cancelled | discarded
```

### 6.1 渲染要求

1. `ToolArgsDelta` 期间显示参数构建中
2. `ToolQueued` 显示已排队
3. `ToolExecuting` 显示执行中 spinner
4. `ToolProgressDelta` 持续刷新卡片正文或附加日志区
5. `ToolResult` 切到稳定终态

### 6.2 重要约束

TUI 不能把 `ToolExecuting` 当作“有实时反馈”的替代品。  
如果没有 `ToolProgressDelta`，体验仍然只是“静态等待”。

---

## 7. 回合内探索与继续执行

本部分要求实现后的系统支持如下行为：

1. assistant 输出短文本
2. 触发工具 A
3. 工具 A 执行中实时反馈
4. 工具 A 完成
5. assistant 继续输出短文本，解释下一步
6. 触发工具 B
7. 重复直到信息足够
8. assistant 输出最终总结

这要求主循环满足：

- `tool_result` 一进入上下文，下一轮 assistant 立刻可用
- 不是一次工具执行完就强制终局
- 不是仅靠 TUI 做表面演出，模型确实得到结构化结果

---

## 8. 最终总结契约

当满足以下任一条件时，assistant 应优先收束：

1. 已找到明确答案
2. 已得到足够多的证据支持结论
3. 继续探索收益明显下降

最终总结要求：

- 明确结论
- 简要列出依据
- 如有剩余不确定性，直接指出

不允许：

- 工具跑完后无总结直接结束
- 只给流水账，不给结论

---

## 9. Prompt 修改范围

本部分要求补充 prompt 约束，至少包含：

1. 首次工具调用前，先给一句简短说明
2. 工具执行过程中，在关键节点给短更新
3. 信息足够后主动总结，不继续无效探索
4. 文本简洁，不输出冗长内部推理

建议把这些要求统一纳入 `PromptBuilder` 生成的执行期 prompt，而不是散落在调用点。

---

## 10. 验收场景

以下体验场景必须通过：

### 场景 A：单工具

用户提问  
-> assistant 先说一句要做什么  
-> 工具执行中有进度  
-> 工具完成  
-> assistant 给结论

### 场景 B：两次连续探索

用户提问  
-> assistant 先说一句  
-> 工具 A 反馈  
-> assistant 说明还需继续查  
-> 工具 B 反馈  
-> assistant 总结

### 场景 C：pending approval

assistant 说明要执行  
-> tool queued  
-> waiting approval progress  
-> UI 明确展示等待确认，而不是“卡死”

### 场景 D：中断

工具执行中  
-> 用户中断  
-> 卡片进入 cancelled  
-> assistant 不产生悬空状态

---

## 11. 本部分完成标准

Part 2 完成后，必须满足：

1. 系统不只是“更早执行工具”，而是有明确回合节奏
2. 工具过程存在实时反馈协议
3. prompt 明确约束用户可见短文本行为
4. UI 能区分 queued / executing / progress / completed
5. 至少能支撑“先说一句 -> 执行 -> 继续说 -> 再执行 -> 总结”的完整体验
