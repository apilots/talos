# Phase 0: Contract Baseline

**状态**: 已完成  
**目标**: 在编码前冻结关键架构决策、接口契约和测试基线，减少后续大规模返工。  
**依赖**: 已完成架构 spec review 的主要决策。  
**输出**: 可执行的接口边界、事件语义、目录命名、测试基线。

---

## 1. 范围

本阶段不做业务实现，只做实现前的契约冻结和基线检查。

包含：

- 确认 `talos-application-service` 与 `coordinator` 的边界。
- 确认 `gateway` 目录语义和 `app-server` 放置位置。
- 确认 JSON-RPC 2.0、JSONL framing、事件 canonical id。
- 确认 event persistence before fan-out 的事件顺序规则。
- 确认首批 turn 并发模型。
- 确认 app-server actor 注入方式。
- 建立当前测试基线记录。

不包含：

- 新建 Rust crate。
- 修改 HTTP handler。
- 实现 JSON-RPC parser。
- 修改前端 SDK。

---

## 2. 子任务

### 0.1 决策清单冻结

**目标**: 把实现必须依赖的决策从 spec 中抽取为 checklist。

**任务**:

- [x] 确认 `talos-application-service` 是 HTTP 与 app-server 的唯一共享应用入口。
- [x] 确认 `coordinator` 只输出 `RawAgentEvent`，不输出客户端最终事件。
- [x] 确认 event service 负责 event id、turn id、session id、持久化、fan-out、replay。
- [x] 确认事件必须先持久化再 fan-out。
- [x] 确认每 session 单 active turn。
- [x] 确认 `event.subscribe` 是 RPC ack + notification。
- [x] 确认 per-subscription bounded channel 与 `subscription.terminated` 语义。
- [x] 确认 app-server actor 来自 `initialize.actor_credentials`。
- [x] 确认 `crates/gateway/app-server` 与 `crates/gateway/app-protocol` 目录。

**完成标准**:

- spec 与本 plan 对上述决策无冲突表述。

### 0.2 现有代码边界盘点

**目标**: 标出 Phase 1 需要迁移的现有代码。

**任务**:

- [x] 盘点 `crates/orchestration/coordinator` 中 session / turn / project / approval 相关职责。
- [x] 盘点 `gateway/http/src/server` 中可迁移到 application-service 的 service 逻辑。
- [x] 盘点 `crates/app/talos-server` 当前 composition root 组装逻辑。
- [x] 盘点 `talos-app/ts-sdk/packages/core` 与 `browser` 现有 client/domain 类型。

**完成标准**:

- Phase 1、Phase 4、Phase 5 子计划能引用具体迁移候选模块。

### 0.3 测试基线记录

**目标**: 在大重构前记录当前可运行的 crate 级测试集合。全量 e2e 只在 Phase 6 执行。

**任务**:

- [x] 运行 coordinator 相关测试，记录结果。
- [x] 运行 HTTP gateway 相关测试，记录结果。
- [x] 如存在已知失败，记录失败命令、失败原因、是否与本需求相关。

**建议命令**:

```text
cargo test -p talos-coordinator
cargo test -p talos-gateway-http
```

**完成标准**:

- 后续 phase 能区分新引入失败与既有失败。

---

## 3. 输出物

Phase 0 完成后必须留下可被后续 phase 引用的文件：

- [x] `spec/plan/p1/phase-0/decision-checklist.md`：决策快照，避免 spec 后续微调时基线丢失。
- [x] `spec/plan/p1/phase-0/baseline-test-report.md`：基线测试命令、结果、失败原因。
- [x] `spec/plan/p1/phase-0/migration-inventory.md`：现有代码盘点表，列出 Phase 1 迁移候选模块。

这些文件只记录基线和事实，不承载新架构决策；新决策仍应先更新 spec。

---

## 4. 风险

| 风险 | 处理 |
|---|---|
| 决策未冻结就开工 | 停止进入 Phase 1，先更新 spec 和本计划 |
| 现有测试本来失败 | 记录为 baseline，不在后续 phase 中误判 |
| 目录命名反复 | Phase 0 关闭后不再改目录，除非重新 review |

---

## 5. 完成记录

**变更摘要**:  
冻结 Phase 0 契约决策，完成 coordinator / HTTP gateway / talos-server / TS SDK 迁移候选盘点，记录重构前测试基线。

**关键文件**:  
`spec/plan/p1/phase-0/decision-checklist.md`  
`spec/plan/p1/phase-0/baseline-test-report.md`  
`spec/plan/p1/phase-0/migration-inventory.md`

**测试结果**:  
`cargo test -p talos-coordinator`：通过，16 passed。  
`cargo test -p talos-gateway-http`：通过，66 passed。

**遗留临时路径（Phase 6 待删）**:  
无。

**未覆盖风险**:  
Phase 0 未执行全量 e2e；按总计划仅在 Phase 6 执行。Phase 0 未创建新 crate，因此未运行后续阶段才会出现的 crate 测试。

**后续移交**:  
Phase 1 从 `gateway/http/src/server/access.rs` 和 `coordinator/src/session.rs` 开始抽取共享 application service 边界；事件基础复用 `talos-session-store::SessionEventService`。
