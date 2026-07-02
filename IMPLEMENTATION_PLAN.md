# Agent Framework API Standardization Plan

本计划对应 `spec/refactor-033-agent-framework-api-standardization/design.md`。032 已完成并提交，本计划只描述下一轮 033 的递进落地，不回写已经完成的执行记录。

## 执行原则

- 每个阶段必须先理解现有代码，再做最小可验证改动。
- Spec 写设计，Plan 写阶段、状态、成功标准和验证命令；实现变化后同步更新 spec。
- Service / Gateway / SDK 对外 DTO 使用 `*Request` / `*Response`；Coordinator / AgentHarness / AgentRuntime 内部 API 使用 `*Params` / `*Result`；Store 使用 `*Record` / `*Store`。
- Agent 域核心结构只保留一套；禁止新增 `RuntimeRunParams`、`HarnessRunParams`、`KernelRunResult`、`AgentUsage`、`RunUsage`、`AgentItem` 等重复 DTO。
- AgentHarness 是公共实现层，AgentRuntime 是 SPI；新增 backend 只能实现 Runtime SPI，不能复制 Harness lifecycle / event / input / error 逻辑。
- Agent 领域不接收 DB Record，不暴露 Gateway DTO，不把 runtime private thread/session/event 传到上层。
- 所有新 public trait / struct / enum / function 和承担框架边界职责的 crate-private SPI 必须有英文 doc comment。
- 每阶段完成后必须自我审查：旧名残留、重复 DTO、fallback、跨层依赖、store record 泄漏、测试专用生产分支。

## 本轮 7 个硬性落地点

1. `AgentRunOptions` 本轮只落地 `current_model?`；`continuation_policy` 不进入 options，dynamic input continuation 由 `AgentResumeParams` / `AgentInput` 表达。
2. `AgentError` / `AgentErrorKind` 是 Agent domain 内部错误结构，不导出到 generated TS schema，也不作为 HTTP / JSON-RPC response body。
3. `resume_streamed` 只表示同一 `turn_id` 下的 continuation stream，不是第二次 `run_streamed`，不是客户端 attach，也不是 Service timeline recovery。
4. `AgentRunParams` 不再携带完整 `TalosSession`；run 边界使用 `AgentRunContext`、`AgentRunConfig`、`AgentRunSessionState`、`AgentRunOptions` 和 input，完整 session 只能留在 Service / Coordinator adapter、session lifecycle API 或测试 fixture。
5. 内部 run identity 写死为 `run_id == turn_id`；如果历史文档、日志或代码出现 `trun_id` 拼写，只能映射回同一个 `turn_id` 并纠正，不能引入第三套 id、alias 或独立存储字段。
6. Unsupported option / capability fail-fast 统一放到阶段 4 的 capability validation framework；阶段 3 只负责结构成型。
7. 阶段 9 必须先跑通所有相关非 E2E case，再完整 E2E 只跑一次；失败后只跑失败 group / case，最后自动执行 Web / TUI 核心 smoke。

## 7 个硬性落地点的阶段归属

- 点 1: 阶段 3 建立 `AgentRunOptions { current_model? }`，阶段 8 用 `AgentResumeParams` / `AgentInput` 承载 continuation。
- 点 2: 阶段 5 落地 `AgentError` / `AgentErrorKind`，阶段 9 复查 generated TS / SDK / Gateway response 不暴露内部错误对象。
- 点 3: 阶段 8 落地 `resume` / `resume_streamed` 注释、API 和测试，复查它不替代 attach 或 Service timeline recovery。
- 点 4: 阶段 3 最小引入 `AgentRunContext`，阶段 9 完整移除 `AgentRunParams.session` 过渡依赖，并解释剩余 `TalosSession` 命中是否属于 session lifecycle / adapter / test fixture。
- 点 5: 阶段 3 固化 `run_id == turn_id`；全阶段复查 `trun_id` 只能作为历史拼写映射回同一个 `turn_id` 并纠正，不能新增字段、alias、兼容入口或独立持久化。
- 点 6: 阶段 4 统一 capability validation；阶段 3 不分散实现 unsupported option fail-fast。
- 点 7: 阶段 9 固化最终验证顺序：非 E2E 全通过、完整 E2E 只跑一次、失败后只跑失败 group / case、最后 Web / TUI smoke。

## 7 个硬性落地点的全局验收命令

这些命令不是每阶段都全量运行；对应阶段完成时按阶段风险选择运行，阶段 9 必须完整复查。

- 点 1: `rg "continuation_policy" talos-rs talos-app` 只能命中文档或明确 TODO，不能进入本轮 `AgentRunOptions` 代码路径。
- 点 2: `rg "AgentError|AgentErrorKind" talos-rs/crates/gateway talos-rs/crates/common talos-app -g '*.rs' -g '*.ts' -g '*.json'` 必须无命中。
- 点 3: `rg "AgentAttachParams|attach\\(" talos-rs/crates/orchestration/agent/src talos-rs/crates/orchestration/coordinator/src` 不能发现本轮新增 attach API；`resume_streamed` 调用点必须复用原始 `turn_id`。
- 点 4: `rg "TalosSession" talos-rs/crates/orchestration/agent/src talos-rs/crates/orchestration/coordinator/src` 必须能解释每个保留点是否属于 session lifecycle、Coordinator adapter、test fixture 或 input bridge；`AgentRunParams` / `AgentKernelParams` 不能携带完整 session。
- 点 5: `rg -n "\\btrun_id\\b|\\btrunId\\b" talos-rs talos-app` 必须无代码或 generated artifact 命中；`rg -n "\\brun_id\\b|\\brunId\\b" talos-rs/crates/orchestration/agent/src talos-rs/crates/orchestration/coordinator/src` 不允许发现独立 public run identity 字段。任何诊断语义中的 `run_id` 都必须能映射为同一个 `turn_id`。
- 点 6: `rg "AgentCapabilityValidator|CapabilityUnsupported" talos-rs/crates/orchestration/agent/src talos-rs/crates/orchestration/coordinator/src` 必须说明 unsupported option / input / interrupt / resume 的唯一 fail-fast 入口。
- 点 7: 阶段 9 按“非 E2E 全通过 -> `just e2e` 一次 -> 失败 group/case 单跑 -> Web / TUI smoke”的顺序记录验证结果。

## 命令工作目录

- `git`、`rg`、`just` 和 `pnpm --dir talos-app ...` 默认从仓库根目录 `/Users/xujiang/data/ailabs/talos` 执行。
- 根目录 `justfile` 已设置 `set working-directory := "talos-rs"`；因此 `just fmt`、`just check`、`just test`、`just e2e` 均从仓库根目录调用。
- 直接 `cargo run -p talos-server ...`、`cargo run -p talos-tui ...` 的 smoke 命令在 `talos-rs` 目录执行。

## 并发 Subagent 策略

**目标**: 并发做只读审查、机械迁移和验证，但 API 命名、分层职责和最终合并由主 agent 裁决。

**通用规则**:

- 每个 subagent 使用空白上下文，只提供仓库路径、当前分支、阶段目标、允许读写范围、禁止事项和输出格式。
- 审查型 subagent 只读；执行型 subagent 必须声明 exact file allowlist。
- 同一文件同一时间只允许一个执行型 subagent 修改。
- `talos-rs/crates/common/types/src/*`、`talos-rs/crates/orchestration/agent/src/service.rs`、`talos-rs/crates/orchestration/agent/src/agent.rs`、`talos-rs/crates/orchestration/service/src/turn/*`、schema/generated TS 属于高冲突文件，必须串行写入。
- 主 agent 合并任何 subagent patch 前必须重新阅读 diff，并运行该阶段 scoped formatter / check / test。
- 完整 E2E 在阶段 9 只运行一次；失败后只运行失败 group / case，不重复跑完整 E2E。

**角色池**:

- `api-boundary-reviewer`: 只读审查 Service / Coordinator / Harness / Runtime / Gateway / SDK API 边界。
- `type-model-worker`: 执行核心结构、命名、doc comment 和旧名残留迁移。
- `lifecycle-worker`: 执行 run lifecycle、input bridge、interrupt、resume/backlog、AgentRunContext 过渡迁移相关改造。
- `router-definition-worker`: 执行 AgentDefinition、AgentRouter、AgentRegistry、selection policy 相关改造。
- `schema-sdk-ui-worker`: 同步 app protocol schema、generated TS、SDK、Web、TUI 和 README。
- `verification-runner`: 运行 formatter、scoped check/test、E2E case、Web/TUI smoke，只记录结果不改架构。

**输出格式**:

- Scope: 允许读写路径、实际读取范围、实际变更文件。
- Findings: 文件路径、类型名、API 名、风险和 breaking change 影响。
- Patch Summary: 修改点、删除旧名 / alias / fallback、未完成项。
- Verification: 命令、工作目录、结果、失败摘要、未运行命令及原因。
- Handoff: 需要主 agent 决策的问题。

## 阶段 1: 033 Baseline Audit And Scope Lock

**目标**: 基于 032 已提交状态，列出 033 仍处于过渡态的 API、结构和小框架入口，锁定本轮最小可落地范围。

**成功标准**:

- 输出当前链路事实清单：`TalosService -> Coordinator -> AgentHarness -> AgentRuntime SPI -> KernelAgent`。
- 列出 033 目标 API 与当前代码差距：`resume` / `resume_streamed`、`AgentRunContext`、`AgentRunOptions`、`AgentError`、`AgentRouter`、capability validation、run lifecycle framework。
- 明确哪些只写 spec / TODO，哪些进入本轮代码实现。
- 明确不实现 CodexAgent / ClaudeAgent / RemoteAgent，不把 Service timeline recovery 下沉到 Harness。

**测试**:

- `rg` 检查 `AgentRunParams`、`AgentInputParams`、`AgentCapabilities`、`AgentRuntime`、`AgentRegistry`、`TimelineEventEnvelope` 调用点。
- `git diff --check`。

**Subagent 分工**:

- 并发启动 `api-boundary-reviewer` 只读审查 API 边界。
- 并发启动 `verification-runner` 只读整理现有验证命令矩阵。
- 主 agent 汇总后更新 spec / plan 范围。

**阶段 1 审计结果**:

- 当前真实链路是 `TalosService -> TurnService/TurnExecutor -> Coordinator -> AgentHarness -> AgentRuntime SPI -> KernelAgent`。
- 当前 `resume` / `resume_streamed` 未落地；dynamic input continuation 仍由 Service 提交 tool result 后携带 `resume_context` 再次调度。
- 当前 `AgentRunContext`、`AgentRunOptions`、`AgentError`、`AgentRouter` 尚未落地；`AgentCapabilities` 已存在但缺少统一 validation 入口。
- 当前 run lifecycle 已有 active turn、backlog、interrupt、submit_input 雏形，但仍集中在 `AgentHarnessImpl`，需要后续阶段拆出清晰边界。
- 本轮代码实现进入阶段 2-9；CodexAgent / ClaudeAgent / RemoteAgent、Gateway / SDK 外部 resume API、attach、跨进程 backend continuation recovery 只保留 spec / TODO。
- Service timeline live recovery / crash recovery 仍在 Service / Gateway 层，不下沉到 Harness。

**阶段 1 验证**:

- 已完成 `rg "AgentRunParams|AgentInputParams|AgentCapabilities|AgentRuntime|AgentRegistry|TimelineEventEnvelope" talos-rs` 静态审查。
- 已完成 `git diff --check`，无 whitespace error。
- 已由空白上下文 subagent 完成 API 边界和命令矩阵只读审查。

**状态**: 已完成

## 阶段 2: Naming Guardrails And Duplicate DTO Cleanup

**目标**: 把 033 命名规则固化到代码、文档和 review guardrails，防止新增重复 DTO 或同义 API。

**成功标准**:

- 根目录 `AGENTS.md` 和 033 spec 对 `*Request` / `*Response`、`*Params` / `*Result`、`*Record` / `*Store` 的规则一致。
- 代码中无新增 `RuntimeRunParams`、`HarnessRunParams`、`KernelRunResult`、`AgentUsage`、`RunUsage`、`AgentItem`。
- 当前存在的 runtime-private 类型被确认只在 runtime 内部使用，没有跨 Harness / Coordinator 边界。
- 对新增 public API 的英文 doc comment 缺口有明确修复清单。

**测试**:

- `rg "RuntimeRunParams|HarnessRunParams|KernelRunParams|RuntimeRunResult|HarnessRunResult|KernelRunResult|AgentUsage|RunUsage|AgentItem" talos-rs talos-app` 必须无命中。
- `rg "RuntimeRunParams|HarnessRunParams|KernelRunParams|RuntimeRunResult|HarnessRunResult|KernelRunResult|AgentUsage|RunUsage|AgentItem" spec` 只允许命中禁止重复 DTO / 审查清单，不允许作为目标 API 或新增结构出现。
- `rg "pub (trait|struct|enum|fn)" talos-rs/crates/orchestration/agent/src talos-rs/crates/orchestration/coordinator/src`
- `just fmt`
- `just check -p talos-agent -p talos-coordinator -p talos-service -p talos-types`

**Subagent 分工**:

- `api-boundary-reviewer` 只读给出重复 DTO 和注释缺口。
- `type-model-worker` 在主 agent 定稿后串行修 doc comment / 旧名残留。

**阶段 2 执行结果**:

- 确认根目录 `AGENTS.md` 与 033 spec 的 `*Request` / `*Response`、`*Params` / `*Result`、`*Record` / `*Store` 规则一致。
- 修正阶段 2 重复 DTO 验证命令：代码侧必须无命中，spec 侧只允许命中禁止清单 / 审查清单。
- 补齐 Agent / Coordinator public API 和框架边界 crate-private SPI 的英文 doc comment，包括 Harness helper、MemoryFlushManager、MockAgent、Coordinator traits、Coordinator maintenance drains、Kernel request adapter、Runtime abort handle、Kernel event normalization。
- 补齐 `AgentDispatchResumeContext` / `AgentDispatchResumeToolCall` 的边界注释，明确它只表达同一 Talos turn 内的 dynamic tool result continuation，不是 backend continuation token、attach handle 或 Gateway / SDK public recovery DTO。
- 确认 runtime-private 类型未跨 Harness / Coordinator 边界泄漏，Agent / Coordinator 中没有 store record 泄漏。

**阶段 2 验证**:

- `just fmt`
- `just check -p talos-agent -p talos-coordinator -p talos-service -p talos-types`
- `git diff --check`
- `rg "RuntimeRunParams|HarnessRunParams|KernelRunParams|RuntimeRunResult|HarnessRunResult|KernelRunResult|AgentUsage|RunUsage|AgentItem" talos-rs talos-app` 无命中。
- `rg "ActiveTurnRecord|TimelineEventRecord|SessionRecord|ProjectRecord|IdempotencyRecord" talos-rs/crates/orchestration/agent/src talos-rs/crates/orchestration/coordinator/src` 无命中。
- 空白上下文 subagent 完成阶段 2 只读复核；反馈的注释缺口已修复。

**状态**: 已完成

## 阶段 3: Agent Run Context And Options Model

**目标**: 最小化引入 `AgentRunContext` 和 `AgentRunOptions`，并把 `turn_id` 收敛为 Agent 内部边界的必填 run identity。

**成功标准**:

- `AgentRunContext.turn_id`、`AgentInputParams.turn_id`、`AgentInterruptParams.turn_id` 在 Agent 内部边界必填；Gateway / SDK 不要求客户端传，Service 负责生成或恢复真实 `turn_id`。
- 内部 `run_id` 和 `turn_id` 写死为同一个值：`run_id == turn_id`。如果代码或文档需要表达 run identity，只能映射到真实 `turn_id`，不能新增第二套 run id。
- 如果历史文档、日志或代码出现 `trun_id` 拼写，迁移期只能按同一个 `turn_id` 解读并纠正；`trun_id` 不是合法字段或别名。
- 新增最小 `AgentRunContext` 概念，至少携带 `session_id`、`turn_id`、`message_id`、`user_id?`、`workspace_id?`、`project_root?`；本阶段允许与过渡用 `TalosSession` 并存。
- `AgentRunContext` 只表达 Agent 执行上下文，不是 `TalosContext` 改名，不携带 DB Record 或 Gateway DTO。
- `AgentRunOptions` 本轮只包含当前真实可达的 `current_model?`；`continuation_policy` 不进入本阶段，dynamic input continuation 交给阶段 8 的 `AgentResumeParams` / `AgentInput`。
- runtime 私有配置仍在 `AgentDefinition.runtime_config`。
- 本阶段不实现完整 unsupported option fail-fast；capability validation 统一放到阶段 4。
- 完整移除 `AgentRunParams.session` 的公共边界依赖放到最后阶段，等待 `AgentRunOptions`、workspace resolution、runtime capability 稳定后执行。

**测试**:

- Coordinator dispatch / stream tests。
- Service turn tests，尤其 start turn、retry、dynamic tool result resume。
- `just fmt`
- `just check -p talos-coordinator -p talos-service -p talos-agent`
- `just test -p talos-coordinator -p talos-service -p talos-agent`
- `rg "TalosSession" talos-rs/crates/orchestration/agent/src talos-rs/crates/orchestration/coordinator/src` 复查保留点是否都标记为过渡依赖。

**Subagent 分工**:

- `api-boundary-reviewer` 只读评审 `AgentRunContext` 字段和 `TalosContext` / `TalosSession` 替代关系。
- `type-model-worker` 只在主 agent 决策后修改共享类型和调用点。

**阶段 3 执行结果**:

- 新增最小 `AgentRunContext`，携带 `session_id`、`turn_id`、`message_id`、`user_id?`、`workspace_id?`、`project_root?`。
- 新增 `AgentRunOptions`，本轮只包含 `current_model?`。
- `AgentRunParams`、`AgentInputParams`、`AgentInterruptParams`、`AgentDispatchParams`、`CancelActiveTurnParams` 内部边界改为必填 `turn_id`。
- Coordinator 负责从 Service 已创建或已恢复的 turn 构造 `AgentRunParams`，保证 `AgentRunContext.turn_id` 是唯一 Agent run identity。
- Harness / Runtime / Mock 不再为缺失 turn 生成 fallback id，内部 run identity 固化为 `run_id == turn_id`。
- `AgentRunParams.session` 和 Kernel adapter 的 `TalosSession` 依赖保留为过渡点，完整迁移放到阶段 9。

**阶段 3 验证**:

- `just fmt`
- `just check -p talos-agent -p talos-coordinator -p talos-service -p talos-types`
- `just test -p talos-coordinator -p talos-service -p talos-agent`
- 失败 case 已单独修复并重跑：`just test -p talos-agent agent::tests::test_run_queues_second_active_turn_for_same_session`
- 复查无内部 `turn_id: Option`、fallback turn id、独立 `run_id` 或 `trun_id` 代码入口。
- `git diff --check`

**状态**: 已完成

## 阶段 4: Capability Validation Framework

**目标**: 抽取 Agent capability validation 小框架，让 unsupported run option、input、interrupt、resume 在进入 runtime 前显式失败。

**成功标准**:

- `AgentCapabilities` 扩展项经过实际需求审查，不为了 Codex / Claude 提前暴露不可达字段。
- 本轮不新增 standalone resume / continuation capability；当前 `resume_context` / dynamic continuation 使用 `dynamic_input` 门禁，不一次性暴露 `mcp`、`sandbox`、`structured_output`、`attach_live_task` 等候选全集。
- Harness / Coordinator 有单一 capability validation 入口。
- 从本阶段开始处理 unsupported option fail-fast；阶段 3 只负责 `AgentRunOptions` 结构成型。
- capability validation 是 unsupported option / input / interrupt / resume 的唯一 fail-fast 入口，不允许在 runtime adapter、test helper 或参数构造路径里复制校验。
- `dynamic_input`、`interrupt`、`streaming` 的现有行为保持不变。
- 新增 capability 错误映射到稳定错误语义，不返回 runtime-private message。

**测试**:

- Agent factory / descriptor tests。
- Coordinator routing / dispatch tests。
- 新增 unsupported capability fail-fast tests。
- `just fmt`
- `just check -p talos-agent -p talos-coordinator`
- `just test -p talos-agent -p talos-coordinator`

**Subagent 分工**:

- `api-boundary-reviewer` 只读检查 capability 是否被当作 UI metadata 使用。
- `type-model-worker` 执行能力模型和 tests。

**阶段 4 执行结果**:

- 新增 `AgentCapability` / `AgentCapabilityValidator` 小框架，统一处理 run、streaming run、interrupt、input、dynamic continuation 的 capability fail-fast。
- `AgentHarness` 增加默认 validation helper，Coordinator 和 `AgentHarnessImpl` 复用同一 validator，不把规则散到 runtime adapter。
- `AgentHarness::descriptor` 默认能力改为 fail-closed 的 `AgentCapabilities::none()`，`MockAgent` 和测试 harness 显式声明自身能力。
- `resume_context` 本轮映射到现有 `dynamic_input` 能力门禁，不新增不可达的 standalone resume capability 字段。
- `AgentRunOptions.current_model` 保留统一校验入口；当前只做过渡期一致性校验，不引入额外 model capability。
- `AgentRunParams::new` 不再把空字符串模型写入 `AgentRunOptions.current_model`，保持现有 runtime fallback 语义。
- Coordinator fail-fast 覆盖 unsupported streaming、interrupt、dynamic input、dynamic continuation，并验证失败时不会调用 harness。

**阶段 4 验证**:

- 空白上下文 subagent 已完成只读审查；反馈的 default descriptor fail-closed、resume_context capability gate、model option 一致性入口已处理。
- `just fmt`
- `just check -p talos-agent -p talos-coordinator`
- `just test -p talos-agent -p talos-coordinator`，54/54 通过。
- `rg` 复查 capability validation 集中在 `AgentCapabilityValidator`、`AgentHarness` validation helper、Coordinator / Harness 入口，没有进入 runtime adapter。
- `rg` 复查无内部 `turn_id: Option`、`trun_id`、重复 DTO 或裸 `runtime_ref: String`；外部 turn API 的 optional `turn_id` 不属于本阶段内部 Agent 边界。
- `git diff --check`

**状态**: 已完成

## 阶段 5: Agent Error Mapping Framework

**目标**: 在 Agent 域引入真实 `AgentError` / `AgentErrorKind`，避免 `TalosError` 在 Harness / Runtime / Coordinator 之间丢失语义。

**成功标准**:

- 明确 `runtime private error -> AgentError -> Coordinator / Service error -> Gateway / SDK response` 的映射路径。
- `AgentErrorKind` 覆盖 033 spec 中的基础分类，至少先落地当前代码能产生的 `RunNotFound`、`RunConflict`、`InputRejected`、`CapabilityUnsupported`、`RuntimeError`、`Timeout`、`Interrupted`。
- `AgentError` 是 Agent domain 内部错误，不导出到 generated TS schema，不作为 HTTP / JSON-RPC response body。
- generated TS schema、HTTP / JSON-RPC response body、Rust SDK 和 TS SDK 不新增 `AgentError` / `AgentErrorKind` 公共 DTO。
- Service 边界负责把 `AgentError` 转成 `TalosError` 或产品错误码；Gateway / SDK 不看到 Agent / runtime private error object。
- `TalosError::ResourceNotFound`、`TalosError::Conflict` 等过渡映射按调用点语义区分，不能一刀切。
- Agent / Harness / Runtime / Coordinator 内部使用 Agent error，Service 边界转换回 `TalosError`。
- Gateway / SDK 不看到 Codex / Claude / Kernel 私有错误对象。

**测试**:

- Harness interrupt / input tests。
- Coordinator registry / routing tests。
- Service interrupt / dynamic tool result tests。
- `just fmt`
- `just check -p talos-agent -p talos-coordinator -p talos-service -p talos-gateway`
- `just test -p talos-agent -p talos-coordinator -p talos-service -p talos-gateway`

**Subagent 分工**:

- `api-boundary-reviewer` 只读给出当前错误路径和语义丢失点。
- `type-model-worker` 在 agent/coordinator 范围实现映射。
- Service 映射改动由主 agent 串行合并。

**阶段 5 执行结果**:

- 新增 `AgentError` / `AgentErrorKind` / `AgentResult`，作为 Agent domain 内部错误结构，不进入 `talos-types`、Gateway DTO 或 generated TS。
- `AgentHarness` 的 run / run_streamed / interrupt / submit_input 和 validation helper 改为 `AgentResult`。
- `AgentRuntime` SPI 和 streamed completion handle 改为 `AgentResult`。
- `AgentRegistry` 内部改为 `AgentResult`，Coordinator 通过 `map_agent_result` 在 `AgentDispatchRuntime` / `SessionRuntime` / `Coordinator` 对外边界统一映射回 `TalosError`。
- `AgentCapabilityValidator` 产生 `CapabilityUnsupported`；run identity mismatch 产生 `InvalidExecutionContext`；interrupt missing/wrong turn 产生 `RunNotFound`；dynamic input replay 产生 `InputRejected`；dynamic input timeout 产生 `Timeout`；runtime join / stream mapping failure 产生 `RuntimeError`。
- Service turn stream 不再把 executor error 二次包装成 `TalosError::GatewayError`，保留 Coordinator 已映射出的产品错误语义。
- 增加 Agent error mapping 单测、direct Harness error kind 断言、Service executor error semantics 断言。

**阶段 5 验证**:

- 空白上下文 subagent 完成只读审查；反馈的 stream task TalosError 残留、Service `GatewayError` 二次包装、dynamic input 粗映射、registry 内部 TalosError 已处理。
- `just fmt`
- `just check -p talos-agent -p talos-coordinator -p talos-service`
- `just test -p talos-agent -p talos-coordinator -p talos-service`，122/122 通过。
- `rg "AgentError|AgentErrorKind" talos-rs/crates/gateway talos-rs/crates/common talos-app -g '*.rs' -g '*.ts' -g '*.json'` 无命中。
- `rg "GatewayError\\(err\\.to_string\\(\\)\\)" talos-rs/crates/orchestration/service/src/turn` 无命中。
- `rg` 复查 Agent run/control API 不再返回 `TalosResult<AgentRunResult>` / `TalosResult<AgentInterruptResult>` / `TalosResult<AgentInputResult>`。
- `git diff --check`

**状态**: 已完成

## 阶段 6: Run Lifecycle Framework

**目标**: 把 `run`、`run_streamed`、`interrupt`、`submit_input`、dynamic input resume、active runtime handle、queued run / backlog 收敛到 Harness-owned lifecycle 模型。

**成功标准**:

- Harness lifecycle 入口有清晰模块边界，不继续把状态机堆进单一大文件。
- `submit_input` 继续作为唯一输入桥接 API，扩展通过 `AgentInput` tagged union，不新增 parallel `submit_*` 方法族。
- `interrupt`、queued run、reserved / running handle、dynamic input resume 的状态语义可解释且有测试。
- `AgentInputParams.turn_id` 参与 Harness lifecycle 校验，dynamic input 必须归属于对应 active / pending turn，不能只按 session、call id、tool name 匹配。
- Service timeline / active turn store 仍归 Service，不进入 Harness。
- Runtime SPI 仍只处理 backend execution，不接收 Service store record。

**测试**:

- `just fmt`
- `just check -p talos-agent -p talos-coordinator -p talos-service`
- `just test -p talos-agent`
- `just test -p talos-coordinator -p talos-service`
- 新增 lifecycle 状态机 tests：reserved、running、queued、cancelled、dynamic input waiting、resume。
- 新增 dynamic input wrong-turn tests，验证错误 turn_id 不会写入 transcript 或恢复 runtime。
- `rg "ActiveTurnRecord|TimelineEventRecord|SessionRecord" talos-rs/crates/orchestration/agent/src talos-rs/crates/orchestration/coordinator/src`

**Subagent 分工**:

- `lifecycle-worker` 独占 agent lifecycle 相关文件。
- `api-boundary-reviewer` 并发只读审查 store record 泄漏。
- 主 agent 合并后跑 agent / coordinator / service tests。

**阶段 6 执行结果**:

- 新增 `agent/lifecycle.rs`，把 `ActiveTurnHandle`、`QueuedTurn`、reserve / enqueue / finish / dispatch 这些 Harness-owned lifecycle 状态和调度入口从 `agent.rs` 抽出。
- `try_reserve_active_turn`、`activate_reserved_turn`、`clear_active_turn`、backlog queue、active-turn drain、session idle check 统一收敛到 `agent/lifecycle.rs`，`agent/session.rs` 保持 memory / session 操作职责。
- 保持 Service timeline / active turn store 在 Service 层；Harness lifecycle 只维护短生命周期内存态和 session backlog。
- Kernel runtime 的 transcript `invocation_id` 改为复用 `AgentRunContext.turn_id`，让 Agent 内部 run identity、memory transcript grouping 和 Service turn identity 对齐。
- `submit_input(AgentInputParams)` 继续作为唯一 input bridge；dynamic tool result 校验现在要求 transcript request / response 属于同一个 `turn_id`。
- `submit_dynamic_tool_result` 写入 `DynamicToolResponse` 时使用原始 `turn_id` 作为 transcript `invocation_id`，不再生成独立 invocation id。
- `resume_context` replay 只读取同一 `turn_id` 的 dynamic request / response；错误 turn 不会恢复 runtime，也不会触发模型调用。
- `resume_context: Some` 但缺 `tool_call`、或 transcript 缺少同一 `turn_id` 下的 request 时显式失败，不再 fallback 成普通 run 或 `Null` arguments。
- 新增 wrong-turn / malformed-resume tests，覆盖错误 `turn_id` 不写入 dynamic response、错误 `turn_id` 不恢复 streamed resume、缺 tool call 不触发模型、缺 request 不触发模型。
- 空白上下文 subagent 完成只读复核；反馈的 malformed resume fallback 和 lifecycle helper 分散问题已修复。

**阶段 6 验证**:

- `just fmt`
- `just check -p talos-agent -p talos-coordinator -p talos-service`
- `just test -p talos-agent`，46/46 通过。
- `just test -p talos-coordinator -p talos-service`，80/80 通过。
- `rg "ActiveTurnRecord|TimelineEventRecord|SessionRecord" talos-rs/crates/orchestration/agent/src talos-rs/crates/orchestration/coordinator/src` 无命中。
- `rg "ActiveTurnRecord|TimelineEventRecord|SessionRecord|ProjectRecord|IdempotencyRecord" talos-rs/crates/orchestration/agent/src talos-rs/crates/orchestration/coordinator/src` 无命中。
- `rg "trun_id" talos-rs talos-app` 无命中。
- `rg -n "\\brun_id\\b" talos-rs/crates/orchestration/agent/src talos-rs/crates/orchestration/coordinator/src` 无命中。
- `rg "AgentError|AgentErrorKind" talos-rs/crates/gateway talos-rs/crates/common talos-app -g '*.rs' -g '*.ts' -g '*.json'` 无命中。
- `rg "AgentAttachParams|attach\\(" talos-rs/crates/orchestration/agent/src talos-rs/crates/orchestration/coordinator/src` 无命中。
- `rg "continuation_policy" talos-rs talos-app` 无命中。
- `git diff --check`

**状态**: 已完成

## 阶段 7: Agent Router And Definition Framework Hardening

**目标**: 强化 `AgentDefinition -> AgentHarnessFactory -> AgentRegistry -> AgentRouter -> Coordinator` 链路，为多 Agent 选择和未来协作留下稳定 API。

**成功标准**:

- `AgentRouter` 或等价选择入口明确 `resolve_agent(AgentSelectionParams) -> AgentSelectionResult`。
- 内部选择优先级清晰：internal explicit `agent_id` > internal explicit `agent_name` > user/session binding > coordinator routing policy > configured default agent。
- 本轮不在 Gateway / SDK `StartTurnRequest` 暴露 `agent_id` / `agent_name`，外部显式选择留到多 Agent 产品形态明确后再开放。
- `workspace_id` 不直接选择 Agent，只能通过显式 routing policy 或 binding 参与。
- `AgentRegistry` 不允许 alias、重复覆盖或 descriptor id 漂移。
- `AgentDefinitionLoader`、`AgentHarnessFactory`、`AgentRegistry` 的注释说明 owner、调用方和禁止职责。

**测试**:

- Config loader tests。
- Registry tests。
- Coordinator selection tests 覆盖 default、internal explicit id/name、binding/routing policy、missing/disabled agent。
- `just fmt`
- `just check -p talos-config -p talos-agent -p talos-coordinator -p talos-base`
- `just test -p talos-config -p talos-agent -p talos-coordinator -p talos-base`

**Subagent 分工**:

- `router-definition-worker` 负责 config / agent definition / coordinator registry routing 相关文件。
- `api-boundary-reviewer` 只读审查 workspace_id 是否被误当 Agent selection key。

**阶段 7 执行结果**:

- 新增 Coordinator-owned `AgentRouter` 小框架，统一封装 `resolve_agent(AgentSelectionParams) -> AgentSelectionResult`。
- `AgentSelectionParams` 支持内部 explicit `agent_id`、内部 explicit `agent_name` 和 routing policy context；未暴露到 Gateway / SDK。
- `AgentSelectionResult` 返回 selected `agent_id`、`AgentRegistration` 和 `AgentSelectionSource`，便于 Coordinator dispatch / diagnostics 复用。
- Coordinator dispatch、stream、session runtime control 统一通过 `AgentRouter` 选择 Agent，不再在 `CoordinatorImpl` 内散落 routing policy + registry lookup。
- 选择优先级落地为 internal explicit id > internal explicit name > routing policy > configured default Agent。
- `workspace_id` 没有进入 `AgentSelectionParams`，只继续作为 run context / session context 字段传给 Harness。
- Router 会拒绝 disabled Agent，避免选择不可执行 Harness。
- 删除旧 `coordinator/stream.rs` 的单一 route helper，避免 selection 逻辑分散。
- `talos-base` embedded runtime 的 timeline subscription 修正为 `turn_id: Some(ack.turn_id)`，保持 timeline 产品 API 的 optional filter 语义，不影响 Agent 内部必填 `turn_id`。

**阶段 7 验证**:

- `just fmt`
- `just check -p talos-config -p talos-agent -p talos-coordinator -p talos-base`
- `just test -p talos-coordinator`，21/21 通过。
- `just test -p talos-config -p talos-agent -p talos-coordinator -p talos-base`，128/128 通过。
- `git diff --check`
- `rg "RuntimeRunParams|HarnessRunParams|KernelRunParams|RuntimeRunResult|HarnessRunResult|KernelRunResult|AgentUsage|RunUsage|AgentItem" talos-rs talos-app` 无命中。
- `rg "ActiveTurnRecord|TimelineEventRecord|SessionRecord|ProjectRecord|IdempotencyRecord" talos-rs/crates/orchestration/agent/src talos-rs/crates/orchestration/coordinator/src` 无命中。
- `rg -n "workspace_id" talos-rs/crates/orchestration/coordinator/src/router.rs talos-rs/crates/orchestration/coordinator/src/coordinator.rs` 仅命中 run context 传递和 router 注释，未作为 Agent selector。

**状态**: 已完成

## 阶段 8: Resume And Backend Continuation Design Slice

**目标**: 在 run lifecycle 下落地内部 `resume` / `resume_streamed` API slice，明确它和 Service timeline recovery 的边界。

**成功标准**:

- `resume` / `resume_streamed` 是 AgentHarness / AgentRuntime 标准 API，业务范围注释清楚。
- `resume_streamed` 只表示同一 `turn_id` 下的 continuation stream，不是第二次 `run_streamed`，也不是客户端 attach / Service timeline recovery。
- 当前 dynamic input resume 映射到标准 `resume` / `resume_streamed` 语义；不暴露 Gateway / SDK resume API。
- `resume_streamed` 必须复用原始 `turn_id`，不能引入独立 `run_id`、`trun_id`、backend thread id 或 runtime session id 跨层流动。
- 如果引入 `AgentContinuationRef`，它是 internal persisted envelope，不是 Gateway / SDK public DTO。
- raw backend token 只在对应 runtime 内解码，不以裸 `runtime_ref: String` 跨多层流动。
- `attach` 只作为未来可选能力，不在没有真实 backend task 接回能力时暴露半成品 API。
- `AgentResumeParams` 继续携带或解析到原始 `turn_id`，保证 `run_id == turn_id` 映射不漂移。

**测试**:

- Dynamic tool result resume tests。
- Runtime unsupported continuation tests。
- Service timeline recovery tests，确认 live recovery API 未下沉。
- `just fmt`
- `just check -p talos-agent -p talos-coordinator -p talos-service`
- `just test -p talos-agent -p talos-coordinator -p talos-service`
- `rg "AgentAttachParams|attach\\(" talos-rs/crates/orchestration/agent/src talos-rs/crates/orchestration/coordinator/src` 复查本轮没有暴露 attach API。
- `rg "trun_id" talos-rs talos-app` 必须无命中。
- `rg "run_id" talos-rs/crates/orchestration/agent/src talos-rs/crates/orchestration/coordinator/src` 复查没有独立 public run identity 字段；如命中诊断语义，必须映射到同一个 `turn_id`。

**Subagent 分工**:

- `lifecycle-worker` 负责 Harness / Runtime resume API。
- `api-boundary-reviewer` 只读审查 Service recovery 与 backend continuation 是否混层。

**阶段 8 执行结果**:

- 新增内部 `AgentResumeParams`，包裹原始 `AgentRunParams`，并提供 `turn_id()` / `session_id()`，明确 resume 只继续同一个 Talos turn。
- `AgentHarness` 新增 `resume` / `resume_streamed` 和对应 validation helper；默认实现复用 Harness facade，`AgentHarnessImpl` 显式走 lifecycle mode。
- `AgentRuntime` SPI 新增 `resume` / `resume_streamed`；`KernelAgent` 复用原有 Kernel resume 逻辑，但入口语义已经与 `run_streamed` 分离。
- Harness lifecycle 队列新增内部 `AgentRunMode::{Start, Resume}`，排队后仍保留 start / resume 选择，不会退回普通 run。
- `AgentCapabilityValidator` 新增 `validate_resume` / `validate_streaming_resume`，要求 resume 必须携带 continuation context，并继续复用 `dynamic_input` / `streaming` capability fail-fast。
- 普通 `run` / `run_streamed` 携带 `resume_context` 时 fail-fast，必须改走 `resume` / `resume_streamed`；trait 默认 `resume*` 不再退化调用普通 run。
- Coordinator 在 `resume_context.is_some()` 时调用 `agent.resume` / `agent.resume_streamed`，普通 start 继续调用 `run` / `run_streamed`。
- Dynamic input resume 测试改为直接调用 `resume_streamed(AgentResumeParams)`；新增缺失 `resume_context`、普通 run 误带 `resume_context` 的 fail-fast 测试。
- 新增 Coordinator 测试验证非流式和流式 resume dispatch 都走 `resume` / `resume_streamed`，不是旧 `run` / `run_streamed`。
- 修复 Gateway core 测试对必填 `AgentDispatchParams.turn_id` 和 `submit_dynamic_tool_result(..., turn_id, ...)` 的旧签名用法。
- 未新增 Gateway / SDK resume API，未新增 `AgentAttachParams` / `attach`，未新增 `AgentContinuationRef` 或裸 `runtime_ref`。

**阶段 8 验证**:

- `just fmt`
- `just check -p talos-agent -p talos-coordinator -p talos-service -p talos-gateway`
- `just test -p talos-agent -p talos-coordinator -p talos-service -p talos-gateway`，139/139 通过。
- 空白上下文 subagent 阶段 8 只读审查反馈的两个阻塞已修复：普通 run 不再接受 `resume_context`，Gateway core 测试不再按旧 optional `turn_id` / 旧 trait 签名实现。
- `rg -n "\\btrun_id\\b|\\btrunId\\b" talos-rs talos-app` 无命中。
- `rg -n "AgentAttachParams|attach\\(" talos-rs/crates/orchestration/agent/src talos-rs/crates/orchestration/coordinator/src` 无命中。
- `rg -n "\\brun_id\\b|\\brunId\\b" talos-rs/crates/orchestration/agent/src talos-rs/crates/orchestration/coordinator/src talos-rs/crates/orchestration/service/src` 无命中。

**状态**: 已完成

## 阶段 9: Complete AgentRunContext Migration, Schema, SDK, Docs, E2E And Smoke

**目标**: 在前序 `AgentRunOptions`、workspace resolution、runtime capability 稳定后，完成 `AgentRunContext` 对 `AgentRunParams.session` 公共边界依赖的替换；同步所有外部可见影响，并完成最终质量门禁、完整 E2E 和 Web / TUI smoke。

**成功标准**:

- `AgentRunParams` 公共边界不再要求完整 `TalosSession`；Runtime 消费 `AgentRunContext`、`AgentRunOptions` 和必要的共享 Agent domain inputs。
- 任何仍需要完整 `TalosSession` 的代码只能留在 Service / Coordinator adapter 或明确标记的临时内部转换点。
- `SubmitDynamicToolResultRequest` 不新增 `turn_id`；Service 继续负责把 session-level 产品 API 转换成内部必填 `turn_id`。
- `AgentRunOptions` 仍只包含本轮真实可达 option；`continuation_policy` 未进入代码 API。
- `AgentError` / `AgentErrorKind` 未进入 Gateway / SDK / generated TS schema。
- `resume_streamed` 只用于同一 `turn_id` continuation；`attach` 仍是未来可选能力，未暴露半成品 API。
- `trun_id` 不出现在代码或 generated artifacts；任何诊断里的 `run_id` 都能映射回同一个 `turn_id`。
- unsupported option / capability 仍由 capability validation framework 集中 fail-fast。
- 如果 app protocol / schema / generated TS 变化，必须由生成命令产生并提交。
- Rust SDK、TS SDK、Web、TUI、README、spec registry 与代码 API 一致。
- 进入完整 E2E 前，所有相关非 E2E case 已通过。
- 完整 E2E 只运行一次；失败后只跑失败 group / case，不能重复跑完整 E2E。
- E2E 后自动执行 Web / TUI 核心 smoke。

**测试**:

- `just fmt`
- 必要的 `just check -p <package>` / `just test -p <package>`
- 运行“7 个硬性落地点的全局验收命令”，并记录每条命令结果。
- 如果 schema 变化：`just gen-app-protocol`
- 如果前端/SDK 变化：`pnpm --dir talos-app lint`、`pnpm --dir talos-app typecheck`、`pnpm --dir talos-app test`、`pnpm --dir talos-app check:repo`
- 完整 E2E：`just e2e` 只运行一次。
- 失败后仅运行 `just e2e-group <group>` 或 `just e2e-case <case>`。
- Web smoke 启动命令：在 `talos-rs` 运行 `cargo run -p talos-server --bin talos-server`，另起终端在仓库根目录运行 `pnpm --dir talos-app dev`。
- Web smoke 验证路径：打开前端，验证 session 创建、start turn、timeline stream/recovery、dynamic input/interrupt、agent routing/default agent。
- TUI smoke 命令：在 `talos-rs` 运行 `cargo run -p talos-tui --bin talos -- models`、`cargo run -p talos-tui --bin talos -- status`、`cargo run -p talos-tui --bin talos -- exec "hello"`。
- TUI smoke 验证路径：确认 embedded runtime 能启动，models/status 返回，exec 能创建一个 turn 并渲染 timeline 输出。

