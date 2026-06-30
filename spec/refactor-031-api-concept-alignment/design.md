# API Concept Alignment Refactor Design

Status: implemented
Owner: agent
Created: 2026-06-29
Updated: 2026-06-30
Related: `refactor-027-core-api-object-boundary`, `refactor-028-orchestration-store-service-layout`, `refactor-030-tui-embedded-sdk-transport`

## 背景

Talos 当前已经形成多端产品形态：HTTP Web、Desktop app-server、Rust SDK、embedded runtime、TUI / CLI，以及后续可能扩展的 MCP / 多 Agent 管理。核心链路已经基本收敛到：

```text
SDK / Client
  -> Transport Adapter
  -> Gateway
  -> TalosService
  -> Store / Timeline / Coordinator
  -> Agent
```

但是在多个层之间仍有一些同一业务概念的命名、参数形态和边界不一致。它们会导致不同端需要写 adapter 特例，也会让未来 Agent routing、多 Agent 管理和 SDK public API 演进继续产生漂移。

本设计聚焦最明显、最容易产生跨端行为差异的概念对齐问题：

1. memory file 的 `date` / `path` 混用。
2. memory search 的 `q` / `query` 混用。
3. `TalosContext` / `ProtocolTalosContext` 双上下文对象。
4. timeline recovery / session recovery 命名不统一。
5. dynamic tool result 命名不统一。
6. workspace / project / current directory / default workspace 的边界说明不足。
7. Store `Record` 与 Service / Gateway DTO 边界需要继续收紧。
8. Agent 概念存在多处表达，但本轮只记录 TODO，不展开完整 Agent API 设计。

## 原始审查基线问题

### SDK / Gateway

1. Rust SDK `GatewayClient` 基本对齐 Gateway trait，并复用 `talos_types` 的 request / response 类型。
2. TS Browser SDK 使用 HTTP REST shape，对外提供 `TalosClient` 分组 API。
3. TS app-server SDK 使用 JSON-RPC method shape，对外提供 `AppServerClient` 方法。
4. Desktop adapter 把 Browser-facing `TalosWebClient` 形态映射到 app-server SDK。
5. Gateway Core 是协议无关业务 API 门面。

### Memory

1. Browser SDK 对外 memory file API 是 date 级：`memory.get(date)` / `memory.put(date, request)`。
2. HTTP handler 把 date 映射到 workspace memory file path：`memory/{date}.md`。
3. App-server SDK 对外 memory file API 是 path 级：`readMemoryFile(path)` / `writeMemoryFile(path, ...)`。
4. Desktop adapter 当前把 date 直接传给 app-server SDK 的 path 参数。
5. Rust / JSON-RPC domain request 使用 `MemorySearchRequest { query, limit, date_from, date_to }`。
6. Browser SDK 使用 `MemorySearchQuery { q, limit, date_from, date_to }`。
7. Desktop adapter 当前只桥接 `q` 和 `limit`，没有完整传递 date filter。

### Context

1. Turn / Session 真实业务对象使用完整 `TalosContext`。
2. 原始问题：app-protocol 曾生成 `ProtocolTalosContext`。
3. 原始问题：Rust SDK facade 曾 re-export `ProtocolTalosContext`。
4. 处理结果：对外 context API 收敛为 `TalosContext`，`ProtocolTalosContext` 不再进入 schema 或 SDK public surface。

### Workspace / Project / Directory

1. `workspace_id` 和 `default_workspace_id` 是为 Agent workspace routing / future multi-agent management 保留的设计点，不是 legacy 字段。
2. `workspace_id` 是 Agent workspace 的逻辑 key，不是 filesystem path；它挂在 turn / session / context 上，用于当前执行上下文和未来 Agent routing。
3. `default_workspace_id` 是 Agent 配置中的默认 workspace key；当请求或 session 未显式指定 `workspace_id` 时，应使用该默认 key。
4. 当前实现中，`workspace_id` 主要作为 context / routing key 传递，尚未完整参与 workspace dir 解析。
5. 目标设计中，`workspace_id` 应通过 Agent workspace 配置解析到对应 workspace dir。
6. `project_id` 表示用户管理的项目绑定。
7. `current_directory` 表示 embedded / local runtime 的当前工作目录。
8. `project_root` 是 session context 中已解析出的执行根目录。
9. `workspace_dir` / `AgentWorkspaceInfo` 表示已解析后的用户级 Agent workspace 文件区域。

