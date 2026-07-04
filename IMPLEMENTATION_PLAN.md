# Model-driven Agent Loop Implementation Plan

本计划对应 `spec/refactor-034-model-driven-agent-loop/design.md`。本轮目标是增量优化长任务执行能力：删除主 Agent loop 的固定工具轮次停止语义，让 run 由模型和明确资源边界驱动；同时把 context compaction、MemoryStore session summary、PromptBuilder prompt view 和 Kernel working context 预算治理补齐为可观测、可恢复、可测试的闭环。

## 执行原则

- 每个阶段完成后必须自查 spec 对齐、dead code、兼容分支、重复 DTO、反向测试和跨层职责泄漏。
- 第一阶段不引入新的 `maxTurns`、`agent_step_limit` 或等价主 loop 硬限制。
- `MemoryStore` 负责持久化 session 级 compact summary；Kernel active run working context 不写入 `SummaryRecord`，只在 run 内治理并通过 timeline 暴露。
- `run_id == turn_id` 是内部身份不变量，文档、代码注释、测试命名都不得引入第二套 run identity。
- 完整 E2E 只在所有局部编译、单测、集成测试和前端类型检查通过后运行一次；失败后只重跑失败 group 或 case。
- Web / TUI 自测必须发生在完整 E2E 后，验证 context compaction 等待态、普通 run、timeline recovery 的核心路径。

## 阶段 1: 现状扫描与测试锚点

**目标**: 梳理 `max_tool_rounds`、`ToolRoundLimit`、session compaction、MemoryStore prompt context、PromptBuilder trim、Kernel loop state 和 timeline event 的真实调用链，形成可执行改造清单。

**成功标准**:
- 列出所有 `max_tool_rounds` / `ToolRoundLimit` 出现位置，区分生产代码、配置、schema、generated TS、Web/TUI、测试 fixture。
- 确认自动 session history compaction 当前入口、summary 持久化入口、PromptBuilder recent history 加载入口。
- 确认 active run working context 当前由 `prepared.messages + state.messages` 组成，并标记预算检查插入点。
- 更新本计划阶段状态和必要的 spec 备注，不引入代码行为变化。

**测试**:
- `rg "max_tool_rounds|ToolRoundLimit|tool_round" .`
- `rg "compact_session_history_if_needed|SummaryRecord|covers_until_seq|TrimToBudget|state.messages" talos-rs`
- 不需要运行测试；本阶段只做只读扫描和计划锚定。

**状态**: 已完成

## 阶段 2: 删除固定工具轮次停止语义

**目标**: 删除主 Agent loop 的 `max_tool_rounds` / `ToolRoundLimit` 产品语义，保留工具轮次和模型 step 作为 metrics / diagnostics。

**成功标准**:
- 删除 `agent.kernel.max_tool_rounds` 配置、默认值、schema、生成协议、SDK、UI/TUI 展示和测试 fixture。
- 删除 Kernel 与公共 API 中的 `StopReason::ToolRoundLimit`，清理 forced finalization 分支和相关负向测试。
- 不新增兼容 alias、fallback、hidden env override 或等价硬停止参数。
- 仍保留现有模型 final response、`ModelOutputLimit`、`WaitingDynamicInput`、`Cancelled` 等生命周期语义。

**测试**:
- `just fmt`
- 针对改动 crate 运行 `just test -p <package>`
- 如果涉及 schema / TS，运行项目既有协议生成命令并检查 generated diff。

**状态**: 已完成

## 阶段 3: ContextCompaction Timeline Wire Contract

**目标**: 把自动 context compaction 建模为 timeline item / delta，使阻塞 compaction 能被 SSE、replay、Web、TUI 看到。

**成功标准**:
- 新增 `ContextCompaction` timeline content、delta、trigger、stage、outcome、metrics，包含 `WorkingContextExceeded` trigger。
- Rust serde、schema、generated TS、Web、TUI enum tag 和字段命名保持一致。
- 自动 session compaction 在 `Preparing -> Summarizing -> RebuildingPrompt -> Trimming -> Completed/Failed` 生命周期中发出可恢复事件。
- `timeline.recover`、`subscribe_timeline`、live SSE 能看到同一条 compaction item 的 started / delta / completed。
- raw helper prompt、raw helper completion、runtime-private payload 不通过 Gateway / SDK 暴露。

**测试**:
- timeline content / delta serde 单测。
- timeline recovery / subscription 集成测试，覆盖 compaction item replay。
- Web / TUI 类型检查和最小渲染测试。

**状态**: 已完成

## 阶段 4: MemoryStore Session Summary Coverage

**目标**: 将 session history compaction 改为基于 summary coverage 游标推进，并让 MemoryStore prompt context 负责 cursor 过滤、Prompt view 回补和 summary source 连续窗口。

**成功标准**:
- 复用现有 `SummaryRecord.covers_until_seq` 作为第一阶段 compact cursor；如实现需要，补充 `covers_from_seq` 并同步 schema / store migration / tests。
- compact source 使用 `OldestUncovered` 窗口，只选择 `seq > latest_compacted_seq && seq <= latest_completed_seq` 的连续 completed turns。
- compact range 为空时不生成新 summary；summary 之间不重复覆盖同一区间。
- MemoryStore / session prompt context 支持 `CompactPromptCursor`、`SessionPromptRecentWindow` 和 diagnostics。
- raw recent 默认过滤已 compact 的 turns；不足 `recent_messages_limit` 时按 turn 边界回补最近少量 completed turns。
- hot cache 不能在缺少 turn seq 证明时绕过 cursor 过滤。

