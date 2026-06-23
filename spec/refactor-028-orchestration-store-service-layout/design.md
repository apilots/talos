# Orchestration Store / Service Layout 重构设计

Status: implemented
Owner: agent
Created: 2026-06-22
Updated: 2026-06-23
Depends on: `refactor-027-core-api-object-boundary`

## 背景

`refactor-027-core-api-object-boundary` 已经明确了对外对象、Gateway、Application Service、Coordinator、AgentHarness 和 Agent Kernel 的职责边界。但原先 `talos-rs/crates/orchestration` 下的 crate 布局比较平铺，例如 session store、project store、application-service 与 coordinator / agent 并列，目录结构没有直接表达 store 层与 service 层的职责差异。

随着 session、project、timeline、workspace docs、memory、plugin、skills、auth cache 等业务继续增长，如果持久化封装、业务编排和 Agent 执行协调继续散落在同一层级，后续会重新出现几类债务：

1. `XxxRecord` 容易被公共 types、Gateway、SDK 或 UI 误用。
2. DB、markdown、JSON state file 等持久化访问逻辑没有统一归属。
3. 某个业务需要组合多个持久化来源时，编排逻辑容易落到 Gateway、Transport 或 Agent 层。
4. `Record -> Info` 转换边界不稳定，导致服务端内部和客户端之间重复定义 DTO。
5. 新增业务模块时不知道应该放在 store、service、coordinator 还是 agent。

本 spec 定义 `orchestration/store` 与 `orchestration/service` 的目标目录和职责边界。本轮已落地 `talos-store` 与 `talos-service` 的拆分；`talos-application-service` 保留为 facade / access / composition 层。

## 目标

1. 在 `orchestration` 下建立清晰的 `store` 和 `service` 物理目录边界。
2. project/session DB 持久化统一放入 `orchestration/store` 这个 `talos-store` crate，并按 `project` / `session` 分模块。
3. session、timeline、history、turn 等领域应用服务放入 `orchestration/service` 这个 `talos-service` crate，并按业务场景拆模块。
4. `XxxRecord` 只存在于对应 store crate 内部或其明确公开的 store API 中。
5. service 层统一组合 Agent / Coordinator 执行能力和一个或多个 store API。
6. service 往上只返回 `talos_types` 中的 `XxxInfo` / `XxxRequest` / `XxxResponse`。
7. `Record -> Info` 转换统一放在 service 层，不在 Gateway、Transport、SDK 或 UI 中重复实现。
8. 让目录结构本身表达分层，降低新增业务模块时的理解成本。

## 非目标

- 不改变 `refactor-027` 已定义的对外 API 对象原则。
- 不把 Gateway 变重。
- 不让 store 直接调用 AgentCoordinator、AgentHarness 或 Gateway。
- 不让 service 直接处理 HTTP、JSON-RPC、SSE 或 stdio envelope。
- 不重写 Agent Kernel 或 AgentHarness 内部执行逻辑。
- 不把 `talos-application-service` 移入 `orchestration/service`；它继续作为 facade / access / composition 层。

## 目标目录结构

```text
talos-rs/crates/orchestration/
  store/
    Cargo.toml              # talos-store
    src/
      lib.rs
      project/
        mod.rs
        types.rs
        store.rs
        mock_store.rs
      session/
        mod.rs
        types.rs
        store.rs
        mock_store.rs
        events.rs
        active_turn.rs
        idempotency.rs

  service/
    Cargo.toml              # talos-service
    src/
      lib.rs
      session/
      timeline/
      history/
      turn/

  application-service/
  coordinator/
  agent/
  agent-kernel/
```

说明：

- `store` 是持久化访问层；当前由单一 `talos-store` crate 承载，并按业务域拆 `project` / `session` 子模块。
- `service` 是领域应用服务层；当前由单一 `talos-service` crate 承载，并按 session / timeline / history / turn 拆业务模块。
- `application-service` 是 facade / access / composition 层，负责权限、对 Gateway 的聚合门面和跨 service 组装。
- `coordinator`、`agent`、`agent-kernel` 保持执行链路职责，不承接 DB/file 持久化。

## Store 层职责

`orchestration/store` 统一封装 project/session 持久化读写。持久化介质包括但不限于：

- SQL database
- SQLite store
- markdown files
- JSON state files
- local cache files
- index files

每个 store 业务模块可以包含：

- `XxxRecord`
- `XxxStore`
- store-specific query/filter/update input
- storage schema migration
- file read/write safety logic
- serialization/deserialization logic
- persistence-level tests

Store 层允许做：

- 操作 DB / file / cache。
- 定义 `XxxRecord` 和持久化查询对象。
- 暴露面向 service 的 store API。
- 处理持久化层错误并附带上下文。

Store 层不允许做：

- 返回 SDK/protocol DTO 作为主对象。
- 调用 Gateway、HTTP、JSON-RPC、SDK 或 UI。
- 调用 AgentCoordinator、AgentHarness 或 Agent Kernel。
- 承担跨多个 store 的业务事务编排。
- 把 `XxxRecord` 暴露到 `talos_types`、SDK schema 或 Web shared types。

## Service 层职责

`orchestration/service/*-service` 统一封装应用业务能力。Service 层是 Agent 执行能力和持久化状态之间的融合层。

Service 层允许做：

