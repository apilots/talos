# P2 Phase 2: Pending Approval Resume Regression

**状态**: 已完成  
**目标**: 补齐真实 pending approval 到 resumed turn 的完整回归。  
**输出**: CLI first 集成 case 与 JSON-RPC app-server event sequence case。  

---

## 1. 背景

当前已覆盖 tool approval 的 transport 入口与错误映射，但仍需要真实回归覆盖：

```text
pending approval event
  -> submit approval
  -> resumed turn continues
  -> tool completes
  -> final message completes
```

这是执行状态机正确性的核心风险，应在 P2 优先处理。

---

## 2. 范围

- 覆盖真实工具 pending approval，而不是 mock transport-only case。
- 覆盖 CLI first 路径和 JSON-RPC app-server event sequence。
- 校验服务端提供的 event id / sequence / turn state；客户端不得自行生成业务 sequence。

---

## 3. 非目标

- 不改 approval 业务语义。
- 不为测试新增 fallback planner / fallback tool 路径。
- 不在 CLI 或 SDK 中复制 runtime 状态机。
- 不在本阶段运行全量 `tests/integration/run_e2e.sh`。

---

## 4. 子任务

- [x] 定位现有 approval tool fixture、active turn 查询和 submit approval API。
- [x] 补齐 CLI 集成 case：启动 session、触发 pending approval、提交 approved、等待 resumed turn 完成。
- [x] 补齐 app-server protocol case：订阅事件，触发同一链路，断言 notification 顺序与 event payload。
- [x] 断言 `session_id`、`turn_id`、`call_id` 在 pending / submit / completed 链路中一致。
- [x] 将新 case 接入 `tests/integration/run_e2e.sh`，但只在 Phase 4 最终收口时运行全量 e2e。
- [x] 在失败时输出 event log artifact，便于定位 runtime / transport 边界问题。

---

## 5. 成功标准

- CLI first 集成 case 覆盖 pending approval 到 resumed turn 的完整链路。
- app-server protocol case 覆盖同一链路的 JSON-RPC event sequence。
- 校验 `session_id`、`turn_id`、`call_id`、event sequence 一致。
- 校验 approval rejected / approved 两类路径至少覆盖 approved 主链路。
- 新增回归已接入 `tests/integration/run_e2e.sh`。

---

## 6. 建议测试位置

- `tests/integration/scenarios/06_tools/cli_tool_approval_resume.sh`
- `tests/integration/scenarios/09_app_server/tool_approval_resume.sh`

---

## 7. 局部验证

```text
source .env && tests/integration/scenarios/06_tools/cli_tool_approval_resume.sh
source .env && tests/integration/scenarios/09_app_server/tool_approval_resume.sh
source .env && tests/integration/protocol_smoke.sh
```

全量 `source .env && tests/integration/run_e2e.sh` 留到 Phase 4 最后统一执行。

---

## 8. 完成记录

**变更摘要**: 已补齐真实 Bash pending approval -> submit approval -> server-side resume -> approved tool completed -> message completed 链路。修复了 approval response 固定写入 `ReactiveLoop` 导致跨 branch 恢复不可见的问题；application-service 的 approval / dynamic result submit 改为通过 gateway stream 恢复并写入 event service；replay tool result 进入统一 stream event；CLI 与 app-server protocol 新增窄集成 case。  

**关键文件路径**:
- `talos-rs/crates/orchestration/agent/src/agent/session.rs`
- `talos-rs/crates/orchestration/agent-kernel/src/processor/request_kinds.rs`
- `talos-rs/crates/orchestration/agent-kernel/src/flow/single.rs`
- `talos-rs/crates/orchestration/application-service/src/lib.rs`
- `talos-rs/crates/orchestration/application-service/src/turn.rs`
- `talos-rs/crates/gateway/core/src/trait.rs`
- `talos-rs/crates/gateway/core/src/gateway.rs`
- `talos-rs/crates/orchestration/coordinator/src/coordinator.rs`
- `talos-rs/apps/app-server-test-client/src/main.rs`
- `talos-rs/apps/cli/src/cli.rs`
- `talos-rs/tests/integration/scenarios/06_tools/cli_tool_approval_resume.sh`
- `talos-rs/tests/integration/scenarios/09_app_server/tool_approval_resume.sh`

**已执行测试**:
- `cargo test -p talos-agent-kernel confirmation_processor -- --nocapture`
- `source .env && tests/integration/scenarios/09_app_server/tool_approval_resume.sh`
- `source .env && tests/integration/scenarios/06_tools/cli_tool_approval_resume.sh`
- `source .env && tests/integration/protocol_smoke.sh`

**未覆盖风险**: 当前主链路覆盖 approved path；rejected path 仍由既有 unit 级处理覆盖，未新增真实集成 case。approval resume 自动恢复阶段会直接完成工具执行 turn，不再让模型基于空输入继续生成业务内容。  

**后续移交事项**: Phase 4 统一跑全量 `tests/integration/run_e2e.sh`，确认新增 case 在完整 e2e 编排中稳定。  
