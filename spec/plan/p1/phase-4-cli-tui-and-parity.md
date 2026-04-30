# Phase 4: CLI/TUI and Transport Parity

**状态**: 已完成  
**目标**: 建立 CLI/TUI 本地验证入口，并用 transport 等价性契约测试防止 HTTP 与 app-server 漂移。  
**依赖**: Phase 1 application-service；Phase 3 app-server。  
**输出**: CLI app-server mode、app-server-test-client 集成回归、HTTP 与 JSON-RPC 语义等价测试。

---

## 1. 范围

包含：

- CLI 支持 app-server mode smoke。
- `app-server-test-client` 纳入集成测试。
- TUI 接 app-server 可行性 spike。
- HTTP adapter 与 JSON-RPC adapter 的 contract parity tests。

不包含：

- Desktop Electron 实现。
- TUI 完整 UI 重写。
- 前端视觉优化。

---

## 2. 子任务

### 4.1 CLI app-server mode

**任务**:

- [x] 增加 CLI app-server mode 参数或子命令。
- [x] 支持 spawn `talos app-server`。
- [x] 支持 initialize。
- [x] 支持 create session。
- [x] 支持 start turn。
- [x] 输出基础事件。

**完成标准**:

- CLI 能以本地 app-server 路径跑通基础会话。

### 4.2 App-server test client 集成

**任务**:

- [x] 将 smoke scenario 接入 integration test。
- [x] 将 malformed line scenario 接入 integration test。
- [x] 将 replay scenario 接入 integration test。
- [x] 将 active turn conflict scenario 接入 integration test。
- [x] 将 multi-subscriber scenario 接入 integration test。

**完成标准**:

- CI 或本地集成测试能稳定运行 app-server 协议回归；全量 e2e 留到 Phase 6。

### 4.3 Transport 等价性契约测试

本子任务可以分两步执行：Phase 2 完成后先建立 skeleton 和 fixture shape；Phase 3 app-server 可运行后再要求完整通过。

**任务**:

- [x] 建立共享 test harness，复用同一个 `TalosApplicationService`。
- [x] HTTP adapter scenario 创建 session。
- [x] JSON-RPC app-server adapter scenario 创建 session。
- [x] 比较 CLI application result 语义。
- [x] 比较 JSON-RPC event replay envelope。
- [x] 比较 tool approval 状态转换入口与错误响应映射。
- [x] 比较 config/model/memory/workspace docs 成功和错误响应。

**完成标准**:

- 新能力若只接 HTTP 或只接 app-server，contract test 能暴露差异。

### 4.4 TUI feasibility spike

**任务**:

- [x] 盘点 TUI 当前 transport 依赖。
- [x] 实现最小 app-server client spike。
- [x] 验证 session list / start turn / event stream。
- [x] 记录 pass/fail 结论。
- [x] 如果 fail，记录阻塞点和推荐迁移顺序。

**完成标准**:

- 得到明确 pass/fail，不保留模糊“方案明确”表述。

---

## 3. 测试

本阶段重点新增测试：

- CLI app-server smoke。
- app-server-test-client scenarios。
- HTTP vs JSON-RPC parity contract。
- TUI spike smoke。

全量质量门禁见 [PLAN.md §5](PLAN.md#5-全局质量门禁)。

---

## 4. 风险

| 风险 | 处理 |
|---|---|
| CLI 同时维护两套语义 | CLI 只验证 transport 差异，业务语义来自 application-service |
| parity test 过重 | 先覆盖 session/turn/event/tool/config 五条主链路 |
| TUI spike 扩散为大迁移 | spike 只给 pass/fail，不做完整 UI 改造 |

---

## 5. 完成记录

**变更摘要**:  
已新增 Rust SDK app-server JSON-RPC stdio client。Phase 7 已进一步收敛：Rust SDK 移除正式 HTTP client 暴露；CLI 固定通过 app-server JSON-RPC 本地子进程验证；TUI 固定使用 app-server JSON-RPC 子进程，不再启动或连接 HTTP gateway；`app-server-test-client` 复用 Rust SDK transport，仅 malformed raw protocol 场景保留底层 harness。Phase 4.3 已补同一进程共享 `TalosApplicationService` 的 HTTP/app-server adapter contract，并增加 app-server resource/error matrix 覆盖 config、model、memory、workspace docs、session not found、tool approval not found。

**关键文件**:  
- `talos-rs/apps/sdk/src/lib.rs`
- `talos-rs/apps/sdk/src/app_server.rs`
- `talos-rs/apps/cli/src/cli.rs`
- `talos-rs/apps/ui-tui/src/main.rs`
- `talos-rs/apps/ui-tui/src/client.rs`
- `talos-rs/apps/app-server-test-client/src/main.rs`
- `talos-rs/crates/gateway/http/src/server/tests.rs`
- `talos-rs/crates/gateway/http/src/server/messages.rs`
- `talos-rs/tests/integration/scenarios/05_cli/app_server_e2e.sh`
- `talos-rs/tests/integration/scenarios/09_app_server/protocol_e2e.sh`

**测试结果**:  
- `cargo test -p talos-gateway-http -p talos-client -p talos-app-server-test-client` PASS
- `source .env && tests/integration/run_e2e.sh --case 09_app_server/protocol_e2e` PASS，报告 `e2e_report_20260430_074516.md`
- `source .env && tests/integration/run_e2e.sh --case 05_cli/app_server_e2e` PASS，报告 `e2e_report_20260430_074550.md`
- `source .env && tests/integration/run_e2e.sh --case 05_cli/app_server_e2e` PASS，报告 `e2e_report_20260430_010247.md`
- `source .env && tests/integration/run_e2e.sh --case 05_cli/full_flow` PASS，报告 `e2e_report_20260430_010321.md`
- `source .env && tests/integration/run_e2e.sh --case 09_app_server/protocol_e2e` PASS，报告 `e2e_report_20260430_010448.md`
- `cargo test -p talos-cli` PASS
- `cargo test -p talos-client -p talos-cli` PASS
- `cargo test -p talos-client -p talos-ui-tui --lib` PASS
- `cargo check -p talos-client -p talos-cli -p talos-ui-tui -p talos-app-server-test-client` PASS

**遗留临时路径（Phase 6 待删）**:  
- 无。`talos-rs/apps/app-server-test-client` 继续作为协议回归工具保留；`AppServerClient::send_message_events` 已改为后台 stdout reader + subscription 分发模型，支持 CLI/TUI app-server 路径增量事件渲染。

**未覆盖风险**:  
- `session.turns` 已在 Phase 6 补齐 app-protocol、app-server dispatch、Rust/TS SDK 和 protocol e2e；Phase 7 后 TUI 历史视图已通过 Rust SDK JSON-RPC `get_session_turns` 接入该能力。
- tool approval 已覆盖 transport 入口与错误映射；真实 pending approval 到 resumed turn 的 agent runtime 状态推进仍归 Phase 6 全量回归补强。

**后续移交**:  
- Phase 5 Desktop 直接复用 Rust/TS SDK 的 JSON-RPC stdio app-server 子进程方案，不走本地 HTTP 端口。
- Phase 6/7 已完成 CLI/TUI smoke 与 full e2e 回归，并将 `AppServerClient::send_message_events` 演进为后台 reader + subscription 分发模型；后续继续补强真实 tool approval resume 回归。
