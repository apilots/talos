# P2 Phase 4: Final Hardening

**状态**: 已完成  
**目标**: P2 一次性全量回归、文档收口、风险复盘。  
**输出**: P2 可进入 CR 的完整验证记录。  

---

## 1. 范围

- 确认 Phase 1-3 均已完成并回填完成记录。
- 清理 P2 期间产生的临时兼容路径。
- 复核协议 schema、TS generated fixture、SDK helper 和 integration fixture 无漂移。
- 只在本阶段最后运行一次全量 `tests/integration/run_e2e.sh`。

---

## 2. 非目标

- 不新增业务功能。
- 不扩展新的协议 method。
- 不在 e2e 失败时绕过测试或新增 fallback。

---

## 3. 子任务

- [x] 复核 `DEFERRED_CLIENT_METHODS` 不再保留 P2 目标 method。
- [x] 复核 pending approval resume case 已接入 CLI first 与 JSON-RPC protocol smoke。
- [x] 复核 Desktop crash recovery smoke 已覆盖 lifecycle 与 replay。
- [x] 运行 P2 全局质量门禁。
- [x] 回填 Phase 1-4 完成记录。
- [x] 更新必要 spec / README / test docs。

---

## 4. 全量验证

按以下顺序执行，`run_e2e.sh` 放在最后且本迭代只跑一次：

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

---

## 5. 成功标准

- P2 目标 method 已 freeze，并从 deferred list 移除。
- pending approval resume 真实链路有 CLI first 和 JSON-RPC protocol 覆盖。
- Desktop app-server crash recovery 有产品级 smoke 覆盖。
- 全量质量门禁通过。
- P2 文档完成记录完整，无未关联 TODO。

---

## 6. 完成记录

**变更摘要**: 已完成 P2 收口复核和全量验证。最终 e2e 初次暴露 `session_close_flushes_memory` 在 transcript-only session 下无法生成 workspace memory entry；已在 memory store 层修复 prompt context 构建逻辑，使无 `TalosTurn` artifact 时复用 transcript record，不新增重复业务模型。修复后定点场景和最终全量 e2e 均通过。  

**关键文件路径**:
- `spec/plan/p2/PLAN.md`
- `spec/plan/p2/phase-1-freeze-deferred-protocol-methods.md`
- `spec/plan/p2/phase-2-pending-approval-resume-regression.md`
- `spec/plan/p2/phase-3-desktop-crash-recovery-smoke.md`
- `spec/plan/p2/phase-4-final-hardening.md`
- `talos-rs/crates/cognition/memory/src/session_store.rs`
- `talos-rs/crates/cognition/memory/src/store/tests.rs`
- `talos-rs/tests/integration/reports/e2e_report_20260501_082252.md`

**已执行测试**:
- `cargo test -p talos-app-protocol`
- `cargo test -p talos-application-service`
- `cargo test -p talos-app-server`
- `cargo test -p talos-client`
- `set -a; source .env; set +a; tests/integration/protocol_smoke.sh`
- `pnpm --dir talos-app --filter @talos/sdk-core test`
- `set -a; source talos-rs/.env; set +a; TALOS_APP_SERVER_BIN=/Users/xujiang/data/ailabs/talos/talos-rs/target/debug/talos pnpm --dir talos-app --filter @talos/sdk-app-server test:integration`
- `pnpm --dir talos-app test`
- `pnpm --dir talos-app typecheck`
- `cargo test -p talos-memory prompt_context_falls_back_to_transcript_records -- --nocapture`
- `cargo test -p talos-memory`
- `set -a; source .env; set +a; tests/integration/scenarios/07_memory/session_close_flushes_memory.sh`
- `set -a; source .env; set +a; tests/integration/run_e2e.sh` -> PASS, report `talos-rs/tests/integration/reports/e2e_report_20260501_082252.md`

**未覆盖风险**: 根目录无 `.env`，本地验证在 `talos-rs` 目录使用 `.env`，跨目录 TS integration 使用 `set -a` 导出环境变量并显式指定 `TALOS_APP_SERVER_BIN`。首次全量 e2e 失败已定位并修复；最终全量 e2e 通过。  

**后续移交事项**: P2 可进入 CR；后续若要给 Desktop 单独定义 `desktop` source/provider，应作为新协议/业务枚举变更独立评审。  