### Agent

当前存在多处 Agent 相关表达：

1. `SessionAgentConf { model }`
2. `AgentDispatchRequest` / `AgentDispatchResponse`
3. runtime `AgentRequest` / `AgentResponse`
4. subagent tool 内部 request / response
5. coordinator `resolve_target_agent`

这些属于独立的 Agent 统一 API 设计问题。本 spec 只记录 TODO，不在本轮定义完整 Agent domain model。

## 目标

1. 消除最明显的跨端 API 概念不一致。
2. 对外 SDK public API 使用统一业务语义，不暴露 transport 或内部 path 特例。
3. Gateway / Rust SDK / TS app-server SDK / Browser SDK 对同一能力使用同一核心 request / response 概念。
4. HTTP REST path/query 差异只存在于 transport adapter 层。
5. Store `XxxRecord` 继续限制在 persistence / repository 边界内。
6. 保留 `workspace_id` / `default_workspace_id` 作为 Agent routing 扩展点，并明确命名语义。
7. Agent 统一 API 单独设计，本轮只留 TODO 和边界约束。

## 非目标

1. 不重写 Agent runtime / coordinator / kernel。
2. 不移除 `workspace_id` 或 `default_workspace_id`。
3. 不改变当前 Agent routing 的实际策略。
4. 不把 app-server 改成 MCP server。
5. 不引入新的 transport 协议。
6. 不做大范围前端 UI 改版。
7. 不在本 spec 中写执行阶段；执行阶段见根目录 `IMPLEMENTATION_PLAN.md`。

## 设计原则

1. **业务对象优先**：SDK / Gateway / Service 对同一业务能力共享 `XxxRequest` / `XxxResponse`。
2. **transport mapping 局部化**：HTTP path、JSON-RPC method、SSE event name 只在 transport adapter 层映射；业务字段名不为 transport 习惯保留别名。
3. **Record 不外泄**：`XxxRecord` 只属于 store / repository 边界。
4. **命名表达真实概念**：date、path、query、workspace、project、directory 不能互相代替。
5. **保留未来扩展点**：`workspace_id` 是 Agent workspace routing key，缺省时来自 `default_workspace_id`，不应按 legacy 字段删除。
6. **一次性目标态收敛**：本轮不保留旧 public SDK 方法、deprecated wrapper 或 REST alias；多端 SDK 和调用方随 API 一次性迁移。

## 目标概念模型

### MemoryFile

对外产品 API 表达为 date 级对象：

```text
MemoryFileDate
  -> date: yyyy-mm-dd
  -> maps internally to memory/{date}.md
```

目标对外 API：

```text
listMemoryFiles(request: MemoryFileListRequest)
readMemoryFile(request: MemoryFileReadRequest)
writeMemoryFile(request: MemoryFileWriteRequest)
searchMemory(request: MemorySearchRequest)
```

`MemoryFileReadRequest` / `MemoryFileWriteRequest` 的对外字段使用 `date`，不是 raw path。path 级文件访问只允许作为 service / store 内部 helper，不进入 Gateway、protocol schema 或 SDK public API。

```text
MemoryFileReadRequest { date }
MemoryFileWriteRequest { date, content }
```

约束：

1. Browser SDK、Desktop adapter、TS app-server SDK、Rust SDK 和 Gateway 的产品级 memory file API 必须统一为 date 级。
2. HTTP handler 可以从 URL date 构造 `memory/{date}.md`，但这个转换不能泄漏到 Desktop adapter、app-server SDK 或 Gateway trait。
3. app-server JSON-RPC 也接受 date 级 request；不保留 path 级 public method 或 wrapper。

### MemorySearch

统一业务字段为 `query`：

```text
MemorySearchRequest {
  query: String,
  limit: Option<usize>,
  date_from: Option<String>,
  date_to: Option<String>,
}
```

约束：

