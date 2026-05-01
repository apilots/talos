# P2 Phase 3: Desktop App-server Crash Recovery Smoke

**状态**: 已完成  
**目标**: 补齐 Electron runtime 下 app-server 子进程异常退出后的产品级 smoke。  
**输出**: Desktop controller、typed IPC、TS SDK app-server replay 链路的 smoke 覆盖。  

---

## 1. 背景

Desktop controller 已有 restart + subscription replay 单测，但仍缺产品级 smoke，覆盖 Electron runtime 下 app-server 子进程异常退出后的重连体验。

该事项不扩展服务端业务能力，只验证 Desktop lifecycle 和订阅 replay 体验。

---

## 2. 范围

- 覆盖 Electron runtime 下 main/controller 管理 app-server 子进程。
- 覆盖 renderer 通过 typed IPC / TS SDK 继续消费 app-server 事件。
- 覆盖 last seen event id replay，不要求 Desktop 承担服务端业务恢复逻辑。

---

## 3. 非目标

- 不让 renderer 直接访问 Node / filesystem。
- 不在 Desktop main process 里复制 application-service 或 SDK 业务模型。
- 不和 dynamic tool result protocol freeze 合并。
- 不在本阶段运行全量 `tests/integration/run_e2e.sh`。

---

## 4. 子任务

- [x] 确认现有 controller restart 单测和 app-server integration fixture。
- [x] 增加 smoke runner：启动 Desktop main/controller，创建 session，订阅事件。
- [x] 终止 app-server child process，断言 lifecycle 进入 `restarting` 后回到 `ready`。
- [x] 使用 last seen event id 重新订阅，断言 renderer 可继续收到后续事件。
- [x] 将 lifecycle 与 event replay 记录到测试 artifact，便于 CI 失败定位。

---

## 5. 成功标准

- 启动 desktop main/controller。
- 创建 session 并订阅事件。
- 模拟或终止 app-server child process。
- controller 自动重启 app-server。
- 使用 last seen event id replay subscription。
- renderer 继续收到后续事件。
- 测试记录 app-server lifecycle 状态变化：starting / ready / restarting / ready。

---

## 6. 局部验证

```text
pnpm --dir talos-app --filter @talos/sdk-app-server test:integration
pnpm --dir talos-app test
pnpm --dir talos-app typecheck
```

全量 `source .env && tests/integration/run_e2e.sh` 留到 Phase 4 最后统一执行。

---

## 7. 完成记录

**变更摘要**: 已补齐 Desktop app-server crash recovery smoke。单测覆盖 controller restart、renderer SDK event bridge、last seen event id replay 和 artifact 输出；gated integration smoke 使用真实 stdio app-server 子进程，SIGKILL 后验证 controller 自动重启、订阅 replay 使用 last event id、renderer SDK 继续收到后续事件。  

**关键文件路径**:
- `talos-app/desktop/src/app-server-controller.test.ts`
- `talos-app/desktop/src/app-server-controller.integration.test.ts`
- `talos-app/ts-sdk/packages/app-server/src/stdio.ts`
- `test-results/desktop/app-server-crash-recovery-smoke.json`
- `test-results/desktop/real-app-server-crash-recovery-smoke.json`

**已执行测试**:
- `pnpm --dir talos-app --filter talos-desktop test -- src/app-server-controller.test.ts`
- `pnpm --dir talos-app --filter talos-desktop test`
- `pnpm --dir talos-app --filter talos-desktop lint`
- `pnpm --dir talos-app --filter talos-desktop typecheck`
- `pnpm --dir talos-app --filter @talos/sdk-app-server typecheck`
- `set -a; source talos-rs/.env; set +a; TALOS_APP_SERVER_BIN=/Users/xujiang/data/ailabs/talos/talos-rs/target/debug/talos pnpm --dir talos-app --filter @talos/sdk-app-server test:integration`
- `set -a; source talos-rs/.env; set +a; TALOS_DESKTOP_APP_SERVER_INTEGRATION=1 TALOS_APP_SERVER_BIN=/Users/xujiang/data/ailabs/talos/talos-rs/target/debug/talos pnpm --dir talos-app --filter talos-desktop test -- src/app-server-controller.integration.test.ts`
- `pnpm --dir talos-app test`
- `pnpm --dir talos-app typecheck`

**未覆盖风险**: Desktop runtime smoke 复用当前 app-server 支持的 `cli` source/provider 创建 session；服务端尚未定义独立 `desktop` source/provider 业务枚举，本阶段未扩展该业务语义。  

**后续移交事项**: Phase 4 统一执行全局质量门禁和最终 `tests/integration/run_e2e.sh`。  
