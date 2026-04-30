# Phase 7: Server and SDK Boundary Convergence

**状态**: 已完成  
**目标**: 在进入 CR 前完成 server / gateway / SDK 分层收敛，消除新增模块位置不当、协议层重复定义、客户端重复封装等问题。  
**依赖**: Phase 1-6 已完成后的当前实现状态。  

---

## 1. 背景

当前 Phase 1-6 已完成主要能力落地，但 CR 前仍存在架构边界需要收敛：

1. `talos-application-service` 放在 `crates/app/application-service`，与现有分层中 `app` 作为 composition root / runtime 入口的定位不匹配。
2. `talos-app-protocol` 与 `talos-app-server` 的职责需要重新明确，避免协议契约、transport runtime、业务 service 混杂。
3. `talos-server` 应成为多种 server mode 的统一出口，包括 HTTP server 与 JSON-RPC stdio server。
4. Rust / TS SDK 应保持薄封装，复用 shared protocol / shared types，不重复定义业务结构体和 API 语义。
5. Rust SDK 只保留 JSON-RPC stdio client 能力，不继续暴露 HTTP client。

---

## 2. 目标分层

按当前 Talos 既有分层架构归位：

```text
crates/orchestration/application-service
  Shared server application use cases for HTTP and JSON-RPC.

crates/gateway/app-protocol
  JSON-RPC protocol contract, methods, params, responses, notifications, errors, schema export.

crates/gateway/app-server
  JSON-RPC stdio runtime and transport adapter.

crates/gateway/http
  HTTP and SSE runtime and transport adapter.

crates/app/talos-server
  Unified server composition root and runtime mode owner.

apps/sdk
  Rust SDK, JSON-RPC client only.

talos-app/ts-sdk
  Thin TypeScript SDK with shared generated protocol/types and transport adapters.
```

Dependency direction:

```text
apps/* and ts-sdk
    |
    v
talos-server / talos-client
    |
    v
gateway/http or gateway/app-server
    |
    v
orchestration/application-service
    |
    v
orchestration/coordinator + session-store
common/types + infra/* + cognition/* + action/*
```

---

## 3. 子阶段

### 3.1 Move Application Service Into Orchestration

**目标**: 将 `crates/app/application-service` 迁移到 `crates/orchestration/application-service`，crate name 保持 `talos-application-service`。

**任务**:

- 移动目录。
- 更新 workspace members。
- 更新 workspace dependency path。
- 更新所有 crate 的 path 引用。
- 保持 public API 不变，先只做目录归位。
- 确认 `crates/app` 只保留 runtime / composition root 类 crate。

**成功标准**:

- `talos-application-service` 位置符合现有 orchestration 分层。
- `gateway/http`、`gateway/app-server`、`talos-server` 引用正常。
- 无业务逻辑在本阶段被顺手重写。

**测试**:

```text
cargo check -p talos-application-service
cargo check -p talos-gateway-http -p talos-app-server -p talos-server
```

### 3.2 Clean App Protocol Boundary

**目标**: 保留 `crates/gateway/app-protocol`，但严格限定为 JSON-RPC 协议契约层。

**允许内容**:

- JSON-RPC envelope。
- method enum。
- params / response DTO。
- notification DTO。
- protocol error body / code。
- protocol version。
- schema export。

**禁止内容**:

- 重复定义 `TalosEvent` / `TalosEventPayload`。
- 重复定义 session / project / memory / tool 等业务对象。
- 承载 application-service 业务规则。
- 使用松散 JSON 结构表达可强类型化的业务语义。

**成功标准**:

- `app-protocol` 只表达协议契约。
- 业务对象引用 `talos-types` 或其他共享强类型。
- HTTP 层不依赖 JSON-RPC 私有语义。

**测试**:

```text
cargo test -p talos-app-protocol
pnpm --dir talos-app typecheck
```

### 3.3 Make Talos Server The Unified Runtime Exit

**目标**: `talos-server` 统一拥有 HTTP 与 JSON-RPC stdio 两种启动模式。

**目标 API**:

```rust
start_http_server(config, options) -> TalosServerRuntime
run_json_rpc_stdio(config, options) -> TalosResult<()>
build_application_runtime(config) -> TalosApplicationRuntime
```

**职责边界**:

- `talos-server` 负责 config、storage、auth、session-store、coordinator、application-service 的组装。
- `gateway/http` 只负责 HTTP route、SSE encoding、middleware。
- `gateway/app-server` 只负责 JSON-RPC stdio loop、initialize lifecycle、request dispatch、subscription runtime。
- CLI / TUI / Desktop 不直接组装 server 内部依赖。

**成功标准**:

- HTTP 和 JSON-RPC 均复用同一个 `TalosApplicationService`。
- JSON-RPC stdio 模式由 `talos-server` 统一启动。
- 本地客户端不需要占用 HTTP port。

**测试**:

```text
cargo check -p talos-server -p talos-app-server
cargo test -p talos-app-server
```

### 3.4 Make Rust SDK JSON-RPC Only

**目标**: Rust SDK 只保留 JSON-RPC stdio client，移除正式 HTTP client 暴露。

**任务**:

- `apps/sdk` 不再导出 HTTP client。
- 删除或迁出 `connect_http` / `connect_default_http`。
- CLI / TUI / app-server-test-client 使用 JSON-RPC client。
- HTTP 测试 helper 如仍需要，放入 HTTP crate test support 或 integration helper。
- SDK re-export shared protocol / shared types，避免重复 DTO。

**成功标准**:

- `talos-client` crate public API 不再暴露 HTTP client。
- Rust SDK 只承担 JSON-RPC transport、request id、pending response、typed helper、event stream helper。
- 无客户端生成业务 event sequence。

**测试**:

```text
cargo check -p talos-client -p talos-cli -p talos-ui-tui -p talos-app-server-test-client
cargo test -p talos-client
```

### 3.5 Thin TypeScript SDK

**目标**: TS SDK 复用 generated protocol / shared types，只保留 transport 薄封装。

**建议结构**:

```text
talos-app/ts-sdk/packages/core
  generated protocol re-export, shared client helpers, shared event helpers.

talos-app/ts-sdk/packages/app-server
  JSON-RPC stdio client and process spawn for Node/Desktop.

webui HTTP client
  HTTP/SSE transport for Web UI only, with shared business types.
```

**任务**:

- 扫描 `types.ts`、`client.ts`、`app-server.ts` 中重复 DTO。
- 业务事件、session、project、memory 类型复用 generated protocol 或共享 schema。
- transport 内部消化 HTTP / JSON-RPC 差异。
- 限制 `Record<string, unknown>` 等松散结构继续扩散。

**成功标准**:

- TS SDK 不手写重复 `TalosEvent*`。
- Node/Desktop JSON-RPC client 复用 generated request / response / event 类型。
- Web HTTP client 与 JSON-RPC client API 语义对齐，但 transport 独立。

**测试**:

```text
pnpm --dir talos-app typecheck
pnpm --dir talos-app test
```

### 3.6 Client Integration Alignment

**目标**: 本地客户端统一通过 JSON-RPC stdio 连接 `talos-server`。

**接入规则**:

```text
CLI      -> Rust SDK JSON-RPC
TUI      -> Rust SDK JSON-RPC
Desktop  -> TS SDK JSON-RPC
Web UI   -> HTTP/SSE
```

**成功标准**:

- CLI 成为服务端全量能力回归入口。
- TUI / Desktop 不依赖 HTTP port。
- 客户端只处理服务端事件，不定义业务状态机。
- 服务端 sequence / replay / active turn 语义由 application-service 和 event service 提供。

**测试**:

```text
cd talos-rs && source .env && tests/integration/run_e2e.sh
cd talos-rs && source .env && tests/integration/protocol_smoke.sh
```

---

## 4. CR Gate

进入 CR 前必须确认：

- [x] `talos-application-service` 已迁移到 `crates/orchestration/application-service`。
- [x] `app-protocol` 只保留 JSON-RPC 协议契约。
- [x] `talos-server` 是 HTTP / JSON-RPC stdio 的统一启动出口。
- [x] HTTP / JSON-RPC 都复用同一个 application-service。
- [x] Rust SDK 只支持 JSON-RPC。
- [x] TS SDK 不重复定义业务 DTO。
- [x] 客户端不生成 event sequence。
- [x] 无重复 error 抽象，统一使用 `talos-error`。
- [x] 无可避免的 `serde_json::Value` / flexible metadata JSON 扩散。
- [x] 单个源码文件不超过 1000 行。
- [x] 全量真实回归通过。

---

## 5. 完成记录

### 变更摘要

- `talos-application-service` 归位到 `crates/orchestration/application-service`，workspace path 同步更新。
- `talos-server` 新增 JSON-RPC stdio 统一启动出口，`talos app-server` 不再由 CLI 二进制直接组装 application runtime。
- Rust SDK 移除正式 HTTP client 暴露，只保留 JSON-RPC stdio client；CLI / TUI / app-server-test-client 切换到 Rust SDK JSON-RPC。
- TUI 固定使用内嵌 app-server 子进程，不再暴露 HTTP gateway mode。
- TS SDK core 收敛为 shared generated protocol / shared business types，Web streaming 测试补齐强类型 payload。
- 集成脚本中 CLI 场景切换为 JSON-RPC 路径，`protocol_smoke.sh` 独立覆盖 app-server protocol smoke。

### 关键文件路径

- `talos-rs/crates/orchestration/application-service`
- `talos-rs/crates/app/talos-server/src/lib.rs`
- `talos-rs/bin/talos/src/main.rs`
- `talos-rs/apps/sdk/src/{lib.rs,app_server.rs,app_server_events.rs}`
- `talos-rs/apps/cli/src/cli.rs`
- `talos-rs/apps/ui-tui/src/{main.rs,client.rs,app.rs,run.rs}`
- `talos-rs/tests/integration/{protocol_smoke.sh,scenarios/05_cli,scenarios/06_tools}`
- `talos-app/ts-sdk/packages/core/src/types.ts`
- `talos-app/web/src/features/chat/{stream-events.ts,__tests__/streaming.test.ts}`

### 已执行测试

- `cargo check -p talos-application-service -p talos-server -p talos-app-server -p talos-gateway-http -p talos-client -p talos-cli -p talos-ui-tui -p talos-app-server-test-client` 通过。
- `cargo test -p talos-application-service -p talos-gateway-http -p talos-cli --no-fail-fast` 通过。
- `cargo test -p talos-client -p talos-app-server -p talos-app-protocol --no-fail-fast` 通过。
- `cargo test -p talos-coordinator -p talos-ui-tui --no-fail-fast` 通过。
- `pnpm --dir talos-app typecheck` 通过。
- `pnpm --dir talos-app test` 通过。
- `cd talos-rs && source .env && tests/integration/protocol_smoke.sh` 通过。
- `cd talos-rs && source .env && tests/integration/run_e2e.sh` 通过，报告 `tests/integration/reports/e2e_report_20260430_214337.md`。

### 遗留临时路径（Phase 6 待删）

- 无。

### 未覆盖风险

- 无已知阻塞 CR 的 Phase 7 分层风险。跨阶段剩余事项为明确 deferred 能力或产品级 smoke，继续由 Phase 2/5/6 完成记录跟踪。

### 后续 phase 需要接手的事项

- Phase 7 无新增后续事项。
