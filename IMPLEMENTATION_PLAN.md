# Unified Error Code Implementation Plan

本计划推动 Talos 建立统一的 error code 规范，目标是用 Codex 同款的极简方式替换当前基于字符串 `contains` / `starts_with` 的错误猜测映射：内部错误用 typed enum 表达，同一个业务领域只定义一套 error code / kind，跨 crate 产品级错误由 `talos-error` 管理，对外协议暴露稳定的结构化错误信息。

## 执行原则

- 不设计复杂 registry、全局 category 框架、动态 code 注册或跨层万能 trait。
- 借鉴 Codex 的方式：typed error enum + explicit mapper + protocol error info。
- `talos-error` 是跨 crate 产品级错误和 run-level error code 的 owner；HTTP、JSON-RPC、Plugin warning、Skill load 这类边界或领域专属 code 由各自领域 owner 管理。
- 同一个业务领域只能有一套 code / kind；不同层级只能透传或做边界映射，不能每层重新定义一套同义 enum。
- `TalosRunErrorCode` 这类对外 code 可以继续出现在协议类型中，但定义和映射必须由 error 模块 owner 管理。
- 不确定的错误不猜测映射；宁可 `error_info = None` 或 `InternalError`，也不能靠日志文案碰运气。
- `UsageLimitExceeded`、`SessionBudgetExceeded` 在真实 budget policy 落地前只允许 reserved，不允许启用字符串映射。
- 每个阶段完成后自查 dead code、重复 DTO、兼容 fallback、反向测试和 schema / TS drift。

## 全仓库异常处理场景归类

本轮全仓库扫描后，异常处理不能继续只围绕 `TalosRunErrorCode` 展开。Talos 现在至少有五类不同语义的错误 code / kind，需要按 owner 分层治理：

| 场景 | Owner | 当前主要对象 | 标准化规则 |
| --- | --- | --- | --- |
| 产品级内部错误 | `talos-rs/crates/infra/error` | `TalosError` / `TalosResult` | 作为跨 crate 业务错误边界；新增产品级语义先判断是否应落在这里，不能承载 HTTP、JSON-RPC、UI 专用 code |
| Agent run 生命周期错误 | `talos-error` + `agent-kernel` timeline | `TalosRunErrorCode` / `TalosRunErrorInfo` | 只表达 turn/run 失败原因；必须由 typed `TalosError` 或显式调用方传入，禁止从 error message 反推 |
| Agent framework 内部错误 | `orchestration/agent` | `AgentErrorKind` / `AgentError` | 只在 Coordinator / Harness / Runtime 内部流转；跨到 Service/Gateway 前必须映射回 `TalosError` |
| Tool 执行结果错误 | `talos-error` + `action/tools` | `TalosToolErrorKind` / `ToolResultError` | `TalosToolErrorKind` 是 tool failure kind 的单一来源；普通 tool failure 不自动升级为 run error，只有导致 run fail 时才映射到 `ToolFailure` |
| Tool governance / sandbox 错误 | `action/tools-policy` / `action/tools-sandbox` | `ToolPlanError` / governance decision | 表达执行前策略决策；应显式映射到 `TalosToolErrorKind::GovernanceRejection` 或 `PermissionRejection`，禁止靠文案关键词 |
| LLM provider / tool calling 错误 | `common/llm/provider` 与 provider 实现 crate | `TalosError` / `ToolCallingError` | provider 层保留本地错误；跨 Agent run 边界时只映射确定语义，例如 provider unavailable、tool call validation |
| Service / Store / Timeline recovery 错误 | `orchestration/service` / `orchestration/store` | `ServiceResult<T>` / `TalosError` | 负责 session、timeline、idempotency、persistence 语义；不能暴露 DB record，也不能生成 run-level code，除非正在写 run failure event |
| JSON-RPC app protocol 错误 | `gateway/app-protocol` | `AppServerErrorBody` / numeric `AppServerErrorCode` | 只表达 JSON-RPC transport / app-server request 失败；SDK 可以映射为 `TalosError`，但不能和 run error code 混用 |
| HTTP REST 错误 | `gateway/http` | `ApiError` / string code | HTTP code 应由 gateway/http typed enum 或常量生成；当前 handler 内 ad hoc string code 需要收敛，禁止从 message prefix 解析 |
| SDK / Web / TUI 展示错误 | `apps/sdk` / `talos-app` / `apps/tui` | public DTO / UI state | 客户端只消费结构化 code、status、event type；message 仅用于展示和日志，不能作为分支条件 |
| Skill 加载错误 | `common/types::skills` + `action/skills` | `SkillLoadErrorKind` / `SkillLoadError` | Skill 加载失败只定义一套 kind；插件 marketplace 只能把它映射为 `PluginWarningCode`，不能解析 message |
| Skill selection / prompt warning | `common/types::skills` + `action/skills` | `SkillWarningCode` / `SkillWarning` | Skill 选择和注入 warning 只定义一套 code；prompt 层复用该 code，不再手写 string code |
| Plugin 扫描 warning | `common/types::plugin` + `action/plugin` | `PluginWarningCode` / `PluginWarning` | 只表达插件扫描和 marketplace 展示 warning；不能重复定义 skill load error kind |