**测试**:
- MemoryStore 单测覆盖无重复 coverage、cursor 过滤、fresh tail 回补、空 compact range。
- PromptBuilder 测试覆盖 compact summary region + raw recent tail 的组装顺序。
- 如涉及 DB / file schema，增加 migration 或 fixture 回归测试。

**状态**: 已完成

## 阶段 5: Kernel Working Context Budget Guard

**目标**: 在每次主路径 provider request 前治理 active run working context，避免删除固定工具轮次后 `state.messages` 无界增长。

**成功标准**:
- Kernel 在每次主路径 model request 前构建候选 messages，并检查 `prepared.messages + state.messages` 是否超过 effective prompt budget。
- 超预算时发出 `ContextCompaction(trigger=WorkingContextExceeded)` timeline lifecycle。
- 第一阶段至少实现 deterministic working-context trim + fail-fast；helper summary 可后续演进，但不能没有 preflight budget check。
- trim 保留 static prefix、compact summary region、raw recent tail、current user input、pending dynamic input、未闭合 tool call/result linkage、最近 N 个完整 working tool exchanges。
- trim 后仍超预算时，在 provider request 前失败，并映射为结构化 context window error。

**测试**:
- Agent Kernel 集成测试覆盖长工具结果导致 working context 超预算时不会继续发 provider request。
- 测试保留最近完整 tool exchange 和 unresolved tool linkage。
- 测试 trim 失败时产生 timeline error / active turn failed，而不是模糊 panic 或 provider error。

**状态**: 已完成

## 阶段 6: Usage Aggregation 与错误边界

**目标**: 将 session compaction usage、working-context helper usage 和主模型 usage 聚合到 run-level usage，并补齐最小结构化错误语义。

**成功标准**:
- `PreparedGeneralTurn` 或等价准备结果携带 session compaction usage。
- `GeneralTurnLoopState` 或等价 loop state 携带 working-context compaction / trim usage。
- `ExecutedTurnArtifacts.usage`、`TurnExecutionMetrics.usage`、`GeneralTurnResult.usage` 看到同一套 run-level 聚合结果。
- 最小结构化错误覆盖 `ContextWindowExceeded`、`ContextCompactionFailed`、`InternalError`；不把这些错误临时塞进 public `StopReason`。
- AgentHarness / Service / Gateway 的错误映射只做本层职责，不把 DB record 或 runtime-private payload 外泄。

**测试**:
- usage 聚合单测或集成测试，覆盖 compaction helper + main model usage 相加。
- turn error / timeline error 测试，覆盖 context unrecoverable 和 compaction failed。
- SDK / generated TS 类型检查。

**状态**: 已完成

## 阶段 7: 多端协议与用户可见状态收敛

**目标**: 同步协议、SDK、Web、TUI，使长任务和 compaction 等待态在多端表现一致。

**成功标准**:
- 生成协议、TS SDK 和前端类型包含新的 context compaction item / delta / metrics。
- Web run timeline 能渲染 compaction waiting / completed / failed 状态，不显示为静默卡住。
- TUI 能显示 compaction waiting 状态，并能通过 timeline recovery 恢复。
- 删除所有旧 `ToolRoundLimit` UI 文案、schema fixture 和测试断言。
- README / spec / API 文档同步说明 `max_tool_rounds` 已移除，以及 compaction 是 run 内部维护阶段。

**测试**:
- 协议生成命令。
- Web typecheck / unit tests。
- TUI snapshot 或集成测试。
- `rg "ToolRoundLimit|max_tool_rounds|tool_round_limit" .` 确认只剩必要历史说明或没有残留。

**状态**: 已完成

## 阶段 8: 完整验证、E2E 与手工自测

**目标**: 在局部验证全部通过后，执行一次完整 E2E，并对 Web / TUI 核心用户路径做真实手工验证。

**成功标准**:
- `just fmt`、必要 `just fix -p <package>`、局部 `just test -p <package>` 均通过。
- 所有新增/修改的协议 fixture、schema、generated TS 已由生成命令产生，未手写 generated 文件。
- 完整 E2E 只运行一次：`talos-e2e/run_e2e.sh`。
- 完整 E2E 如果失败，只针对失败 group 或 case 单独修复和重跑，不重复跑完整 E2E。
- E2E 通过后启动 Web / TUI，自测普通 run、长工具链 run、compaction waiting、timeline recovery、interrupt/cancel 基础路径，并保留日志或截图结论。
- 提交前完成 dead code、反向测试、兼容逻辑、重复 DTO、旧配置残留的全局审查。

**测试**:
- `just fmt`
- `just test -p <package>` 按实际改动范围执行。
- `talos-e2e/run_e2e.sh` 完整一次。
- 失败后使用 e2e group / case scoped rerun。
- Web / TUI 手工 smoke test。

**本地验证记录**:
- 已执行 `just fmt`。
- 已执行并通过 `just test -p talos-memory`、`just test -p talos-prompt`、`just test -p talos-agent-kernel`、`just test -p talos-agent`、`just test -p talos-service`、`just test -p talos-app-protocol`。
- 已执行 `just clippy -p talos-memory`、`just clippy -p talos-prompt`、`just clippy -p talos-agent-kernel`，命令退出码为 0；输出仍包含项目既有 warning，本轮触达的 `memory_summary.rs` manual div ceil 已修复。
- 已执行一次完整 `talos-e2e/run_e2e.sh` 并通过，报告位于 `talos-e2e/reports/e2e_report_20260704_120928.md`。
- 已完成 Web HTTP smoke 和 TUI CLI smoke；本轮 review 后未改动 Web / TUI 运行时代码。
- 独立 subagent review 指出的 summary coverage、compaction terminal event、coverage metrics、structured error 分类、重复 compact policy 字段已收敛。

**状态**: 已完成