1. SDK public API 不使用 `q`。
2. HTTP REST query 参数也使用 `query`；不保留 `q` alias。
3. Desktop adapter 必须完整传递 `query`、`limit`、`date_from`、`date_to`。

### Context

对外只保留 `TalosContext` 作为 turn/session context。

约束：

1. SDK facade 不 re-export `ProtocolTalosContext`。
2. app-protocol 不生成 public `ProtocolTalosContext` schema；如运行时内部确实需要轻量结构，必须是 crate-private/internal 类型，不能进入 SDK 或 schema namespace。
3. `StartTurnRequest.context`、session record context、timeline context 统一使用 `TalosContext`。

### TimelineRecovery

统一概念为按 session 恢复 timeline snapshot。

建议命名：

```text
RecoverTimelineRequest
TimelineRecoveryResponse
recover_timeline
recoverTimeline
```

约束：

1. JSON-RPC method 使用 `timeline.recover`，避免把 timeline snapshot recovery 表达成 session record recovery。
2. SDK method 名称必须表达 timeline recovery，不应让调用方误解为恢复 session record。
3. HTTP route 使用 `/v1/sessions/:session_id/timeline/recovery`，文档说明返回的是 timeline recovery snapshot。

### DynamicToolResult

统一概念为提交等待用户动作的 dynamic tool result。

目标命名：

```text
SubmitDynamicToolResultRequest
SubmitDynamicToolResultResponse
submit_dynamic_tool_result
submitDynamicToolResult
```

约束：

1. Dynamic tool result 的对外对象必须使用 `SubmitDynamicToolResultRequest` / `SubmitDynamicToolResultResponse`。
2. protocol method 使用 `tool.dynamic_result.submit`；不保留泛化 tool result method 或类型别名。
3. Coordinator / Agent / Service / Gateway 命名保持一致。

### Workspace / Project / Directory

保留并明确以下概念：


| Concept | Meaning |
| --- | --- |
| `workspace_id` | Agent workspace 的逻辑 key；请求/session 可显式指定，未来支持多 Agent 管理和 routing。它不是 filesystem path。 |
| `default_workspace_id` | Agent 配置中的默认 workspace key；当请求/session 未显式指定 `workspace_id` 时使用。 |
| resolved workspace dir | 目标语义中，`workspace_id` 经 Agent workspace 配置解析后的实际 workspace directory。当前实现尚未完整建立该映射。 |
| `AgentWorkspaceInfo` | 已解析后的用户级 Agent workspace 文件区域信息。 |
| `WorkspaceDoc*` | Agent workspace 下的 workspace document。 |
| `project_id` | 用户创建和管理的 project 绑定。 |
| `project_root` | session 中已解析的 execution root。 |
| `current_directory` | embedded / local runtime 的当前目录输入。 |

当前 execution root 优先级：

```text
project.directory
  > embedded current_directory
  > effective workspace root
```

当前 Agent workspace dir 解析：

```text
agent.kernel.workspace_dir
  -> base_workspace_dir
  -> base_workspace_dir/users/{user_id}
  -> user_workspace_dir
```

目标 Agent workspace dir 解析：

```text
request.workspace_id
  or session.context.workspace_id
  or agent.kernel.default_workspace_id
  -> workspace config
  -> workspace dir
```

本轮落地状态：

1. 已完成：`workspace_id` 作为 `StartTurnRequest` / `SessionContext` key 贯通到 service、session record，并注入 agent invocation context。
2. 已完成：未显式绑定 workspace 的 session 在对外读取时使用 `agent.kernel.default_workspace_id` 作为动态默认 routing key。
3. 未完成且不属于本轮：`workspace_id` 参与 coordinator agent selection routing。
4. 未完成且不属于本轮：通过 workspace config 把 `workspace_id/default_workspace_id` 解析到具体 workspace dir。
5. 后续 Agent workspace routing spec 必须定义 workspace registry、routing selection、workspace dir resolution 和多 Agent 管理 API。

约束：

