# Phase 0 迁移盘点

**状态**: 已完成  
**记录时间**: 2026-04-29

本文记录 Phase 1 抽取 `talos-application-service` 时可引用的现有模块与迁移候选点。

## Coordinator 迁移候选

| 文件 | 当前职责 | Phase 1 目标归属 |
|---|---|---|
| `talos-rs/crates/orchestration/coordinator/src/trait.rs` | `Coordinator` 继承 `RequestHandler + SessionService`，把 agent 协调和 application API 绑在一起 | 拆出新的 `AgentCoordinator` 边界；新增入口不再依赖旧 `SessionService` / `RequestHandler` |
| `talos-rs/crates/orchestration/coordinator/src/coordinator.rs` | 路由 agent、执行 turn、stream mapping、cancel，同时实现 session/project/approval 操作 | 保留 agent registry、routing、turn execution、stream bridge、cancel；session/project/approval 上移 |
| `talos-rs/crates/orchestration/coordinator/src/session.rs` | session cache、effective root、current directory 校验、project 校验、session title、progress 记录、project CRUD | 不新增仅转发的 session/project application service；共享访问与绑定策略进 `ResourceAccessService`，真实 lifecycle policy 继续从 coordinator/session manager 逐步上移 |
| `talos-rs/crates/orchestration/coordinator/src/stream.rs` | `OrchestratorEvent` 到 `TalosEvent` 的映射 | Phase 1 可先保留；后续作为 `RawAgentEvent` 到 `ApplicationEvent` 转换的参考 |
| `talos-rs/crates/orchestration/coordinator/src/maintenance.rs` | memory flush drain/pending 查询 | 保留为 coordinator/agent 后台任务能力；talos-server shutdown 仍可调用 |

## HTTP Gateway 迁移候选

| 文件 | 当前职责 | Phase 1 目标归属 |
|---|---|---|
| `talos-rs/crates/gateway/http/src/server/access.rs` | `ResourceAccessService` 封装 `SessionService`，处理 actor 过滤、admin guard、session/project visibility、approval 访问检查 | 迁入 `talos-application-service::access`，输入从 `RequestIdentity` 收敛到共享强类型 actor |
| `talos-rs/crates/gateway/http/src/server/state.rs` | `HttpApiState` 同时持有 gateway、access、event、idempotency、config、auth、models、file locks | 后续改为持有 `TalosApplicationService` 与 HTTP 专属依赖；共享 service 逻辑下沉到 application-service |
| `talos-rs/crates/gateway/http/src/server/messages.rs` | HTTP message ack、idempotency、active turn retry 校验、first event timeout、事件持久化和 forwarder | `TurnService` 应接管 turn start、active turn guard、idempotency、first event 持久化、event forwarder；HTTP 保留 DTO 与状态码映射 |
| `talos-rs/crates/gateway/http/src/server/events.rs` | SSE subscribe、replay cursor、active turn query、SSE frame mapping | application-service/event service 提供 replay 与订阅流；HTTP 保留 SSE encoding 与 header/query 映射 |
| `talos-rs/crates/gateway/http/src/server/sessions.rs` | session CRUD DTO mapping，部分 model/current directory 校验 | HTTP 保留 REST DTO 与响应映射；共享访问、project binding、model 校验进入 application-service |
| `talos-rs/crates/gateway/http/src/server/projects.rs` | project CRUD HTTP handler | HTTP 保留 REST DTO 与响应映射；共享 project visibility 与 CRUD 边界进入 `ResourceAccessService` |
| `talos-rs/crates/gateway/http/src/server/memory_files.rs` | memory file list/read/write/search，含 file lock | 共享语义迁入 `MemoryService`；file lock 作为 application 层能力 |
| `talos-rs/crates/gateway/http/src/server/workspace_docs.rs` | workspace docs list/read/write | 共享语义迁入 `WorkspaceDocService` |
| `talos-rs/crates/gateway/http/src/server/admin_users.rs` / `user_profile.rs` | admin/user account HTTP handler | 共享 account/admin resource policy 迁入 application-service；HTTP 保留 auth token 与 DTO |

## 当前调用路径

| 路径 | 当前调用链 | Phase 1 调整方向 |
|---|---|---|
| HTTP message | `messages.rs` -> `GatewayImpl` -> `RequestHandler::handle_stream` -> `CoordinatorImpl` -> `AgentOrchestrator` | `messages.rs` -> `TalosApplicationService::start_turn` -> `AgentCoordinator` |
| HTTP session/project/access | HTTP handler -> `ResourceAccessService` -> `SessionService` -> `CoordinatorImpl` / `SessionManager` | HTTP handler -> `TalosApplicationService` -> access/session/project 子 service |
| talos-server composition | `CoordinatorImpl` 同时注入为 `RequestHandler` 与 `SessionService` | `CoordinatorImpl` 注入为 agent 协调依赖；HTTP 注入 `TalosApplicationService` |
| SDK/CLI | Rust SDK 与 CLI 当前均通过 HTTP API/SSE 访问服务端 | Phase 4/5 再接入本地 app-server transport；Phase 1 不改 SDK/CLI 行为 |

## Session Store / Event 候选

| 文件 | 当前职责 | Phase 1 目标归属 |
|---|---|---|
| `talos-rs/crates/orchestration/session-store/src/event_service.rs` | `SessionEventService` 提供 event append、live broadcast、replay、active turn 更新、GC | Phase 1 复用为 application event service 的持久化基础；需补齐 persistence before fan-out 测试 |
| `talos-rs/crates/orchestration/session-store/src/events.rs` | `SessionEventStore`、event id、event seq、event replay 与 GC | 保留为 infra/session-store；application-service 通过强类型接口使用 |
| `talos-rs/crates/orchestration/session-store/src/active_turn.rs` | active turn 状态、pending user action、store trait | Phase 1 可复用；每 session 单 active turn guard 需在 application service 层显式实现 |
| `talos-rs/crates/orchestration/session-store/src/idempotency.rs` | message idempotency reservation/complete | Turn application service 迁移 HTTP message command 时复用 |

## Composition Root 候选

| 文件 | 当前职责 | Phase 1 目标归属 |
|---|---|---|
| `talos-rs/crates/app/talos-server/src/lib.rs` | 创建 storage、session store、event service、auth、router、agent、coordinator、gateway、HTTP server，并把 coordinator 同时注入为 `RequestHandler` 和 `SessionService` | 新增 `TalosApplicationService` 组装；HTTP 注入 application-service；coordinator 只作为 agent 协调依赖 |

## Frontend Client 候选

| 文件 | 当前职责 | 后续 Phase 目标归属 |
|---|---|---|
| `talos-app/ts-sdk/packages/core/src/client.ts` | 当前 `TalosClient` 直接围绕 HTTP 路径组织 client API | Phase 5 抽象 transport 无关 client/domain model |
| `talos-app/ts-sdk/packages/core/src/types.ts` | HTTP/Web UI 已使用的领域类型、event record、session/project/model/config 类型 | Phase 5 复用并与 app-protocol 生成类型对齐 |
| `talos-app/ts-sdk/packages/core/src/adapters.ts` | `HttpAdapter` / `StreamAdapter` | Phase 5 继续作为 browser transport 基础，新增 node/desktop transport 时不污染 core |
| `talos-app/ts-sdk/packages/browser/src/index.ts` | HTTP + SSE browser adapter | 保留为 Web runtime transport |
