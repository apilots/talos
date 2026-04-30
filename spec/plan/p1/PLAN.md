# Desktop App Server Shared Application Service Implementation Plan

**状态**: 已完成  
**最后更新**: 2026-04-30  
**关联 spec**: [desktop-appserver-shared-application-service-design.md](../../desktop-appserver-shared-application-service-design.md)

---

## 1. 计划目标

本计划把 Desktop App Server 与 `talos-application-service` 共享层拆成可独立执行、可独立 review 的阶段计划。总计划只维护阶段顺序、依赖关系和全局质量门禁；每个阶段的具体任务、测试、风险和完成定义放在独立 sub plan 中。

核心目标：

1. HTTP/SSE 与 JSON-RPC app-server 收敛到同一个 `talos-application-service`。
2. `coordinator` 收缩为下层 agent 协调组件。
3. 现有 HTTP 中可共享的 resource access / application service 逻辑迁入共享层。
4. 新增 `crates/gateway/app-server` 与 `crates/gateway/app-protocol`。
5. Web UI 与 Desktop 共享 `TalosClient`、frontend domain model、event reducer 和主要 UI 组件。

---

## 2. 阶段索引

| Phase | 子计划 | 目标 | 状态 |
|---:|---|---|---|
| 0 | [phase-0-contract-baseline.md](phase-0-contract-baseline.md) | 冻结关键决策、接口契约、测试基线 | 已完成 |
| 1 | [phase-1-application-service.md](phase-1-application-service.md) | 提取 `talos-application-service`，瘦身 `coordinator` | 已完成 |
| 2 | [phase-2-app-protocol.md](phase-2-app-protocol.md) | 建立强类型 JSON-RPC app protocol 与生成物 | 已完成 |
| 3 | [phase-3-app-server.md](phase-3-app-server.md) | 实现 stdio JSON-RPC `talos app-server` | 已完成 |
| 4 | [phase-4-cli-tui-and-parity.md](phase-4-cli-tui-and-parity.md) | 建立 CLI/TUI 本地验证入口与 transport 等价测试 | 已完成 |
| 5 | [phase-5-ts-sdk-and-desktop.md](phase-5-ts-sdk-and-desktop.md) | 扩展 TS SDK transport，接入 Desktop 与共享 UI | 已完成 |
| 6 | [phase-6-hardening-and-rollout.md](phase-6-hardening-and-rollout.md) | 全量回归、文档收口、清理临时兼容路径 | 已完成 |
| 7 | [phase-7-boundary-convergence.md](phase-7-boundary-convergence.md) | CR 前分层归位、server 统一出口、SDK 薄封装收敛 | 已完成 |

---

## 3. 阶段依赖

```text
Phase 0 Contract Baseline
    |
    v
Phase 1 Application Service
    |
    +----------------------+
    |                      |
    v                      v
Phase 2 App Protocol   Phase 4 (skeleton only)
    |                      |
    v                      |
Phase 3 App Server <------+
    |
    v
Phase 4 (full) CLI/TUI and Transport Parity
    |
    v
Phase 5 TS SDK and Desktop
    |
    v
Phase 6 Hardening and Rollout
    |
    v
Phase 7 Server and SDK Boundary Convergence
```

说明：

- Phase 0 必须先完成，避免后续实现反复改名、改事件语义、改目录。
- Phase 1 是主干重构，后续所有入口都依赖 `talos-application-service`。
- Phase 2 与 Phase 1 可以部分并行，但协议类型不得绕开 Phase 1 的 application-service 参数模型。
- Phase 3 依赖 Phase 1 与 Phase 2。
- Phase 4 编号只代表一个阶段。图中的 `Phase 4 (skeleton only)` 表示 §4.3 transport parity contract 的测试骨架可在 Phase 2 后先开工；`Phase 4 (full)` 表示 CLI/TUI 验证和 contract 完整通过依赖 Phase 3。
- Phase 5 依赖 Phase 3 的 app-server 和 Phase 4 的 client contract。
- Phase 6 只做收口，不引入新的架构方向。
- Phase 7 是 CR 前收敛阶段，用于修正 Phase 1-6 落地后暴露出的目录归位、server 统一出口和 SDK 边界问题；不引入新的业务能力。

---

## 4. 全局执行规则

1. 每个 phase 开始前，先把对应 sub plan 的状态改为 `进行中`。
2. 每完成一个子任务，更新 sub plan 中的 checklist。
3. 每个 phase 必须保持代码可编译、相关测试通过。
4. 不允许为了迁移新增长期 fallback 逻辑。
5. 不允许在协议层或配置层引入任意 metadata JSON 承载核心语义。
6. 新增 enum 不使用裸 `String` 表达业务分支。
7. 单个源码文件超过 1000 行时，必须在当前 phase 内拆分。
8. 临时兼容路径必须登记到每个 sub plan 完成记录的 `遗留临时路径（Phase 6 待删）` 字段，并在 Phase 6 前删除。

---

## 5. 全局质量门禁

所有阶段完成后至少执行。`tests/integration/run_e2e.sh` 只在 Phase 6 hardening / rollout 阶段执行，不作为 Phase 0-5 的每阶段必跑项。

```text
cargo test -p talos-app-protocol
cargo test -p talos-application-service
cargo test -p talos-coordinator
cargo test -p talos-app-server
cargo test -p talos-gateway-http
cargo test -p talos-client
source .env && tests/integration/run_e2e.sh
source .env && tests/integration/protocol_smoke.sh
```

如果前端接入：

```text
pnpm --dir talos-app lint
pnpm --dir talos-app test
pnpm --dir talos-app typecheck
pnpm --dir talos-app build
```

如果改动 app protocol 或 TS app-server SDK，还需执行：

```text
cargo run -p talos-app-protocol --example export_fixtures
pnpm --dir talos-app --filter @talos/sdk-app-server test:integration
```

---

各 phase 文档只列该阶段新增或重点测试；全量测试命令以本节为唯一来源。

## 6. 状态更新约定

状态取值：

- `未开始`
- `进行中`
- `阻塞`
- `已完成`

每个 sub plan 的完成记录必须包含：

- 变更摘要
- 关键文件路径
- 已执行测试
- 遗留临时路径（Phase 6 待删）
- 未覆盖风险
- 后续 phase 需要接手的事项

Phase 6 收口后仍 deferred 的协议方法必须在完成记录中写明 method、原因与 owner，不允许只以“后续处理”笼统描述。

CR 合并后的非阻塞后续事项记录在 [cr-follow-up-issues.md](cr-follow-up-issues.md)。