允许保留的字符串匹配：
- 测试断言输出文案。
- 搜索、路径、markdown、UI 渲染、命令风险识别等非错误分类逻辑。
- 协议方法名或 SSE event 名匹配，例如 `timeline.*`。

不允许保留的字符串匹配：
- 从 `error.to_string()`、HTTP body message、tool output message 推导错误 code / kind。
- 通过 message prefix 选择 HTTP API code。
- 通过自然语言关键词判断 retryable、permission、timeout、budget、provider、tool failure。

## 全仓库残留风险清单

| 优先级 | 位置 | 问题 | 目标整改 |
| --- | --- | --- | --- |
| P0 | `gateway/http/src/server/fs.rs::fs_code` | 通过 `invalid_path:` / `path_outside_root:` 等 message prefix 推导 HTTP error code | 为 FS service 引入 typed error kind 或 gateway/http 私有 typed code mapper，Service 产生结构化 FS 错误，HTTP 只做边界映射 |
| P0 | `gateway/http/src/server/*::map_*_error` | HTTP handler 分散维护 ad hoc string code，缺少统一 code owner 和测试矩阵 | 在 gateway/http 内收敛 `HttpApiErrorCode` 或分 resource typed code 常量，统一 `ApiError` constructor，保留 wire `code: String` |
| P1 | `orchestration/agent/src/agent.rs::map_dynamic_input_error` | 通过 `TalosError::ToolError(message) if message.contains("timed out")` 判断 timeout | 动态输入提交链路应产生 typed timeout / expired error，Agent 只按 `TalosError::Timeout` 或 `AgentErrorKind::Timeout` 分支 |
| P1 | `agent-kernel/src/general_turn/prepare.rs::is_recoverable_compaction_parse_error` | 通过 parse error message 判断 compaction parse failure 是否可恢复 | memory summary / persistence decision parser 返回 typed parse failure kind，compaction 决策按 kind 判断 |
| P1 | `apps/tui/src/stream.rs::is_turn_active_conflict` | TUI 通过 conflict message 判断 active turn 状态 | app-server / SDK 应保留更具体 conflict code，例如 active turn running；TUI 按 code/status 分支 |
| P2 | `action/mcp/src/lib.rs::map_service_error` | MCP service error 全部格式化为 `TalosError::ToolError`，丢失 source/kind | MCP adapter 增加结构化 source/kind，落到 `TalosToolErrorKind` 或 provider/tool boundary |
| P2 | `apps/sdk/src/app_server.rs::map_app_server_error` | JSON-RPC numeric code 到 `TalosError` 的反向映射较粗，CONFLICT / VALIDATION 语义丢失 | app-protocol 如需细分，新增明确 numeric code；SDK 不解析 message |
| P2 | 测试中的 `error.to_string().contains(...)` | 部分测试仍验证错误文案，容易固化临时 message | 能断言 typed kind 的测试改为 `matches!`；纯文案测试只保留在 UI / display 语义下 |

## 当前猜测映射清单

### Turn Error 猜测映射

改造前位置：`talos-rs/crates/orchestration/agent-kernel/src/timeline.rs::classify_turn_error`

必须迁移的猜测：

