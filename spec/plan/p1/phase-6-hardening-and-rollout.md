# Phase 6: Hardening and Rollout

**状态**: 已完成  
**目标**: 全量回归、清理临时兼容路径、补齐文档和发布前质量门禁。  
**依赖**: Phase 1-5 完成。  
**输出**: 可维护的最终形态，无遗留临时路径，无未记录测试缺口。

---

## 1. 范围

包含：

- 删除迁移期临时兼容路径。
- 清理过时 trait / DTO / helper。
- 补齐 spec、plan、README 或开发文档。
- 执行 Rust、集成、前端全量回归。
- 记录剩余风险。

不包含：

- 新增架构方向。
- 新增未评审的协议能力。
- 新增视觉大改版。

---

## 2. 子任务

### 6.1 临时路径清理

清理清单来自 Phase 1-5 各 sub plan 完成记录中的 `遗留临时路径（Phase 6 待删）` 字段。Phase 6 开始时必须先汇总这些字段，形成实际删除 checklist。

**任务**:

- [x] 汇总 Phase 1-5 的 `遗留临时路径（Phase 6 待删）`。
- [x] 清理 Phase 1 中保留的旧 `SessionService` / `RequestHandler` 临时依赖（服务端 production path 完成迁移；测试侧保留最小兼容 adapter）。
- [x] 清理 HTTP crate 中残留的共享 service 逻辑。
- [x] 清理 app-server 中绕过 application-service 的测试 helper。
- [x] 清理重复 DTO 或重复 enum。
- [x] 检查源码文件是否超过 1000 行。

**完成标准**:

- 没有长期 fallback 逻辑。

### 6.2 文档同步

**任务**:

- [x] 更新架构 spec 中已实现状态。
- [x] 更新本 plan 和所有 sub plan 完成记录。
- [x] 更新 CLI/TUI/Desktop 使用说明。
- [x] 更新协议生成命令说明。
- [x] 更新测试运行说明。

**完成标准**:

- 新贡献者可以从 spec + plan 理解最终形态。

### 6.3 全量回归

Phase 6 是唯一执行全量 e2e 的阶段。前面阶段只跑本阶段新增或相关的 crate / contract / smoke 测试。

**任务**:

- [x] Rust crate tests。
- [x] app-server e2e。
- [x] transport parity contract。
- [x] HTTP e2e。
- [x] CLI smoke。
- [x] TUI smoke 或 spike 结论复核。
- [x] Frontend lint/test/build。
- [x] Desktop smoke。

**完成标准**:

- 所有必须测试通过，或有明确、可接受、已记录的非相关 baseline 失败。

### 6.4 风险复盘

**任务**:

- [x] 复盘 back-pressure 与 subscription termination。
- [x] 复盘 actor credentials 安全边界。
- [x] 复盘 event replay 正确性。
- [x] 复盘 HTTP 与 app-server parity 覆盖。
- [x] 复盘 Desktop app-server crash recovery。

**完成标准**:

- 残余风险都有 owner 或 P2 承接计划。back-pressure、actor credentials、replay、parity 风险均在 full-e2e、app-server contract 与现有单测链路中得到当前覆盖；其余剩余风险见后续移交部分并保留 owner。

---

## 3. 测试

