# 流式交替执行 Spec Index

本主题拆分为 3 份文档，目的是把“边输出边执行边反馈，最后自然收束总结”的目标写清楚，并降低单份 spec 的复杂度。

## 目标

本次迭代的目标不是仅仅“把工具提早执行”，而是尽量一次性实现接近 `claude-code` 的核心体验：

1. 模型可以先输出一小段用户可见文本，再进入工具执行
2. 工具在 assistant streaming 期间即可进入调度与执行
3. 工具执行过程存在实时反馈，而不是只有开始和结束
4. 工具结果回来后，模型能继续输出下一段思考/说明，并继续探索
5. 当信息足够时，最终给出自然总结，而不是僵硬结束

## 文档拆分

- [streaming-interleaved-execution-part1.md](/Users/xujiang/data/ailabs/talos/spec/streaming-interleaved-execution-part1.md:1)
  说明核心架构改造：`stream_round`、`StreamingToolScheduler`、`ToolBatchCollector`、主循环对接。

- [streaming-interleaved-execution-part2.md](/Users/xujiang/data/ailabs/talos/spec/streaming-interleaved-execution-part2.md:1)
  说明用户体验契约：模型短文本节奏、工具实时反馈协议、TUI 渲染状态机、回合内多次探索与收束。

- [streaming-interleaved-execution-part3.md](/Users/xujiang/data/ailabs/talos/spec/streaming-interleaved-execution-part3.md:1)
  说明一次迭代的实施边界、阶段安排、验收标准、风险与非目标。

## 阅读顺序

建议按以下顺序阅读：

1. Part 1：先确认系统边界和执行路径
2. Part 2：再确认是否真的覆盖目标体验
3. Part 3：最后确认这次迭代是否能一次性交付

## 决策说明

当前总方案明确拒绝以下路径：

- 只改 `stream_round()` 里的工具执行时机，不定义交互体验协议
- 只有 `ToolExecuting` / `ToolResult`，没有 progress 流
- 指望 prompt 自发产生 Claude Code 风格节奏，而不写成强约束
- 把“这次先做一半，剩下下次再补”作为默认落地方式

本次拆分文档后，只有当 Part 1、Part 2、Part 3 同时满足时，才视为达到本轮目标。