| 当前 code | 当前触发方式 | 目标迁移方式 |
| --- | --- | --- |
| `ContextCompactionFailed` | error string contains `prompt budget remains exhausted after compaction` / `session history compaction failed` / `compaction failed` / `compact failed` / 任意 `compaction` / `compact` | 由 `TalosError::ContextCompactionFailed` 或等价 typed error 显式产生 |
| `ContextWindowExceeded` | error string contains `context budget` / `prompt budget` / `context window` | 由 prompt / working context budget guard 显式返回 `TalosError::ContextWindowExceeded` |
| `ProviderUnavailable` | error string starts with `runtime unavailable:` / `transport unavailable:` / `unsupported model:` | 由 `TalosError::{RuntimeUnavailable, TransportUnavailable, UnsupportedModel}` 显式映射 |
| `PermissionDenied` | error string starts with `authorization denied` / `authentication failed:` / `auth error:` | 由 `TalosError::{AuthorizationDenied, AuthenticationFailed, AuthError}` 显式映射 |
| `ToolFailure` | error string starts with `tool error:` / `tool not found:` | 由 `TalosError::{ToolError, ToolNotFound}` 显式映射；普通 tool result failure 不自动升级为 turn failure |
| `InternalError` | error string starts with `internal error:` | 由 `TalosError::InternalError` 显式映射 |
| `UsageLimitExceeded` | 当前无可靠触发源 | 保留 reserved；等 usage budget policy 实现后再启用 |
| `SessionBudgetExceeded` | 当前无可靠触发源 | 保留 reserved；等 session budget policy 实现后再启用 |

### Tool Failure 猜测映射

改造前位置：`talos-rs/crates/action/tools/src/output.rs::classify_error_kind`

必须梳理但不一定首阶段全部迁移的猜测：

| 当前 kind | 当前触发方式 | 目标迁移方式 |
| --- | --- | --- |
| `ValidationFailure` | string contains `validation_failure` / `invalid` / `required` | 优先使用 structured tool error payload 的 `error_kind` |
| `PermissionRejection` | string contains `permission_rejection` / `authorization denied` / `permission denied` | 由 tool governance / permission layer 显式写入 `error_kind` |
| `GovernanceRejection` | string contains `governance` | 由 policy layer 显式写入 `error_kind` |
| `StaleWriteFailure` | string contains `stale` | 由 write guard 显式写入 `error_kind` |
| `DeferredToolUnavailable` | string contains `deferred` / `not yet visible` | 由 deferred tool loader 显式写入 `error_kind` |
| `RuntimeFailure` | fallback | 允许作为 fallback，但不能再承载可识别业务错误 |

## 阶段 1: 现状盘点与目标边界

**目标**: 完整列出所有基于字符串猜测错误 code / kind 的位置，并确认哪些属于本轮必须迁移，哪些作为后续工具输出规范优化。

**成功标准**:
- `classify_turn_error(&str)` 的所有分支已在计划和 TODO 中逐项登记。
- `TalosToolErrorKind` 的字符串 fallback 已登记为二级迁移对象。
- 明确本轮不引入 `TalosErrorCategory`、registry、动态 code、全局 trait。
- 明确 `UsageLimitExceeded` / `SessionBudgetExceeded` 本轮不启用，除非真实 budget policy 已存在。

**测试**:
- `rg "classify_turn_error|classify_error_kind|contains\\(|starts_with\\(" talos-rs/crates -S`
- 不需要运行编译；本阶段是只读扫描。

**状态**: 已完成

## 阶段 2: `talos-error` 定义统一 code 与显式 mapper

**目标**: 在 `talos-error` 中建立 error code 的单一来源，并给现有 `TalosError` 增加 Codex 风格的显式映射方法。

**成功标准**:
- `TalosRunErrorCode` 的 owner 从普通公共 DTO 迁移到 `talos-error` 或由 `talos-error` 显式 re-export / drive。
- `TalosError` 增加最小必要领域 variant：
  - `ContextWindowExceeded(String)`
  - `ContextCompactionFailed(String)`
- `TalosError` 增加显式 mapper：
  - `to_run_error_info(&self) -> Option<TalosRunErrorInfo>`
- `RuntimeUnavailable`、`TransportUnavailable`、`UnsupportedModel` 映射为 `ProviderUnavailable`。
- `AuthorizationDenied`、`AuthenticationFailed`、`AuthError` 映射为 `PermissionDenied`。
- `ToolError`、`ToolNotFound` 映射为 `ToolFailure`。
- `InternalError` 映射为 `InternalError`。
- `UsageLimitExceeded`、`SessionBudgetExceeded` 不从任何现有字符串或泛化错误映射。

**测试**:
- `just test -p talos-error`
- 新增 mapper 单测，覆盖所有确定映射和 reserved code 不被误用。

**状态**: 已完成

## 阶段 3: Kernel 产生 typed run errors

**目标**: 让 context window 和 compaction 失败在产生处返回 typed `TalosError`，不再依赖上层从 message 猜测。

