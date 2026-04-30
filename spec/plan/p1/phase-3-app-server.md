# Phase 3: Stdio App Server

**状态**: 已完成  
**目标**: 实现 `talos app-server`，通过 stdio JSONL 提供 JSON-RPC 2.0 本地协议入口。  
**依赖**: Phase 1 application-service 可用；Phase 2 app-protocol 可用。  
**输出**: `crates/gateway/app-server` 与 `talos app-server` 命令。


---

## 1. 范围

包含：

- 新增 `crates/gateway/app-server`。
- 实现 stdin/stdout JSONL framing。
- 实现 initialize / initialized 状态机。
- 实现 request dispatch 到 `TalosApplicationService`。
- 实现 event subscription notification。
- 实现 server request 基础能力。
- 实现 cancellation、back-pressure、shutdown。

不包含：

- Electron shell。
- TS SDK transport。
- TUI 完整迁移。

---

## 2. 子任务

### 3.1 crate 与命令入口

**任务**:

- [x] 新增 `crates/gateway/app-server`。
- [x] 暴露 app-server runtime API。
- [x] 在 `talos` binary 中增加 `talos app-server`。
- [x] 确保 stdout 只输出协议消息。
- [x] 日志只写 stderr。

**完成标准**:

- 空启动后能等待 stdin JSONL。

### 3.2 JSONL framing

**任务**:

- [x] 逐行读取 stdin。
- [x] 逐行写出 stdout。
- [x] malformed JSONL 返回 parse error 或记录 stderr。
- [x] 单行处理失败不能污染后续消息。
- [x] 支持 request、response、notification 三类消息。

**完成标准**:

- malformed line 测试通过，进程保持健康。

### 3.3 Initialize 与 actor

**任务**:

- [x] 初始化前只允许 `initialize`。
- [x] 校验 `actor_credentials`。
- [x] 构造统一 `Actor`。
- [x] 支持 `initialized` client -> server notification。
- [x] 未收到 `initialized` 之前，server 不得发出任何 notification 或 server request。
- [x] 初始化失败返回 `UNAUTHORIZED` 或 `NOT_INITIALIZED`。

**完成标准**:

- app-server 不存在匿名降级或 admin 降级。

### 3.4 Request dispatch

**任务**:

- [x] session create/list/read。
- [x] `turn.start`。
- [x] `turn.interrupt`。
- [x] `event.subscribe`。
- [x] `config.read`。
- [x] `model.list`。
- [x] tool approval submit mock flow。

**完成标准**:

- 所有业务 request 都通过 `TalosApplicationService`。

### 3.5 Event notification

App-server 必须通过 `TalosApplicationService::subscribe_session_events` 获得专属 stream，也就是 per-subscription channel。App-server 不直接持有 event store，不直接实现 replay 查询，也不复用其他订阅者的 stream。

**任务**:

- [x] 订阅 `ApplicationEventStream`。
- [x] 将 envelope 转成 JSON-RPC notification。
- [x] 使用 canonical event id 作为 method。
- [x] 支持 `since_event_id` replay。
- [x] 支持 `subscription.terminated`。

**完成标准**:

- app-server 能收到 turn started / delta / completed notifications。

### 3.6 Back-pressure 与 cancellation

**任务**:

- [x] outgoing channel 有界。
- [x] per-subscription bounded channel 不阻塞其他订阅者。
- [x] stdout 写失败时取消该客户端拥有的 active turn。
- [x] stdin EOF 时取消该客户端拥有的 active turn 并退出。
- [x] 慢订阅超过容量时发送 `subscription.terminated`。

**完成标准**:

- 卡住的订阅者不会阻塞全局 fan-out。

### 3.7 App-server test client

**任务**:

- [x] 新增 `apps/app-server-test-client` 或确认现有位置。
- [x] 支持 smoke scenario。
- [x] 支持 malformed JSONL scenario。
- [x] 支持 replay scenario。
- [x] 支持 multi-subscriber scenario。

**完成标准**:

- test client 能作为 Desktop 之前的本地协议回归验证入口。

---

## 3. 测试

本阶段重点新增测试：

- initialize 状态机。
- actor credentials 校验。
- malformed JSONL。
- stdout/stderr 隔离。
- event.subscribe replay。
- stdin EOF cancellation。
- stdout write failure cancellation。
- slow subscription termination。

全量质量门禁见 [PLAN.md §5](PLAN.md#5-全局质量门禁)。

---

## 4. 风险

| 风险 | 处理 |
|---|---|
| app-server 直接调 coordinator | review dispatch 层，只允许调 application-service |
| stdout 被日志污染 | 日志 writer 强制 stderr |
| cancellation 语义不清 | 只取消该客户端拥有的 active turn |
| JSON-RPC streaming 误用 | subscribe 只返回 ack，事件走 notification |

---

## 5. 完成记录

**变更摘要**:  
新增 stdio JSON-RPC app-server crate、真实 `talos app-server` 命令、共享 `TalosApplicationService` composition runtime、事件订阅 notification mapper、actor credential 校验、app-server test client。

**关键文件**:  
`talos-rs/crates/gateway/app-server`, `talos-rs/crates/app/talos-server/src/lib.rs`, `talos-rs/bin/talos/src/main.rs`, `talos-rs/apps/app-server-test-client`, `talos-rs/crates/orchestration/application-service/src/event.rs`, `talos-rs/crates/gateway/app-protocol/src/errors.rs`。

**测试结果**:  
`cargo test -p talos-app-protocol -p talos-app-server -p talos-application-service -p talos-server -p talos-gateway-http` 通过；`cargo test -p talos-coordinator -p talos-client` 通过；`cargo check -p talos -p talos-app-server-test-client` 通过；`pnpm --dir talos-app/ts-sdk/packages/core typecheck` 通过；`TALOS_LOCAL_LAUNCH_TOKEN=secret ./target/debug/talos app-server` initialize/model.list smoke 通过。

**遗留临时路径（Phase 6 待删）**:  
无。

**未覆盖风险**:  
server request 反向调用目前只冻结协议类型，尚未接入真实需要客户端交互的工具流程；`skill.list` / `app.list` / `tool.list` 仍等待后续正式 application-service facade，不返回硬编码假数据。

**后续移交**:  
Phase 4 接入 CLI/TUI 本地验证和 transport parity contract；Phase 5 接入 TS SDK/Desktop transport。
