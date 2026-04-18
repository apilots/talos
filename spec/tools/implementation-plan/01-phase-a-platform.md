# Phase A Platform Foundation Plan

## 1. 目标

Phase A 只做平台层，不做公开工具大规模切换。

本阶段目标是把 Talos 现有 tools 骨架升级成可承载 Claude Code 同等级工具契约的平台：

- 统一 `ToolExecutor` / `ToolDescriptor` / `ToolSpecBuilder`
- 统一 `ToolContext` 与共享状态模型
- 统一 validation / permission / runtime behavior / scheduler / result policy
- 统一 `ToolRegistry` / `ToolRouter` / `RegistryOrchestrator` / `ToolHandler` 接线
- 统一模型结果、UI render、progress 事件路径

## 2. 里程碑

### 2.1 Milestone A0: ADR Freeze

交付物：

- 冻结 `spec/tools/10-phase-a-adr.md`
- 冻结 canonical public name / internal name / alias discipline
- 冻结 Phase A 非目标清单

完成标志：

- 不再允许新增超大动态 trait
- 不再允许把 `read_file_state` 做成 per-turn only
- 不再允许工具直接写共享状态

### 2.2 Milestone A1: Contract And Catalog

交付物：

- `ToolExecutor`
- `ToolDescriptor`
- `ToolSchemaDescriptor`
- `ToolGovernanceDescriptor`
- `ToolPresentationDescriptor`
- `ToolDefaults`
- `ToolRuntimeBehavior`
- `ToolExecutionResult`
- `ToolContextMutation`
- `ToolResultPolicy`
- `RegisteredToolSpec`
- `ToolCatalogEntry` / `ToolLookupKey` / `ToolVisibility`

目标模块：

- `tools/spec`
- `tools/catalog`
- `tools/defaults`

重点约束：

- 静态信息只能进 descriptor / policy structs
- 运行时动态判定只能通过 `runtime_behavior()`
- 公开名只能有一个 canonical public name

### 2.3 Milestone A2: Shared State And Scheduler

交付物：

- `ToolContext`
- `MessageHistoryView`
- `ReadFileStateStore`
- `ToolDecisionState`
- `ToolResultBudgetState`
- `ToolCatalogState`
- `ContentReplacementState`
- `ToolScheduler`
- `ToolBatchResult`
- `PendingContextMutation`
- `commit_mutation()` barrier

目标模块：

- `runtime/tool_context`
- `runtime/state`
- `runtime/scheduler`

重点约束：

- `ReadFileStateStore` 为 session-scoped
- `build_context_mutation()` 返回 `None` 时不入 pending queue
- runtime/default merge 规则在 scheduler 固化

### 2.4 Milestone A3: Main Pipeline Wiring

交付物：

- `ToolRegistry` 改为 spec builder 驱动
- `ToolRouter` 完成 public/internal/alias 归一化
- `RegistryOrchestrator` 接入 `validation -> permission -> runtime behavior -> schedule`
- `ToolHandler` 收缩为 source dispatch + execute bridge

目标模块：

- `tools/registry`
- `tools/router`
- `tools/orchestrator`
- `tools/handlers/*`

重点约束：

- 不允许继续按工具名硬编码 parallelism / mutation / governance 默认值
- approval / sandbox 不再是 orchestrator 唯一前置治理

### 2.5 Milestone A4: Result And Prompt Integration

交付物：

- `ToolResultMapper`
- `ToolRenderMapper`
- `ToolProgressSink`
- `ToolRenderMessage`
- `PromptToolView`
- `PromptBuilder` 新边界

目标模块：

- `tools/result_mapper`
- `tools/render_mapper`
- `agent-kernel` stream adapter
- `talos_prompt::PromptBuilder`

重点约束：

- persisted result preview/reference 一次定型
- TUI/Web/transcript 不再自己发明 tool render 格式
- prompt 只消费 prompt-facing tool view

### 2.6 Milestone A5: Phase Gate

交付物：

- Phase A DoD checklist
- pilot tool onboarding checklist
- alias 清理计划
- Phase B 准入门禁

完成标志：

- 平台层测试矩阵通过
- 首批 pilot 工具可以不写特例就接入平台

## 3. 可并行工作包

### WP-A Contract Layer

职责：

- descriptor/defaults/spec builder
- result protocol
- catalog identity

建议 ownership：

- `tools/spec/*`
- `tools/catalog/*`

### WP-B Shared State Layer

职责：

- `ToolContext`
- read/decision/budget/catalog/content replacement state

建议 ownership：

- `runtime/tool_context/*`
- `runtime/state/*`

### WP-C Governance And Scheduler Layer

职责：

- validation
- permission matcher
- runtime behavior merge
- scheduler/barrier

建议 ownership：

- `runtime/validation/*`
- `runtime/permissions/*`
- `runtime/scheduler/*`

### WP-D Pipeline Wiring Layer

职责：

- registry/catalog/router/orchestrator/handler 接线

建议 ownership：

- `tools/registry.rs`
- `tools/router.rs`
- `tools/orchestrator.rs`
- `tools/handlers/*`

### WP-E Result And Prompt Layer

职责：

- result mapper
- render mapper
- progress sink
- prompt view

建议 ownership：

- `tools/result_*`
- `agent-kernel` event adapter
- `prompt/*`

## 4. 依赖关系

```text
A0 -> A1
A1 -> A2
A1 + A2 -> A3
A1 + A2 + A3 -> A4
A4 -> A5
```

补充说明：

- `A1` 与 `A2` 的设计要并行推进，但代码落地以 `A1` 接口冻结为前提
- `A3` 必须在 `A1/A2` 边界冻结后再做
- `A4` 不能早于主链稳定，否则会被 preview/persist 协议反复打回

## 5. 测试策略

单测：

- descriptor default merge
- alias/public/internal lookup
- `should_defer/always_load` 规则
- `ToolContextMutation` commit 顺序
- `ValidationResult` / `PermissionDecision`
- aggregate budget 统计
- progress event mapping

集成：

- mixed read-only + serial batch scheduling
- validation reject / permission ask / deny 分流
- persisted result preview/reference
- progress event -> `KernelStreamEvent`

e2e：

- TUI transcript tool rendering
- SSE tool progress
- approval / sandbox
- multi-turn tool continuation

## 6. 风险与止损点

- 若 `A1` 未冻结就开始接工具域，会产生二次迁移
- 若 `A2` 不先收敛 barrier，后续所有并发工具都要返工
- 若 `A3` 仍保留 name-based hardcode，平台层价值会被抵消
- 若 `A4` 提前让工具各自定义 preview/render，后续无法统一

止损规则：

- 任一 milestone 出现第三次接口返工，必须先回写 ADR 或本计划，再继续编码
- `A5` 未通过，不允许进入 Phase B 公开工具切换