**成功标准**:
- working context budget guard 超限返回 `TalosError::ContextWindowExceeded`。
- session history compaction 不可恢复失败返回 `TalosError::ContextCompactionFailed` 或保留原始 error 后显式包裹为该 variant。
- prompt rebuild + deterministic trim 后仍超预算返回 `ContextWindowExceeded`。
- compaction helper provider failure 和 compaction pipeline internal failure 不混用；只有真正不可恢复的 compaction pipeline failure 映射为 `ContextCompactionFailed`。
- 错误 message 只用于人类阅读，不参与 code 判断。

**测试**:
- `just test -p talos-agent-kernel context`
- `just test -p talos-agent-kernel compaction`
- 修改现有 timeline / compaction 测试，断言 `error_info.code`，不再断言文案片段。

**状态**: 已完成

## 阶段 4: Timeline API 移除字符串分类

**目标**: 删除 `classify_turn_error(&str)`，让 `TimelineBuilder` 接收结构化错误信息。

**成功标准**:
- 删除 `classify_turn_error(&str)` 及其基于 `contains` / `starts_with` 的分支。
- `TimelineBuilder::fail_turn` 调整为以下之一：
  - `fail_turn(error: &TalosError)`
  - `fail_turn_with_info(message: String, error_info: Option<TalosRunErrorInfo>)`
- 所有调用点迁移，不再传 `err.to_string()` 让 timeline 自行分类。
- 仍允许纯 message error event，但必须显式 `error_info = None`，不能猜测。
- `TalosTurnErrorEvent.error` 保留为展示文案；`error_info` 由 `TalosError::to_run_error_info()` 或调用方显式传入。

**测试**:
- `just test -p talos-agent-kernel timeline`
- `rg "classify_turn_error|contains\\(\\\"compact|starts_with\\(\\\"runtime unavailable" talos-rs -S` 无生产残留。

**状态**: 已完成

## 阶段 5: AgentRuntime / Coordinator / Service 调用链迁移

**目标**: 把 Runtime、Coordinator、Service 中所有 `timeline.fail_turn(err.to_string())` 迁移到结构化入口，避免跨层丢失 error code。

**成功标准**:
- `talos-rs/crates/orchestration/agent/src/agent/runtime.rs` 中所有 run failure 路径传递 `TalosError` 或显式 `TalosRunErrorInfo`。
- `talos-rs/crates/orchestration/agent/src/agent.rs` 中 runtime start / activation failure 不再用字符串分类。
- `talos-rs/crates/orchestration/service/src/turn/service.rs` 的 synthetic timeout / first-event error 明确是否有 code；不能留模糊猜测。
- `AgentError` 到 `TalosError` 的转换边界明确；无法准确映射的保持 `error_info = None` 或 `InternalError`，不猜 provider / tool / permission。
- active turn failed、timeline recovery、SSE subscription 行为不变。

**测试**:
- `just test -p talos-agent`
- `just test -p talos-service`
- timeline subscription / recovery 相关 scoped tests。

**状态**: 已完成

## 阶段 6: 协议、SDK 与生成产物收敛

**目标**: 保持对外 wire 类型稳定，但让 schema / TS 的 error code 来源和 Rust error 模块一致。

**成功标准**:
- `TalosRunErrorCode` 的 Rust 定义只有一个 source-of-truth；如需在 `talos-types` 暴露，必须显式从 error 模块复用或镜像生成，不维护两套手写 enum。
- `TalosRunErrorInfo` wire shape 不变：
  - `code`
  - `message`
- schema 和 TS generated artifacts 更新。
- Web / TUI 只消费 `error_info.code`，不解析 `error` 文案。
- SDK 类型继续暴露稳定 public DTO，不暴露内部 `TalosError`。

**测试**:
- `just gen-app-protocol`
- `just test -p talos-app-protocol`
- `pnpm --filter talos-web typecheck`
- 如 TS generated diff 出现新 code，必须确认对应 Rust source-of-truth 已存在。

**状态**: 已完成

## 阶段 7: Tool Failure 结构化输出收敛

**目标**: 收敛 `TalosToolErrorKind` 的字符串 fallback，让工具失败优先走 structured payload，不再依赖错误文案关键词。