- 调用一个或多个 store API。
- 调用 AgentCoordinator 或其他执行抽象。
- 承接权限、幂等、session 校验、timeline、active turn、title/progress 更新。
- 执行业务事务和状态流转。
- 完成 `XxxRecord -> XxxInfo` 转换。
- 对 Gateway 提供面向业务的 service API。

Service 层不允许做：

- 处理 HTTP path/query/header、JSON-RPC method/id、SSE framing。
- 定义协议 envelope。
- 把 DB/file schema 泄漏到 Gateway、Transport、SDK 或 UI。
- 在内部私自初始化全局配置、store 或 agent 依赖；依赖应由组装层传入。

## 对象边界

目标对象流：

```text
Store layer
  XxxRecord
    |
    | service mapping
    v
Service / Gateway public boundary
  XxxInfo
  XxxRequest
  XxxResponse
    |
    v
Transport / SDK / Web / Desktop / TUI
```

原则：

- `XxxRecord` 属于 store。
- `XxxInfo` 属于 `talos_types`。
- `XxxRequest` / `XxxResponse` 属于 `talos_types`。
- `Record -> Info` 转换属于 service。
- Gateway 只接收和返回 `talos_types` 对象。
- SDK 只复用 `talos_types` 生成对象，不重新定义客户端业务 DTO。

示例：

```text
talos_store::session::SessionRecord
  -> talos-service::session maps to talos_types::SessionInfo
  -> Gateway returns SessionGetResponse { session: SessionInfo }
  -> HTTP / JSON-RPC / SDK transport envelope
```

## 多 Store 组合

某些业务天然需要操作多个 store。例如 turn start 可能同时涉及：

- `talos_store::session`
- `talos_store::session::events`
- `talos_store::session::idempotency`
- `talos_store::project`
- `memory-store`

这类组合逻辑必须位于 service 层。Gateway 不直接协调多个 store，AgentCoordinator 不直接协调持久化，store 之间也不互相调用来完成业务事务。

目标形态：

```text
Gateway
  -> talos-service::turn
       -> talos-store::session
       -> talos-store::session::events
       -> talos-store::session::idempotency
       -> AgentCoordinator
       -> talos-store::session::events
       -> talos-store::session
```

## 命名规则

| 类型 | 命名 | 归属 |
| --- | --- | --- |
| 持久化 crate | `talos-store` | `orchestration/store` |
| 持久化子模块 | `project` / `session` | `talos-store` |
| 应用服务 crate | `talos-service` | `orchestration/service` |
| 应用服务子模块 | `session` / `timeline` / `history` / `turn` | `talos-service` |
| 持久化对象 | `XxxRecord` | 对应 store crate |
| 对外 DTO | `XxxInfo` | `talos_types` |
| 对外 API 输入 | `XxxRequest` | `talos_types` |
| 对外 API 输出 | `XxxResponse` | `talos_types` |
| service 内部命令 | `XxxCommand` | 对应 service crate |
| service 内部结果 | `XxxResult` | 对应 service crate |

说明：

- `Record` 命名只用于持久化对象。
- `Info` 命名用于对外可共享 DTO。
- public API 使用 `Request` / `Response`。
- service 内部不应滥用 `Request` 命名，避免和 public API 混淆。

## 与 Gateway 的关系

Gateway 仍然是 HTTP / JSON-RPC / Channel / SDK 之下的统一业务 API 门面。目录调整后，Gateway 不直接依赖 store，而是依赖 service。

```text
Transport Adapter
  -> Gateway
  -> application-service
  -> talos-service::{session,timeline,history,turn}
  -> talos-store::{project,session}
  -> database / files
```

Gateway 保持薄层：

- 不操作 DB。
- 不读写 markdown/json state files。
- 不组合多个 store。
- 不生成 Agent Kernel 输入。
- 不做 `Record -> Info` 转换。

## 与 AgentCoordinator / AgentHarness 的关系

AgentCoordinator 负责注册、路由、选择和分发 Agent。AgentHarness 负责包装 Agent Kernel。它们可以被 service 调用，但不应反向依赖 service 或 store。

```text
talos-service::turn
  -> AgentCoordinator
  -> AgentHarness
  -> Agent Kernel
```

执行结果回到 service 后，由 service 决定如何写入 timeline、active turn、session title、usage、idempotency 等持久化状态。

## 迁移约束

- 本 spec 已在 `refactor-027-core-api-object-boundary` 的边界收敛之后实施。
- 迁移按业务域移动，避免一次性全目录重排导致 review 失焦。
- 每移动一个 store 或 service，都必须保持编译和对应测试通过。
- 不为了目录移动引入新抽象；只有职责边界变化时才调整 API。
- 移动过程中不能重新引入 `XxxRecord` 到 `talos_types`。
- 移动过程中不能让 Gateway 或 Transport 直接依赖 store。

## 验收标准

- `orchestration/store` 是单一 `talos-store` crate，内部按 `project` / `session` 拆分持久化模块。
- `orchestration/service` 下只包含应用服务 crate。
- `XxxRecord` 只出现在 store crate 或 store 测试中。
- service 对上暴露的是 `XxxInfo` / `XxxRequest` / `XxxResponse` 或 service 内部 `XxxCommand` / `XxxResult`。
- Gateway 不直接依赖任何 `*-store` crate。
- Transport、SDK、Web、Desktop、TUI 不依赖任何 `XxxRecord`。
- 相关 crate 编译、单测和受影响 e2e 通过。