**Subagent 分工**:

- `schema-sdk-ui-worker` 独占 schema / SDK / UI / generated artifacts。
- `verification-runner` 负责命令记录和报告。
- E2E 失败时按失败 group/case 启动专门排障 subagent，写入范围只限失败相关文件。

**阶段 9 执行结果（进行中）**:

- `AgentRunParams` 已移除完整 `TalosSession` 字段，run 边界改为 `AgentRunContext`、`AgentRunConfig`、`AgentRunSessionState`、`AgentRunOptions` 和 `input`。
- `AgentRunContext` 新增 `owner_user_id` 和 `environment`，保留 workspace/user 校验与 Kernel invocation environment 所需信息。
- 新增 `AgentRunConfig { agent, llm }`，只携带 Runtime 执行需要的配置域；provider lookup 迁移到该结构。
- 新增 `AgentRunSessionState { title, message_count }`，只服务 Harness 的首轮 title generation 判断，不携带 DB Record 或 timeline 状态。
- Kernel request adapter 改为 `AgentKernelParams { context, config, input, ... }`，不再携带完整 `TalosSession`。
- Kernel runtime run path 改为 `memory_store_for_run(context, config)`；session lifecycle 仍使用 `TalosSession`，属于 session management API，不是 run boundary。
- Harness lifecycle、MockAgent、Coordinator tests 改为从 `AgentRunContext` 读取 `session_id`、`message_id` 和 `workspace_id`。
- 未改动 Gateway / SDK / generated TS schema；本切片没有新增 app protocol shape。

**阶段 9 当前验证**:

- `just fmt`
- `just check -p talos-agent -p talos-coordinator -p talos-service -p talos-gateway`
- `just test -p talos-agent -p talos-coordinator -p talos-service -p talos-gateway`，140/140 通过。
- `rg -n "pub struct AgentRunParams|pub session: TalosSession|AgentRunConfig|AgentRunSessionState|AgentRunContext" talos-rs/crates/orchestration/agent/src/service.rs talos-rs/crates/orchestration/agent/src/lib.rs` 确认 `AgentRunParams` 不含完整 session；命中的 `pub session: TalosSession` 属于 `AgentInputParams` input bridge。
- `rg -n "TalosSession" talos-rs/crates/orchestration/agent/src talos-rs/crates/orchestration/coordinator/src -g '*.rs'` 复查剩余命中：session lifecycle API、Coordinator adapter、测试 fixture、session helper。
- `rg -n "continuation_policy" talos-rs talos-app` 无命中。
- `rg -n "AgentError|AgentErrorKind" talos-rs/crates/gateway talos-rs/crates/common talos-app -g '*.rs' -g '*.ts' -g '*.json'` 无命中。
- `rg -n "AgentAttachParams|attach\\(" talos-rs/crates/orchestration/agent/src talos-rs/crates/orchestration/coordinator/src` 无命中。
- `rg -n "\\btrun_id\\b|\\btrunId\\b|\\brun_id\\b|\\brunId\\b" talos-rs/crates/orchestration/agent/src talos-rs/crates/orchestration/coordinator/src talos-rs/crates/orchestration/service/src talos-app` 无命中。
- `rg -n "AgentCapabilityValidator|CapabilityUnsupported|validate_resume|validate_streaming_resume|validate_run\\(" talos-rs/crates/orchestration/agent/src talos-rs/crates/orchestration/coordinator/src -g '*.rs'` 确认 unsupported option / input / interrupt / resume 仍集中在 capability validation framework。
- `git diff --check`
- `just e2e` 已按计划完整运行一次；整体失败在 `06_tools/exit_plan_mode.sh`，报告路径为 `talos-e2e/reports/e2e_report_20260701_162006.md`。后续禁止重复跑完整 E2E，只允许排查并单跑失败 group / case。
- `just e2e-case 06_tools/exit_plan_mode` 已单跑通过，报告路径为 `talos-e2e/reports/e2e_report_20260701_163337.md`。
- Web smoke 使用 `conf/talos-dev.yaml` 启动 `talos-server` 和 Vite：`/health` 返回 `healthy`，`/v1/models` 返回 865 字符响应，页面 body 渲染 231 字符；helper 已停止两个临时 server。
- TUI smoke 使用 `conf/talos-dev.yaml` 完成：`cargo run -p talos-tui --bin talos -- models` 返回 3 个模型，`status` 返回 runtime accessible，`exec "hello"` 成功创建 turn 并输出 timeline 文本。

## 提交前最终 Review 收敛 TODO

本 TODO 来自空白上下文 subagent 只读审查和主线程静态复核。当前 P0 / P1 / P2 均已在本轮落地并完成 scoped 验证。

### P0: 必须提交前修复

1. 已删除 `AgentRunParams::new(session: TalosSession, ...)` 公共构造入口。
   - 问题：`AgentRunParams` 字段已经拆成 `AgentRunContext` / `AgentRunConfig` / `AgentRunSessionState` / `AgentRunOptions` / `input`，但 constructor 仍接收完整 `TalosSession`，违背 run boundary 不接收完整 session 的目标。
   - 落地：constructor 已改为显式接收 `AgentRunContext` / `AgentRunConfig` / `AgentRunSessionState` / `AgentRunOptions` / `input`。
   - 清理：Coordinator、agent examples、agent tests、integration tests 已迁移；从 `TalosSession` 拆分 run parts 的逻辑只保留在 Coordinator adapter、测试 fixture 或示例 helper 中。
   - 验收：`rg -n "AgentRunParams::new\\(" talos-rs/crates/orchestration -g '*.rs'` 只命中新 domain-parts constructor 调用，不再传 `TalosSession`。

2. 已删除 `SessionRuntime::submit_dynamic_tool_result` 不带 `turn_id` 的旧内部入口。
   - 问题：Gateway / Service 外部 API 保持 session-level 是正确的，但 Coordinator 内部 `SessionRuntime` 曾保留无 `turn_id` 的 dynamic input 方法，并返回 recover-turn-id 类错误，形成兼容残留和第二套内部入口。
   - 落地：已从 `SessionRuntime` trait、`CoordinatorImpl` impl、Gateway / Service / access / turn 测试 mock 中删除该方法。
   - 保留：唯一内部路径为 `TalosService -> TurnService -> TurnExecutor::submit_input(AgentDispatchInputParams { turn_id, ... }) -> AgentDispatchRuntime::submit_input(...) -> Coordinator -> AgentHarness.submit_input(...) / resume*`。
   - 验收：旧 recover-turn-id 错误文本无命中；`SessionRuntime` 不再暴露无 `turn_id` 方法，Service 外部 session-level API 仍存在。

### P1: 强建议本轮一并清理