**成功标准**:
- tool governance / permission / validation / deferred loader / stale write guard 产生 tool error 时显式写入 `error_kind` 或 `[error_kind=...]` tag。
- 删除 `classify_error_kind(error: &str)` 自然语言关键词分类。
- 未结构化 tool error message 默认归类为 `RuntimeFailure`，不再从 `required`、`permission denied`、`not yet visible` 等文案推断 kind。
- 不把普通 tool result failure 自动映射为 turn-level `ToolFailure`；只有 tool failure 导致 run fail 时才产生 run error code。
- 删除或降级针对关键词分类的测试，新增 structured payload / unstructured fallback 测试。

**测试**:
- `just test -p talos-tools`
- `rg "classify_error_kind|normalized\\.contains\\(" talos-rs/crates/action/tools/src/output.rs -S` 无命中。

**状态**: 已完成

## 阶段 8: 全局残留扫描与完整验证

**目标**: 确认统一 error code 规范落地后，没有旧字符串分类、重复 enum、协议漂移或 UI 解析文案残留。

**成功标准**:
- `classify_turn_error` 已删除。
- turn-level `TalosRunErrorCode` 不再由 message substring / prefix 推导。
- `UsageLimitExceeded`、`SessionBudgetExceeded` 没有被未实现策略误触发。
- `error_info` 在 timeline live、replay、recovery 中一致。
- Web / TUI 不再从 `error` string 解析状态。
- Spec / Plan 同步更新，说明 error code owner 是 `talos-error`。

**测试**:
- `just fmt`
- `just test -p talos-error`
- `just test -p talos-agent-kernel`
- `just test -p talos-agent`
- `just test -p talos-service`
- `just test -p talos-app-protocol`
- `pnpm --filter talos-web typecheck`
- `rg "classify_turn_error|contains\\(.*error|starts_with\\(.*error|error_info: None" talos-rs talos-app -S` 人工复核。
- 如改动影响 gateway / service 事件流，再按风险补跑相关 E2E case；完整 E2E 只在所有局部验证通过后运行一次。

**本地验证记录**:
- 已执行 `just fmt`。
- 已执行并通过 `just test -p talos-error`。
- 已执行并通过 `just test -p talos-types`。
- 已执行并通过 `just test -p talos-agent-kernel`。
- 已执行并通过 `just test -p talos-agent`。
- 已执行并通过 `just test -p talos-service`。
- 已执行并通过 `just test -p talos-tools`。
- 已执行 `just gen-app-protocol`。
- 已执行并通过 `just test -p talos-app-protocol`。
- 已执行并通过 `pnpm --filter talos-web typecheck`。
- 已执行并通过 `just fmt-check`。
- 已执行残留扫描；剩余 `normalized.contains` 命中属于工具选择和 bash 命令风险检测，不属于 error code / failure kind 映射。
- 后续扩展阶段再次执行并通过：
  - `just test -p talos-skills`
  - `just test -p talos-plugin`
  - `just test -p talos-gateway-http`
  - `just test -p talos-sdk`
  - `just test -p talos-tui`
  - `pnpm --filter @talos/sdk-core test -- adapters.test.ts`
- 已将 `SkillLoadError.kind`、`SkillWarning.code` 类型化，并执行 `just test -p talos-skills` / `just test -p talos-plugin` / `just test -p talos-app-protocol` 复核。

**状态**: 已完成

## 阶段 9: HTTP API error code typed owner 收敛

**目标**: 让 gateway/http 的 REST error code 不再由 handler 里散落字符串或 message prefix 生成。

**成功标准**:
- gateway/http 拥有明确的 HTTP API error code owner，例如 `HttpApiErrorCode` 或 resource-scoped typed code。
- `ApiError::new` 的 code 参数优先来自 typed code，而不是裸字符串。
- `fs_code(message, fallback)` 被删除；FS service 到 HTTP handler 的错误必须携带可结构化映射的信息。
- messages、events、sessions、projects、memory_files、workspace_docs 等 handler 的 code 常量收敛到统一位置或 resource-local enum。
- HTTP wire shape 保持 `{ code: string, message: string }`，不把 HTTP code 搬到 `TalosRunErrorCode`。

**测试**:
- `just test -p talos-gateway-http`
- HTTP handler scoped tests，覆盖 FS path_outside_root、event cursor expired、active turn running、idempotency conflict。
- `rg "starts_with\\(\\\".*:\\\"\\)|fs_code\\(|ApiError::new\\([^\\n]*\\\"[a-z_]+\\\"" talos-rs/crates/gateway/http/src -S` 人工复核。

**状态**: 已完成