Phase 6 执行 [PLAN.md §5](PLAN.md#5-全局质量门禁) 的全量门禁。本节不重复维护命令清单，避免总计划与 phase 文档漂移。

---

## 4. 风险

| 风险 | 处理 |
|---|---|
| 临时兼容路径遗留 | Phase 6 不接受新增功能，只做收口 |
| 测试耗时过长 | 先按 crate 层定位失败，再跑全量 e2e |
| 文档与实现不同步 | 完成记录必须写关键文件与最终行为 |

---

## 5. 完成记录

**变更摘要**:  
已补齐 Phase 2/4 遗留的 app-server protocol parity：`project.create/list/read/patch/delete` 与 `session.turns` 从 deferred 移入强类型协议，接入 `talos-application-service` dispatch，并同步 Rust SDK、TS SDK 与 protocol e2e resource matrix。协议版本从 `0.1` bump 到 `0.2`，schema 与 TS generated fixture 已通过生成器刷新。Phase 6 已拆分 7 个既有超 1000 行文件，当前源码行数门禁无超限输出。已同步架构 spec、TS SDK spec、协议 changelog、CLI/TUI/Desktop 使用说明与集成测试运行说明。

**关键文件**:  
- `README.md`
- `spec/plan/p1/PLAN.md`
- `spec/plan/p1/phase-2-app-protocol.md`
- `spec/plan/p1/phase-4-cli-tui-and-parity.md`
- `spec/plan/p1/phase-5-ts-sdk-and-desktop.md`
- `spec/plan/p1/phase-6-hardening-and-rollout.md`
- `spec/desktop-appserver-shared-application-service-design.md`
- `spec/ts-sdk-design.md`
- `talos-rs/crates/gateway/app-protocol/src/requests.rs`
- `talos-rs/crates/gateway/app-protocol/src/schema.rs`
- `talos-rs/crates/gateway/app-protocol/src/version.rs`
- `talos-rs/crates/gateway/app-protocol/CHANGELOG.md`
- `talos-rs/crates/gateway/app-server/src/application.rs`
- `talos-rs/apps/sdk/src/app_server.rs`
- `talos-rs/apps/app-server-test-client/src/main.rs`
- `talos-rs/apps/ui-tui/README.md`
- `talos-rs/README.md`
- `talos-rs/tests/integration/README.md`
- `talos-app/README.md`
- `talos-app/ts-sdk/packages/core/src/app-server.ts`
- `talos-app/ts-sdk/packages/core/src/generated/app-protocol.ts`

**测试结果**:  
- `cargo run -p talos-app-protocol --example export_fixtures` 通过。
- `cargo test -p talos-app-protocol` 通过，12 passed。
- `cargo check -p talos-app-server -p talos-client -p talos-app-server-test-client` 通过。
- `pnpm --dir talos-app --filter @talos/sdk-core test` 通过，15 passed。
- `pnpm --dir talos-app --filter @talos/sdk-core typecheck` 通过。
- `pnpm --dir talos-app --filter @talos/sdk-app-server test` 通过，3 passed / 1 skipped。
- `pnpm --dir talos-app --filter @talos/sdk-app-server typecheck` 通过。
- `cargo test -p talos-app-server -p talos-client -p talos-app-server-test-client` 通过。
- `pnpm --dir talos-app --filter talos-desktop test` 通过。
- `source .env && tests/integration/run_e2e.sh --case 09_app_server/protocol_e2e` 通过，报告 `e2e_report_20260430_091242.md`。
- `source .env && TALOS_APP_SERVER_BIN=/Users/xujiang/data/ailabs/talos/talos-rs/target/debug/talos TALOS_APP_SERVER_CWD=/Users/xujiang/data/ailabs/talos/talos-rs pnpm --dir ../talos-app --filter @talos/sdk-app-server test:integration` 通过，1 passed。
- `cargo test -p talos-prompt` 通过，40 passed。
- `cargo test -p talos-tools registry` 通过，23 passed。
- `cargo test -p talos-ui-tui --lib` 通过，104 passed / 2 ignored。
- `cargo test -p talos-session-store` 通过，29 passed。
- `cargo test -p talos-agent-kernel general_turn` 通过，51 passed。
- 源码行数检查通过：无 `.rs/.ts/.tsx` 文件超过 1000 行。
- `cargo test -p talos-app-protocol -p talos-application-service -p talos-coordinator -p talos-app-server -p talos-gateway-http -p talos-client` 通过。
- `cargo test -p talos-gateway-http` 通过，54 passed。
- `cargo test -p talos-app-server -p talos-client -p talos-app-server-test-client` 通过。
- `cargo test -p talos-client -p talos-cli -p talos-ui-tui --lib` 通过，TUI 104 passed / 2 ignored。
- `pnpm --dir talos-app lint` 通过。
- `pnpm --dir talos-app test` 通过：core 15 passed，app-server 3 passed / 1 skipped，desktop 1 passed，web 54 passed。
- `pnpm --dir talos-app build` 通过。
- `source .env && tests/integration/run_e2e.sh` 通过，报告 `e2e_report_20260430_102400.md`。
- `source .env && tests/integration/run_e2e.sh`（重跑后）通过，报告 `e2e_report_20260430_121110.md`。
- `source .env && tests/integration/scenarios/09_app_server/protocol_e2e.sh` 通过，报告 `e2e_report_20260430_121008.md`。
- `source .env && tests/integration/run_e2e.sh --case 09_app_server/protocol_e2e` 通过，报告 `e2e_report_20260430_125313.md`。
- `source .env && tests/integration/run_e2e.sh --case 05_cli/app_server_e2e` 通过，报告 `e2e_report_20260430_125314.md`。

**遗留临时路径（Phase 6 待删）**:  
- 无仍需 Phase 6 删除的临时路径。`AppServerClient::send_message_events` 已完成后台 reader + subscription 分发模型改造，支持增量事件渲染。
- `DEFERRED_CLIENT_METHODS` 仍保留 `tool.dynamic_result.submit`、`admin.session.list`、`admin.project.list`，属于 P2 protocol freeze 项，不再作为 Phase 6 临时路径清理项。
- 源码行数超限项已拆分：`session-store/src/lib.rs`、`tools/src/registry.rs`、`agent-kernel/src/general_turn/prepare.rs`、`prompt/src/tests.rs`、`ui-tui/src/tests/session_tool_tests.rs`、`agent-kernel/src/general_turn/tests/loop_tests.rs`、`ui-tui/src/stream.rs`。

**未覆盖风险**:  
- 真实 pending approval 到 resumed turn 的 agent runtime 状态推进由 [../p2/phase-2-pending-approval-resume-regression.md](../p2/phase-2-pending-approval-resume-regression.md) 承接。
- Desktop app-server crash recovery 产品级 smoke 由 [../p2/phase-3-desktop-crash-recovery-smoke.md](../p2/phase-3-desktop-crash-recovery-smoke.md) 承接。

**后续移交**:  
- owner: `orchestration/runtime`：确认 back-pressure、subscription terminate 与 resumed turn edge 的长期监控告警与回归用例归档到 `talos-coordinator`。
- owner: `gateway/app-server`：验证 app-server credential 边界与 tool approval 续接逻辑，待处理事项见 P2。
- owner: `desktop`：补充 app-server 本地生命周期 crash recovery 与重连体验测试，聚焦 `talos-desktop` controller/sandbox 场景。