1. 已清理 `talos_types::runtime` 中未使用的旧 Agent / Message / Tool DTO。
   - 问题：`AgentInfo`、`Message`、`MessagePayload`、`ToolCall`、`ToolResult` 仍在 `common/types/src/runtime.rs` 导出，静态扫描未发现真实使用，容易和 `AgentDescriptor`、`AgentEvent`、`RunItem`、timeline tool object 形成重复概念。
   - 落地：已删除这些旧 DTO 和 re-export；未发现外部 API / generated schema / SDK 依赖。
   - 验收：`rg -n "runtime::(AgentInfo|Message|MessagePayload|ToolCall|ToolResult)|\\bAgentInfo\\b|\\bMessagePayload\\b" talos-rs talos-app -g '*.rs' -g '*.ts'` 不再命中旧 runtime DTO。

2. 已取消旧 dynamic tool result dispatch 方法。
   - 问题：位置参数方法和 Harness `submit_input` 形成第二套 input bridge，后续扩展 approval/custom tool result 会继续分裂。
   - 落地：已改为 required `submit_input(AgentDispatchInputParams)`；所有实现和测试 mock 显式实现。
   - 保留：`take_generated_session_title` 继续作为 optional hook，doc comment 已说明它是可选 metadata hook，不是 fallback 兼容路径。
   - 验收：旧 unsupported fallback 文本无命中。

3. 已明确 `AgentRunOptions.current_model` 与 `AgentRunConfig.llm.model` 的单一来源。
   - 问题：当前 `AgentRunOptions.current_model` 是 `AgentRunConfig.llm.model` 的 selected snapshot，命名必须表达这是当前运行的模型快照，不是第二套模型配置来源。
   - 落地：doc comment 已说明 `AgentRunConfig.llm` 是 execution source of truth，`AgentRunOptions.current_model` 只是 selected model snapshot，不能覆盖 runtime config。
   - 验收：相关 doc comment 已明确 source-of-truth 关系，代码中不存在相互覆盖的 model resolution。

### P2: 可跟随 P0/P1 顺手修复

1. Service 恢复 dynamic resume context 时已对缺失 `pending.item_id` fail-fast。
   - 问题：`pending_dynamic_resume_context` 现在允许构造 `AgentDispatchResumeContext { tool_call: None }`，把错误留到 runtime 才发现。
   - 落地：Service 层发现 pending dynamic action 缺失 item id 时直接返回 validation error。
   - 验收：新增 `submit_dynamic_tool_result_requires_original_tool_item_id`，覆盖缺失 item id 不会进入 Coordinator / Runtime。

2. 已重命名 `test_legacy_execution_summary_does_not_gate_final_answer`。
   - 问题：测试名保留 `legacy`，但当前目标是删除兼容残留，容易误导为继续维护旧行为。
   - 落地：已改成 `test_execution_summary_item_does_not_gate_final_answer_merge`。
   - 验收：测试行为不变，只删除 legacy 命名残留。

3. 最终验证报告不要写成“完整 E2E 通过”。
   - 问题：本轮 `just e2e` 完整运行一次失败在 `06_tools/exit_plan_mode.sh`，随后 `just e2e-case 06_tools/exit_plan_mode` 单 case 通过。
   - 落地：最终说明必须准确记录为“完整 E2E 一次运行存在真实 LLM 波动失败，失败 case 单跑通过”，不能简化成完整 E2E 通过。
   - 验收：提交说明 / plan / final report 对 E2E 状态表述一致。

### 收敛验证顺序

1. 完成 P0 后运行：`just fmt`。
2. 运行 scoped Rust 验证：`just check -p talos-agent -p talos-coordinator -p talos-service -p talos-gateway`。
3. 运行 scoped Rust 测试：`just test -p talos-agent -p talos-coordinator -p talos-service -p talos-gateway`。
4. 运行 TODO 中列出的 `rg` 验收命令。
5. 运行 `git diff --check`。
6. 不重复跑完整 `just e2e`；如果 dynamic input 链路改动较大，只按失败或受影响 case 单跑对应 E2E case，并记录原因。

**最终收敛验证**:

- `just fmt`
- `just check -p talos-agent -p talos-coordinator -p talos-service -p talos-gateway`
- `just test -p talos-agent -p talos-coordinator -p talos-service -p talos-gateway`，140/140 通过。
- TODO 中旧 constructor、旧 SessionRuntime 入口、旧 runtime DTO、旧 unsupported 文本、legacy 测试名、`trun_id` / 独立 `run_id` / attach / continuation_policy 静态验收均通过。
- `git diff --check`

**状态**: 已完成

## 阶段 10: Dispatch Boundary Duplicate Context Cleanup

**目标**: 根据独立 subagent 的全局审查结果，收敛 Service -> Coordinator dispatch 边界仍存在的重复上下文字段和旧 result 命名。

**范围**:

- `AgentDispatchParams` 只保留 `turn_id`、prepared `session`、`resume_context?` 和 `input`。
- Service 在跨 Coordinator 边界前，把 per-turn workspace override 和 resume message id 合并进 cloned `TalosSession.context`。
- Coordinator 从 prepared `session.context` 派生 routing context 和 `AgentRunContext`，不再接收外层 `context` / `workspace_id`。
- `AgentDispatchResult.content` 重命名为 `output_text`，对齐 `AgentRunResult.output_text`。
- 删除 `AgentDispatchResult.success`，内部 `TalosResult<AgentDispatchResult>` 已表达成功/失败。

**成功标准**:

- `rg -n "pub (context|workspace_id|content|success):" talos-rs/crates/common/types/src/agent_dispatch.rs` 不再命中旧字段定义。
- `rg -n -A8 "AgentDispatchParams \\{" talos-rs/crates/orchestration/service/src/turn/service.rs talos-rs/crates/orchestration/coordinator/src/tests/core.rs talos-rs/crates/orchestration/coordinator/src/coordinator_tests.rs` 显示所有构造点只包含 `turn_id`、`session`、`resume_context`、`input`。
- `AgentDispatchParams` 不再定义外层 `context` / `workspace_id` 字段。
- `AgentDispatchResult` 不再定义 `content` / `success` 字段。
- Service / Coordinator / app-base / collaboration tool 相关 scoped 编译和测试通过。

**测试**:

- `just fmt`
- `just check -p talos-types -p talos-service -p talos-coordinator -p talos-base -p talos-tools -p talos-gateway`
- `just test -p talos-service -p talos-coordinator -p talos-base -p talos-tools -p talos-gateway`
- `git diff --check`

**验证结果**:

- `just fmt`
- `just check -p talos-types -p talos-service -p talos-coordinator -p talos-base -p talos-tools -p talos-gateway`
- `just test -p talos-service -p talos-coordinator -p talos-base -p talos-tools -p talos-gateway`，211/211 通过。
- `rg -n "pub (context|workspace_id|content|success):" talos-rs/crates/common/types/src/agent_dispatch.rs` 无命中。
- `rg -n -A8 "AgentDispatchParams \\{" talos-rs/crates/orchestration/service/src/turn/service.rs talos-rs/crates/orchestration/coordinator/src/tests/core.rs talos-rs/crates/orchestration/coordinator/src/coordinator_tests.rs` 已确认所有构造点无外层 `context` / `workspace_id`。
- `git diff --check`

**状态**: 已完成