## 阶段 10: Agent / Kernel 局部字符串错误分支收敛

**目标**: 删除 Agent 和 Kernel 里剩余会影响控制流的错误文案判断。

**成功标准**:
- `map_dynamic_input_error` 不再通过 `ToolError` message 判断 timeout。
- dynamic input timeout / expired / rejected 链路产生 typed `TalosError` 或 `AgentErrorKind`。
- `is_recoverable_compaction_parse_error` 改为 typed parser failure kind，不再匹配 parse error 文案。
- compaction parse retry、recoverable parse failure、hard failure 的测试断言 typed kind。

**测试**:
- `just test -p talos-agent`
- `just test -p talos-agent-kernel compaction`
- `rg "contains\\(\\\"timed out\\\"\\)|parse memory summary json after|parse memory persistence decision json after" talos-rs/crates/orchestration -S` 人工复核。

**状态**: 已完成

## 阶段 11: Tool / MCP error kind 继续结构化

**目标**: 让 tool、MCP、governance、sandbox 的失败信息都能在 tool result 层携带明确 kind，避免继续退化成泛化 `ToolError(String)`。

**成功标准**:
- governance forbidden 显式映射为 `TalosToolErrorKind::GovernanceRejection`。
- permission rejection 显式映射为 `TalosToolErrorKind::PermissionRejection`。
- deferred tool unavailable、stale write、validation failure 均从结构化 payload 或 typed error 进入 `ToolResultError`。
- MCP service error 至少保留 source/kind，不只拼接 `"MCP service error: {error}"`。
- 未结构化外部工具失败仍允许 fallback 到 `RuntimeFailure`，但不再猜测自然语言。

**测试**:
- `just test -p talos-tools`
- `just test -p talos-mcp`
- `just test -p talos-tools-sandbox`
- `rg "from_message\\(|ToolError\\(format!\\(\\\"MCP service error" talos-rs/crates/action -S` 人工复核。

**状态**: 已完成

## 阶段 12: Client error consumption 收敛

**目标**: Web / TUI / SDK 不再通过错误 message 判断业务状态。

**成功标准**:
- TUI active turn conflict 判断不再依赖 `"turn is already active"` 文案。
- SDK `map_app_server_error` 只按 numeric code 映射；如业务需要更细分状态，先在 app-protocol 增加明确 code。
- Web / TUI 只使用 `error_info.code`、app-server numeric code、HTTP code 或 event type 做分支。
- message 只用于展示和日志。

**测试**:
- `just test -p talos-tui`
- `pnpm --filter talos-web typecheck`
- `rg "message\\.contains|error\\.message\\.includes|contains\\(\\\"turn is already active\\\"\\)" talos-rs/apps talos-app -S` 人工复核。

**状态**: 已完成

## 阶段 13: 全仓库异常处理最终扫描

**目标**: 确认所有错误分类都按模块 owner 收敛，没有跨层重复 code、字符串猜测和半成品 API。

**成功标准**:
- `TalosRunErrorCode` 只用于 run/turn failure event。
- HTTP REST code、JSON-RPC numeric code、TalosToolErrorKind、AgentErrorKind 都有独立 owner 和明确边界。
- Service / Store 错误不泄露 DB record，也不生成协议专用 code。
- UI / SDK 不解析 message。
- 所有新增 error enum、mapper、factory 都有英文 doc comment，说明 owner、职责和禁止承载的跨层语义。

**测试**:
- `just fmt`
- 受影响 Rust crate scoped tests。
- `just gen-app-protocol`，如果 app-protocol code 变化。
- `pnpm --filter talos-web typecheck`
- `git diff --check`
- 最终人工扫描：
  - `rg "classify_.*error|contains\\(.*error|starts_with\\(.*error|error\\.message\\.includes|to_string\\(\\)\\.contains" talos-rs talos-app -S`
  - 逐项剔除测试文案、搜索、路径、UI 渲染等非错误分类命中。

**本地验证记录**:
- 已执行最终残留扫描；旧分类函数、FS prefix 解析、tool error tag 解析、Web `error.message.includes` 均无命中。
- 剩余 `message.contains` / `to_string().contains` 命中均在测试中，用于文案或行为断言，不参与生产错误分类。
- `skill_not_found` / `ambiguous_skill_name` / `skill_injection_failed` 只存在于 `SkillWarningCode` 的序列化定义和生成 TS union 中，不再作为构造点裸字符串散落。
- 已执行 `git diff --check`。

**状态**: 已完成
