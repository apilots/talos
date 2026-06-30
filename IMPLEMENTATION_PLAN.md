## 阶段 1: API Concept Audit Baseline
**目标**: 建立本轮 API 概念对齐的可执行审计基线，确认哪些问题本轮修复、哪些只记录为后续 Agent API TODO。
**成功标准**: 已形成审计清单，覆盖 `MemoryFile`、`MemorySearch`、`TalosContext`、`TimelineRecovery`、`DynamicToolResult`、`Workspace`、`Record`；每项都有目标命名、当前入口、迁移判断和是否本轮实施的结论；`workspace_id`、`default_workspace_id`、`AgentWorkspaceInfo`、`project_root`、`current_directory` 使用点已被列出并标注是否存在误用。
**测试**: 静态审查 Rust / TS API surface；使用 `rg` 检查相关类型、方法、字段和导出点；不改生产代码。
**状态**: 已完成

## 阶段 2: Memory API Alignment
**目标**: 统一 memory file 和 memory search 的 public API 语义，消除 Browser / Desktop / app-server SDK 行为漂移。
**成功标准**: 产品级 memory file API 在 Browser SDK、Desktop adapter、TS app-server SDK、Rust SDK 和 Gateway 统一为 date 级 request object；path 级访问只允许作为 service / store 内部 helper，不进入 protocol schema 或 SDK public API；HTTP date -> `memory/{date}.md` 转换只存在于 HTTP handler / service 内部边界；SDK、HTTP、JSON-RPC public search request 统一使用 `query`，不保留 `q` alias；Desktop adapter 完整传递 `query`、`limit`、`date_from`、`date_to`；仓库内旧调用方全部迁移到目标 API。
**测试**: TS SDK tests、Desktop adapter tests、HTTP gateway memory handler tests；覆盖 Browser 和 Desktop 对同一 date/search request 生成等价 Gateway request。
**状态**: 已完成

## 阶段 3: Context And Event API Alignment
**目标**: 分别收敛 Context、TimelineRecovery、DynamicToolResult 三个跨层 API 概念。
**成功标准**: `TalosContext` 是对外唯一 turn/session context；Rust SDK facade 和 app-protocol generated schema 不再导出 `ProtocolTalosContext`；若运行时内部确实需要轻量 context，只能作为 crate-private/internal 类型存在；timeline recovery 的 JSON-RPC method、HTTP route、SDK 方法、类型和文档表达为恢复 timeline snapshot，而不是恢复 session record；dynamic tool result 的 Request / Response 类型名和 Rust / TS / Gateway / Service / Coordinator / Agent 方法名保持一致；仓库内旧方法、旧类型和旧 public export 全部删除或迁移，不保留 deprecated wrapper。
**测试**: Rust app-protocol decode tests、schema generation diff review、TS app-server SDK tests、Rust SDK contract tests；用静态搜索确认 public exports 和 method names 不再暴露误导性旧概念。
**状态**: 已完成

## 阶段 4: Workspace Semantics Review
**目标**: 验证并记录 Workspace / Project / Directory 当前语义和目标语义，避免把 `workspace_id` 当作 path、project 或已解析 workspace dir。
**成功标准**: 已审查所有 `workspace_id`、`default_workspace_id`、`AgentWorkspaceInfo`、`project_root`、`current_directory` 使用点；本轮完成 `workspace_id` 作为 StartTurnRequest / SessionContext key 的贯通和持久化语义修正，并注入 agent invocation context；完整 agent selection routing 与 `workspace_id/default_workspace_id -> workspace config -> workspace dir` 映射明确记录为后续 Agent workspace routing TODO，不标记为本轮已实现；文档明确当前 execution root 优先级为 `project.directory > embedded current_directory > effective workspace root`，当前 Agent workspace dir 解析仍为 `agent.kernel.workspace_dir -> users/{user_id}`。
**测试**: 静态搜索和必要的 Rust unit tests；如修改行为，补充 session root resolution / workspace info tests。
**状态**: 已完成

## 阶段 5: Service And Store Boundary Cleanup
**目标**: 检查并收紧 Service public API 与 Store Record 的边界，避免 persistence record 外泄到 Gateway / SDK / UI。
**成功标准**: `SessionRecord`、`ProjectRecord`、`ActiveTurnRecord`、`IdempotencyRecord` 只属于 store / repository 边界；它们不出现在 app-protocol schema、TS generated types、SDK facade、Web UI imports 或 Gateway response 中；跨模块 service-facing API 返回 domain DTO 或明确为内部 helper；Record -> DTO mapper 集中在 Service 层。
**测试**: Rust service/store tests、gateway core tests；使用 `rg` 检查具体 Record 名称的导出和跨层引用。
**状态**: 已完成

## 阶段 6: Schema, Call Site, And Documentation Sync
**目标**: 完成本轮 API 概念对齐的 schema 生成、调用方迁移和文档收尾。
**成功标准**: HTTP API、JSON-RPC method、Rust SDK、TS SDK、Desktop adapter、Web UI、schema fixture、README / 示例全部更新到目标 API；仓库内不再保留旧 public SDK 方法、REST alias、JSON-RPC legacy method、deprecated wrapper 或误导性类型导出；已运行项目 schema 生成命令并检查 generated diff；breaking change 以目标态记录，不设计兼容层；Agent 统一 API 作为独立 TODO 保留，不混入本轮实现。
**测试**: schema generation、TS SDK tests、Rust SDK / gateway tests、必要的 lint / typecheck；文档审查确认 spec 与 plan 无漂移。
**状态**: 已完成
