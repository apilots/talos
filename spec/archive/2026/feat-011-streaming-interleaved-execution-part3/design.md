# 流式交替执行 Part 3：一次迭代实施计划与验收

## 1. 本部分目标

本部分回答一个最关键的问题：

这次迭代是否可以一次性做到目标，而不是只交付一半体验。

结论是：  
可以，但前提是本轮实现必须同时覆盖三块内容：

1. Part 1 的执行架构
2. Part 2 的实时反馈协议
3. Prompt 与 TUI 的配套修改

只做其中一块，都不能视为达到本轮目标。

---

## 2. 本轮交付范围

### 2.1 必做

- `StreamingToolScheduler`
- `ToolBatchCollector` 抽取与复用
- `ToolArgsDelta / ToolQueued / ToolExecuting / ToolProgressDelta / ToolResult`
- `stream_round()` 流中调度与 drain
- TUI 工具卡片状态机升级
- PromptBuilder 中的执行期短文本约束
- 中断 / timeout / panic / discard 稳定终态
- 至少 1 类工具的 progress streaming 首次落地

### 2.2 建议首批支持 progress 的工具

优先级建议：

1. shell / bash 类工具
2. MCP 工具
3. dynamic / approval 等待状态

理由：

- 这三类最能直接影响用户对“系统是否卡住”的感知

### 2.3 非目标

- 多 Agent 流式协同
- provider fallback / tombstone 完整机制
- 新 capability schema
- 历史回放层大改

---

## 3. 分阶段实施

## 阶段 1: 核心事件与调度器骨架
**目标**: 落地 `ToolStreamKey`、新增事件、`StreamingToolScheduler` 基础状态机。
**成功标准**: tool call finalized 后可以进入 scheduler，具备 queued/executing/completed 基本闭环。
**测试**: 单测覆盖状态转换、顺序保持、mutating tool 排队。
**状态**: 未开始

## 阶段 2: `stream_round()` 与主循环接入
**目标**: 在 streaming 过程中提交工具、drain 已完成结果，并将结果通过 collector 返回主循环。
**成功标准**: 工具在 assistant stream 期间启动，结果可在流中或流末进入上下文。
**测试**: `general_turn` 单测和集成测试通过。
**状态**: 未开始

## 阶段 3: Progress 协议落地
**目标**: 增加 `ToolProgressDelta`，至少让首批工具支持实时过程反馈。
**成功标准**: UI 不再只有静态 spinner，执行过程中会看到持续更新。
**测试**: shell / MCP / pending approval 场景测试。
**状态**: 未开始

## 阶段 4: Prompt 与 TUI 收口
**目标**: 让模型输出节奏、工具卡片状态机、最终总结行为形成完整体验。
**成功标准**: 能稳定表现为“先说一句 -> 执行 -> 继续说 -> 再执行 -> 总结”。
**测试**: 端到端回归 + 手动体验验收。
**状态**: 未开始

---

## 4. 验收标准

本轮只有在以下验收全部满足时，才算完成目标。

### 4.1 架构验收

1. `stream_round()` 不再承担工具业务收敛职责
2. 调度器与收敛器边界清晰
3. 主循环仍保持统一 `general_turn` 路径

### 4.2 体验验收

1. 无明显空白死窗
2. 首次工具调用前大概率有一小段用户可见短文本
3. 工具执行中出现实时反馈
4. 工具结果回来后 assistant 能继续输出下一步说明
5. 最终有明确总结

### 4.3 稳定性验收

1. 中断后没有悬空 tool call
2. timeout / panic 有稳定错误终态
3. approval / dynamic waiting 状态对用户可见
4. `ChatMessage::tool(...)` 顺序与原始 index 一致

### 4.4 测试验收

至少执行：

- 相关 crate 单测
- 相关 TUI 测试
- `talos-e2e/run_e2e.sh`

---

## 5. 风险

### 风险 1：只完成“早执行”，没完成“好体验”

表现：

- 工具提早执行了
- 但没有 progress
- 没有 prompt 节奏约束
- 用户还是觉得像半成品

规避：

- 本轮验收把 Part 2 强制纳入必做范围

### 风险 2：过度耦合进 `step_execute.rs`

表现：

- 逻辑又重新堆回单文件
- 后续很难维护

规避：

- 强制新增 `tool_streaming.rs`
- 强制保留 collector 复用

### 风险 3：并发写工具破坏工作区

规避：

- 首版严格依赖 `tool_supports_parallel + !tool_is_mutating`
- 不做激进并发

### 风险 4：prompt 行为不稳定

规避：

- 写成明确执行期 prompt 规则
- 在 e2e 里覆盖至少一个“先短文本再执行工具”的场景

---

## 6. 推荐实施顺序

如果希望这次迭代一次完成目标，推荐按以下编码顺序推进：

1. 新增事件和 scheduler 骨架
2. 改 `stream_round()` 与主循环
3. 抽 collector，保证结果规约复用
4. 落地 progress 协议
5. 改 PromptBuilder
6. 改 TUI 状态机
7. 跑单测和 e2e
8. 手动体验验收

不要反过来先改 UI，再补调度器。那样很容易做成“看起来很流式，实际仍然批执行”。

---

## 7. Definition of Done

本轮完成定义：

- 代码层面：工具在流中调度与执行
- 事件层面：存在 progress 流而非仅开始/结束
- Prompt 层面：模型具备明确短文本节奏约束
- UI 层面：工具卡片可见 queued/executing/progress/completed
- 回合层面：支持至少一次“探索 -> 继续探索 -> 总结”的完整过程
- 测试层面：单测、TUI 测试、e2e 全部通过

如果缺任何一项，都不算达到“像 Claude Code 一样边说边做边反馈”的本轮目标。
