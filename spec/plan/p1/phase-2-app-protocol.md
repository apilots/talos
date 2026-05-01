# Phase 2: App Protocol

**状态**: 已完成  
**目标**: 建立强类型 `talos-app-protocol`，定义 JSON-RPC over stdio JSONL 的协议类型、错误模型、版本策略和生成物。  
**依赖**: Phase 0 完成；Phase 1 的 application params 边界基本稳定。  
**输出**: `crates/gateway/app-protocol` 可独立测试、可生成 TS 类型和 JSON Schema。


---

## 1. 范围

协议 DTO 命名与字段必须对齐 application-service 入口；如果对应 application-service 入口尚未稳定，该 DTO 只能先保留草案，不能 freeze schema fixture。Phase 2 可以和 Phase 1 部分并行，但不能绕过 Phase 1 自行定义第二套业务参数模型。

包含：

- 新增 `crates/gateway/app-protocol`。
- 定义 JSON-RPC 2.0 message 类型。
- 定义 client request、server notification、server request。
- 定义 initialize、event.subscribe、turn.start 等首批协议 DTO。
- 定义 error code 与强类型 error data。
- 定义 protocol version、schema fixture、TS 生成物校验。

不包含：

- app-server 进程管理。
- stdin/stdout IO 实现。
- Electron IPC。

---

## 2. 子任务

### 2.1 crate 骨架

**任务**:

- [x] 新增 `crates/gateway/app-protocol`。
- [x] 定义 public module：
  - [x] `jsonrpc`
  - [x] `requests`
  - [x] `notifications`
  - [x] `server_requests`
  - [x] `events`
  - [x] `errors`
  - [x] `version`
  - [x] `schema`
- [x] 配置 serde、schemars 或项目已有 schema 生成方式。

**完成标准**:

- `cargo test -p talos-app-protocol` 可运行。

### 2.2 JSON-RPC 2.0 基础类型

**任务**:

- [x] 定义 `JsonRpcRequest`。
- [x] 定义 `JsonRpcResponse`。
- [x] 定义 `JsonRpcNotification`。
- [x] 定义 `JsonRpcError`。
- [x] 定义 `RequestId`。
- [x] 强制所有 message 带 `jsonrpc: "2.0"`。
- [x] 限制 `serde_json::Value` 只存在于协议 decode 边界。

**完成标准**:

- parse/serialize roundtrip 测试通过。
- malformed request 能映射到标准错误码。

### 2.3 Client requests

**任务**:

- [x] 定义 `initialize`。
- [x] 定义 session create/list/read/patch/delete。
- [x] 定义 `turn.start`。
- [x] 定义 `turn.interrupt`。
- [x] 定义 `event.subscribe`。
- [x] 定义 tool approval submit。
- [x] 定义 model/config/memory/workspace docs 首批 request。
- [x] 定义 skills/apps/tools list。
- [x] 定义 account get。
- [x] 定义 memory search。
- [x] 对 spec 已列但本阶段暂不 freeze 的 method 建立 deferred list，不能静默遗漏。

**完成标准**:

- method string 到强类型 request enum 的映射稳定。

### 2.4 Event envelope 与 notifications

**任务**:

- [x] 定义 `ApplicationEventEnvelope`。
- [x] 明确 wire 层 `event` 使用 `ApplicationEvent` tagged enum。
- [x] 为每个 canonical event id 生成收紧后的 TS 类型。
- [x] 定义 `subscription.terminated`。
- [x] 确认 notification method 与 SSE event / frontend event type 完全一致。

**完成标准**:

- `agent.message.delta` 等事件在 Rust/TS/schema 中命名一致。

### 2.5 Server requests

**任务**:

- [x] 定义 `client.user_input.request`。
- [x] 定义 `client.auth.refresh`。
- [x] 定义 `client.approval.request`。
- [x] 定义客户端不支持 server request 时的 fail-fast 错误。

**完成标准**:

- server request 有独立 request id 与 response 类型。

### 2.6 Error model

**任务**:

- [x] 定义 JSON-RPC 标准错误码。
- [x] 定义业务错误码区间 `-32099..=-32000`。
- [x] 定义强类型 `AppServerErrorData`。
- [x] 禁止任意 metadata JSON。
- [x] 覆盖 unauthorized、forbidden、conflict、not found、not initialized。

**完成标准**:

- 错误序列化稳定，schema fixture 覆盖业务错误。

### 2.7 Version 与生成物

**任务**:

- [x] 定义 `ProtocolVersion`。
- [x] 生成 JSON Schema fixture。
- [x] 生成 TypeScript fixture。
- [x] 建立 CI 校验命令或测试。
- [x] 记录 changelog 更新规则。

**完成标准**:

- Rust 是协议类型真源。
- TS/schema 生成物不允许长期手写漂移。

---

## 3. 测试

本阶段重点新增测试：

- JSON-RPC message parse/serialize。
- method dispatch。
- error code mapping。
- schema fixture。
- generated TS fixture。
- tagged enum event envelope。

全量质量门禁见 [PLAN.md §5](PLAN.md#5-全局质量门禁)。

---

## 4. 风险

| 风险 | 处理 |
|---|---|
| 协议类型绕开 application params | 每个 request DTO 必须标明映射到的 application-service params |
| TS 类型手写漂移 | fixture 校验必须进入 CI |
| Event envelope 表达不一致 | Rust tagged enum 是 wire 真源，TS 泛型只做收紧 |

---

## 5. 完成记录

**变更摘要**:  
已完成 `talos-app-protocol` crate，建立 JSON-RPC 2.0 基础类型、client request params、server notification、server request、application event envelope、强类型 error data、protocol version、schema/TS 生成函数与 fixture 漂移测试。Rust 作为协议真源，JSON Schema fixture 提交在 protocol crate 内，TypeScript 生成物接入现有 `@talos/sdk-core` 并以 `AppProtocol` namespace re-export。Phase 6 已将协议版本推进到 `0.2`，freeze `project.create/list/read/patch/delete` 与 `session.turns`，并通过 `cargo run -p talos-app-protocol --example export_fixtures` 刷新 schema 与 TS generated fixture。当前暂不 freeze 的 `tool.dynamic_result.submit`、`admin.session.list`、`admin.project.list` 仍写入 `DEFERRED_CLIENT_METHODS`，由 [../p2/phase-1-freeze-deferred-protocol-methods.md](../p2/phase-1-freeze-deferred-protocol-methods.md) 承接。

**关键文件**:  
- `talos-rs/crates/gateway/app-protocol`
- `talos-rs/crates/gateway/app-protocol/schemas/app-protocol.schema.json`
- `talos-app/ts-sdk/packages/core/src/generated/app-protocol.ts`
- `talos-app/ts-sdk/packages/core/src/index.ts`
- `talos-rs/Cargo.toml`

**测试结果**:  
- `cargo test -p talos-app-protocol` 通过，12 passed。
- `pnpm --dir talos-app/ts-sdk/packages/core typecheck` 通过。

**遗留临时路径（Phase 6 待删）**:  
- `tool.dynamic_result.submit`：等待 server-request 与动态工具结果回传边界稳定后 freeze。
- `admin.session.list`、`admin.project.list`：等待本地 admin actor credentials 与权限产品边界稳定后 freeze。

**未覆盖风险**:  
Phase 7 已将 protocol event 收敛为复用共享 `TalosEvent` / `TalosEventPayload`，不再保留 app protocol 私有窄 DTO。当前剩余风险集中在 `DEFERRED_CLIENT_METHODS` 中明确登记的 protocol freeze 项，由 P2 Phase 1 承接。

**后续移交**:  
Phase 3/6/7 已直接复用本 crate 的 `ClientRequest::decode`、`JsonRpcMessage`、`AppServerErrorBody` 和 generated event notification method；后续新增协议能力继续以 app-protocol Rust 类型为真源，不得在 app-server 内重复定义协议 DTO。
