# P2 App Protocol Completion Iteration Plan

**状态**: 已完成  
**创建时间**: 2026-04-30  
**来源**: P1 CR follow-up issues  
**关联 P1**: [../p1/PLAN.md](../p1/PLAN.md)  

---

## 1. 目标

P2 用一个独立迭代一次性收敛 P1 CR 后遗留的 3 类问题：

1. freeze deferred app protocol methods。
2. 补齐真实 pending approval resume 回归。
3. 补齐 Desktop app-server crash recovery 产品级 smoke。

本迭代继续遵守 P1 已冻结的分层边界：HTTP/SSE 与 JSON-RPC 共享 application-service；协议层只做 transport 编码、解码、错误映射和订阅分发；SDK 只做薄封装。

---

## 2. 阶段索引

| Phase | 子计划 | 目标 | 状态 |
|---:|---|---|---|
| 1 | [phase-1-freeze-deferred-protocol-methods.md](phase-1-freeze-deferred-protocol-methods.md) | freeze `tool.dynamic_result.submit`、`admin.session.list`、`admin.project.list` | 已完成 |
| 2 | [phase-2-pending-approval-resume-regression.md](phase-2-pending-approval-resume-regression.md) | 覆盖真实 pending approval resume 链路 | 已完成 |
| 3 | [phase-3-desktop-crash-recovery-smoke.md](phase-3-desktop-crash-recovery-smoke.md) | 覆盖 Desktop app-server crash recovery 产品级 smoke | 已完成 |
| 4 | [phase-4-final-hardening.md](phase-4-final-hardening.md) | 一次性全量回归、文档收口、风险复盘 | 已完成 |

---

## 3. 执行顺序

```text
P2 Phase 1 Freeze Deferred Protocol Methods
    |
    v
P2 Phase 2 Pending Approval Resume Regression
    |
    v
P2 Phase 3 Desktop Crash Recovery Smoke
    |
    v
P2 Phase 4 Final Hardening
```

说明：

- Phase 1 先完成协议面 freeze，避免后续测试继续围绕 deferred method 写临时判断。
- Phase 2 优先覆盖 runtime 正确性风险，CLI first 与 JSON-RPC event sequence 必须同时验证。
- Phase 3 聚焦 Desktop lifecycle 和 typed IPC，不扩展服务端业务能力。
- Phase 4 只做收口和最终验证，不再新增功能。

---

## 4. 全局执行规则

1. 每个 phase 开始前，先把对应 subplan 状态改为 `进行中`。
2. 协议、SDK、CLI、Desktop 的新增结构体必须复用 Rust app protocol 或 application-service 的强类型定义，禁止重新定义业务 DTO。
3. JSON-RPC / HTTP 只承担 transport 编码、解码、错误映射和订阅分发，不承载业务语义。
4. 错误类型优先复用 common / protocol 既有抽象；如业务错误未覆盖，先扩展共享错误模型。
5. 不引入 `serde_json::Value` / loose metadata 承载核心语义。
6. 单个源码文件超过 1000 行时，必须在当前 phase 内拆分。
7. 每个 phase 只跑相关快速验证；`tests/integration/run_e2e.sh` 只在 Phase 4 最后跑一次。

---

## 5. 全局质量门禁

Phase 1-3 按各 subplan 的局部验证命令执行。P2 全量验证只在 Phase 4 执行一次：

```text
cargo test -p talos-app-protocol
cargo test -p talos-application-service
cargo test -p talos-app-server
cargo test -p talos-client
source .env && tests/integration/protocol_smoke.sh
pnpm --dir talos-app --filter @talos/sdk-core test
pnpm --dir talos-app --filter @talos/sdk-app-server test:integration
pnpm --dir talos-app test
pnpm --dir talos-app typecheck
source .env && tests/integration/run_e2e.sh
```

`source .env && tests/integration/run_e2e.sh` 必须放在最后执行，且本迭代只执行一次。

---

## 6. 完成记录要求

每个 subplan 的完成记录必须包含：

- 变更摘要
- 关键文件路径
- 已执行测试
- 未覆盖风险
- 后续移交事项

Phase 4 完成后，P2 不应再保留 P1 CR follow-up issue。