1. 不删除 `workspace_id`。
2. 不把 `workspace_id` 当作 project 或 raw filesystem path。
3. API 文档必须说明 `workspace_id` 是 Agent workspace logical routing key，不是 user workspace path、project path 或 current directory。
4. `workspace_id` 属于 `StartTurnRequest` 和 `SessionContext`，不属于来源侧 `TalosContext`；`TalosContext` 仍只描述请求来源和 trace 信息。
5. 新建 session 时显式 `workspace_id` 会写入 session context；既有 session 上的 `workspace_id` 是 per-turn routing override，不自动改写 session binding。
6. 未显式绑定 workspace 的 session 读取时使用 `agent.kernel.default_workspace_id` 作为动态默认 routing key；该默认值未来会通过 workspace config 解析到具体 workspace dir。
7. `AgentWorkspaceInfo` 表示当前已解析后的 workspace dir 信息，不应反向替代 `workspace_id`。
8. `AgentWorkspaceInfo` 不应被 TS SDK 重命名为含混的 workspace info 之外概念，也不应额外引入 facade 别名。

### Store Record Boundary

目标：

```text
Store API -> XxxRecord
Service API -> Domain DTO / XxxInfo / XxxResponse
Gateway API -> XxxRequest / XxxResponse
SDK API -> same XxxRequest / XxxResponse
```

约束：

1. `SessionRecord`、`ProjectRecord`、`ActiveTurnRecord`、`IdempotencyRecord` 不进入 SDK / protocol / UI。
2. Service 子模块如作为内部 helper 可以返回 record；若作为跨模块 public API，应返回 domain DTO。
3. Record 到 DTO 的 mapper 集中在 Service 层。

## Agent API TODO

后续需要单独 spec 设计统一 Agent API，至少覆盖：

1. Agent descriptor / capability / runtime spec。
2. 多 Agent registry 和 routing API。
3. `workspace_id` / `default_workspace_id` 如何参与 Agent workspace routing 和 workspace dir 解析。
4. Session agent config 与 runtime agent request 的边界。
5. Subagent tool request 与 runtime `AgentRequest` 的命名冲突。
6. Agent lifecycle、active turn、memory flush、dynamic tool result 的统一能力模型。

本 spec 不阻塞该 TODO，但本轮整改不能继续扩大 Agent 概念分裂。

## 影响范围

1. `talos-rs/crates/common/types`
2. `talos-rs/crates/gateway/app-protocol`
3. `talos-rs/crates/gateway/http`
4. `talos-rs/apps/sdk`
5. `talos-app/ts-sdk/packages/core`
6. `talos-app/web/src/shared/api/desktop-client.ts`
7. `talos-rs/crates/orchestration/service`
8. Store / DTO mapper 边界

## Cutover Strategy

1. 不保留 deprecated wrapper、legacy alias 或旧 public SDK 方法。
2. Rust SDK、TS Browser SDK、TS app-server SDK、Desktop adapter、Web UI 和 tests 在同一实施阶段迁移到目标 API。
3. HTTP REST、JSON-RPC method、Gateway trait、Service API、schema fixture 和 SDK 类型同步切换到目标命名。
4. 生成产物只能由项目 schema / type generation 命令生成，不手写 patch。
5. 仓库内调用方全部迁移后，用静态搜索确认旧 public 概念不再暴露。

## 验证策略

1. Rust unit tests 覆盖 type decode / mapper。
2. Gateway HTTP tests 覆盖 memory date -> path 转换和 search query mapping。
3. TS SDK tests 覆盖 Browser SDK 与 app-server SDK request shape。
4. Desktop adapter tests 覆盖 memory date、search filters、timeline recovery 和 dynamic tool result bridge。
5. Rust SDK contract tests 覆盖 AppServerClient / EmbeddedServerClient 行为一致性。
6. Schema generation 后检查 TS generated type diff。

## 风险

1. TS SDK 方法改名需要同步迁移 Web / Desktop 调用方，否则会出现编译期失败。
2. Memory path 级 API 降为内部 helper 时，必须补齐 date -> path 转换测试，避免路径语义散落。
3. `TalosContext` 收敛需要通过 schema diff 和 public export 搜索确认，防止双 context 对象回流。
4. Dynamic tool result 类型改名会牵涉 Gateway、Service、Coordinator、Agent、TS SDK 多层，实施时应拆小提交。
5. Agent API TODO 如果长期不处理，`workspace_id` 的 routing 语义仍可能继续被误用。
