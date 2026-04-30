# Phase 1: Application Service Extraction

**状态**: 已完成  
**目标**: 新增 `talos-application-service`，迁移共享应用语义，并把 `coordinator` 收缩为下层 agent 协调组件。  
**依赖**: Phase 0 完成。  
**输出**: HTTP handler 通过 `TalosApplicationService` 进入共享路径，`coordinator` 不再承载 resource access / session application policy。

---

## 1. 范围

包含：

- 新增 `crates/orchestration/application-service`。
- 迁移 `ResourceAccessService` 到 application-service。
- 建立 session、project、turn、event、config、memory、workspace docs、account、admin resource 子 service 边界。
- 定义 `TalosApplicationApi` 与 `AgentCoordinator` 边界。
- 让 HTTP session / project / message / approval handler 逐步调用 application-service。
- 保持 `talos-server` 为 composition root。

不包含：

- 实现 JSON-RPC app-server。
- 修改 Desktop UI。
- 引入新的前端 transport。

---

## 2. 子任务

执行顺序必须按以下依赖推进：

```text
1.1 crate skeleton
  -> 1.2 access service
  -> 1.3 session/project application services
  -> 1.4 application event service
  -> 1.5 turn application service
  -> 1.6 HTTP handler migration
  -> 1.7 coordinator final slimming
  -> 1.8 composition root cleanup
```

说明：coordinator 职责盘点可以提前做，但最终瘦身要等 HTTP handler 迁入 application-service 后再做，避免 HTTP 仍依赖旧 coordinator trait 时提前拆断主链路。

### 1.1 新建 crate 与模块骨架

**任务**:

- [x] 新增 `crates/orchestration/application-service`。
- [x] 配置 workspace dependency。
- [x] 建立 `lib.rs` 与子模块：
  - [x] `access`
  - [x] `account`
  - [x] `admin`
  - [x] `session` / `project` 不新增 pass-through service，归入 `ResourceAccessService` + 现有 `SessionService` 边界
  - [x] `turn`
  - [x] `event`
  - [x] `config`
  - [x] `memory`
  - [x] `workspace_docs`
- [x] 定义 crate 级 error/result 类型复用策略。

**完成标准**:

- crate 可单独 `cargo test -p talos-application-service`。

### 1.2 抽取共享访问控制

**任务**:

- [x] 从 `gateway/http` 迁移 `ResourceAccessService`。
- [x] 去掉对 HTTP request/response 类型的依赖。
- [x] 输入统一为强类型 `Actor` 与 application params。
- [x] 保留 actor visibility、admin guard、session/project access 校验。
- [x] 增加迁移后单元测试。

**完成标准**:

- HTTP 与未来 app-server 能复用同一 access service。

### 1.3 建立 session / project application services

**任务**:

- [x] 从 `CoordinatorImpl` / `SessionManager` 盘点 session lifecycle 逻辑。
- [x] 确认不新增仅转发 `SessionService` 的 `SessionApplicationService` / `ProjectApplicationService`。
- [x] 迁移 actor scoping、session-project binding 访问校验、admin session/project 入口。
- [x] 迁移 `context.project_root` 禁用校验与 patch ack not-found 语义。
- [x] 迁移 effective root、project directory validation。
- [x] 迁移 session title 纯策略。
- [x] 迁移 session progress 纯策略。
- [x] 保持对现有 HTTP 行为的兼容测试。

**完成标准**:

- session / project 主路径可通过 application-service 调用。
- coordinator 中不再新增 session/project application policy。

### 1.4 建立 event service 规则

**任务**:

- [x] 定义 `ApplicationEvent` 强类型边界。
- [x] 定义应用层 replay/live 订阅接口。
- [x] 实现或整理 event id / turn id / session id 生成位置。
- [x] 确保 message stream 主链路 persistence before fan-out。
- [x] 支持按 last seen event id 的 replay 语义，但不在 Phase 1 固化 JSON-RPC DTO 字段名。
- [x] 不引入 `subscription_id`、`ApplicationEventEnvelope` wire DTO、`subscription.terminated` method 等协议层概念。

**完成标准**:

- application-service 能产出持久化后的 `ApplicationEventStream`。
- replay 基于持久化事件，而不是内存通知缓存。

### 1.5 建立 turn application service

**任务**:

- [x] 建立简洁命名的 `TurnService`。
- [x] 实现 retry/active turn 状态校验迁入 `TurnService`。
- [x] 将 message/turn start 应用入口连接到 gateway/coordinator 主链路。
- [x] 将 `turn.interrupt` 应用入口连接到共享 cancel 入口。
- [x] 将 agent stream first event 与后续 event 持久化后转发。
- [x] 保持 tool approval / dynamic tool result 的应用入口经共享 access service 进入。

**完成标准**:

- turn 主链路从 application-service 进入 coordinator。
- active turn conflict 可在 application-service 层返回。

### 1.6 HTTP handler 迁移

**任务**:

- [x] session handler 调用 application-service access boundary。
- [x] project handler 调用 application-service access boundary。
- [x] message / retry turn handler 调用 `TalosApplicationService` 用例入口。
- [x] approval handler 调用 application-service access boundary。
- [x] config/model/memory/workspace docs/account handler 迁移共享逻辑。
- [x] HTTP crate 只保留 route、auth middleware、DTO/header mapping、SSE encoding 与 HTTP 状态码适配。

**完成标准**:

- HTTP 路径仍通过现有回归。
- HTTP crate 不继续新增共享 service 逻辑。

### 1.7 拆分 coordinator 职责

**任务**:

- [x] 标出 `CoordinatorImpl` 中残留 session lifecycle 相关逻辑。
- [x] 标出残留 project / root / title / progress 相关逻辑。
- [x] 标出 turn execution 与 agent routing 相关逻辑。
- [x] 将 current directory / project directory 纯校验抽到 `talos-types` 复用。
- [x] 将 HTTP 生产路径的 session/project record CRUD/query policy 从 coordinator 迁入 application-service。
- [x] 将 coordinator legacy `SessionService` 中剩余 agent/session runtime 边界收缩为 runtime/control trait。
- [x] 保留 `coordinator` 的 agent registry、routing、turn execution、stream mapping、cancel、agent feedback bridge。
- [x] 定义 `RawAgentEvent` 输出边界。

**完成标准**:

- 新入口不依赖旧 `SessionService` / `RequestHandler` 作为 application API。
- `coordinator` 文件保持在项目单文件约束内。

### 1.8 Composition root 调整

**任务**:

- [x] 在 `talos-server` 中组装 application-service。
- [x] 明确 coordinator、event store、session store、config、auth、tool registry 的注入关系。
- [x] 保持 `talos-server` 不沉淀业务逻辑。

**完成标准**:

- `talos-server` 只负责组装，不承担 application policy。

---

## 3. 测试

本阶段重点新增测试：

- `ResourceAccessService` actor 过滤。
- `TalosApplicationService` 到 `AgentCoordinator` 的 turn 调用。
- event persistence before fan-out。
- application event stream replay。
- active turn conflict。

全量质量门禁见 [PLAN.md §5](PLAN.md#5-全局质量门禁)。

---

## 4. 风险

| 风险 | 处理 |
|---|---|
| Phase 1 过大 | 按 access/event/http handler/coordinator final slimming 顺序小步迁移 |
| coordinator 旧 trait 临时保留过久 | 在本计划记录清理点，Phase 6 必须删除 |
| HTTP 行为回归 | 每迁一个 handler 就跑对应 handler test |

---

## 5. 完成记录

**变更摘要**:  
已完成。已新增 `talos-application-service`，并迁移 access、event replay/live 订阅、turn submit/retry、file/version/lock、memory files、workspace docs、config/model catalog、account wrapper、admin session pagination、session/project HTTP 用例入口与强类型业务错误。HTTP handler 已瘦身为 auth/route/DTO/header/status/SSE 适配为主，不再持有 HTTP 本地 `ResourceAccessService` pass-through facade。生产组合根已通过 `TalosApplicationService::new_with_session_store` 注入 `SessionStore`，session/project record create/list/get/patch/delete/query policy 由 application-service 直接处理；coordinator 通过新的 `SessionRuntime` 边界只向 production application-service 暴露 target agent routing、cache invalidation 与 agent/session control 能力。application-service 记录读取侧已移除 session cache，避免 coordinator 写入 turn progress 后读取旧 session record；application-service 写 session/project 后会通过 `invalidate_cached_session` hook 失效 coordinator runtime cache。ID 生成边界已整理为：session id 由 session-store 创建，turn id/request id 由 `TurnService` 创建，event id 由 `SessionEventService` 持久化时创建。session title/progress 纯策略已下沉到 `talos-types`。

**关键文件**:  
- `talos-rs/crates/orchestration/application-service`
- `talos-rs/crates/common/types/src/session.rs`
- `talos-rs/crates/orchestration/coordinator/src/{coordinator,session,stream}.rs`
- `talos-rs/crates/app/talos-server/src/lib.rs`
- `talos-rs/crates/gateway/http/src/server/{access,events,messages,projects,sessions,memory_files,workspace_docs,misc,admin_users,user_profile,state}.rs`
- `talos-rs/crates/infra/error/src/lib.rs`

**测试结果**:  
- `cargo test -p talos-error` 通过，0 passed。
- `cargo test -p talos-application-service` 通过，18 passed。
- `cargo test -p talos-types` 通过，22 passed。
- `cargo test -p talos-coordinator` 通过，16 passed。
- `cargo test -p talos-gateway-http` 通过，52 passed。
- `cargo test -p talos-server` 通过，4 passed。
- `tests/integration/scenarios/03_messages_http_sse.sh` 修复后通过。
- `tests/integration/run_e2e.sh` 通过，报告：`talos-rs/tests/integration/reports/e2e_report_20260429_233827.md`。
- `cargo test -p talos-session-store` 通过，29 passed。
- `cargo test -p talos-coordinator` 通过，16 passed。
- `cargo test -p talos-gateway-http` 通过，52 passed。
- `cargo test -p talos-server` 通过，4 passed。

**遗留临时路径（Phase 6 待删）**:  
暂无登记为 Phase 6 待删的临时兼容路径。

**未覆盖风险**:  
- Phase 6/7 已完成 production path 收敛；coordinator legacy `SessionService` 不再作为 application API 暴露给 HTTP / JSON-RPC 协议层。测试侧仍保留 legacy adapter，用于覆盖 runtime/control 边界行为，不作为长期产品入口风险。
- `ApplicationEvent` 已命名为现有强类型 `TalosEventPayload` 的 application 边界别名；`RawAgentEvent` 已在 coordinator 侧命名为现有 `OrchestratorEvent` 的边界别名，当前 Phase 1 已保证 replay 和 persistence-before-forward 的共享规则。

**后续移交**:  
- Phase 6/7 已完成 production path 收敛；`CoordinatorImpl` 在服务端运行链路中只作为 agent runtime/control 边界继续被 application-service 组合使用。测试侧 legacy adapter 仅保留为单元测试支撑，不作为 application API。
