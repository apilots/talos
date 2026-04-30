# Phase 5: TS SDK and Desktop

**状态**: 已完成  
**目标**: 扩展 `talos-app/ts-sdk` transport，建立 Desktop shell，并复用 Web UI 领域模型和主要组件。  
**依赖**: Phase 3 app-server；Phase 4 client contract 与 parity tests。  
**输出**: `StdioAppServerClient`、Electron IPC bridge、Desktop smoke。

---

## 1. 范围

Phase 5 启动前必须先做 web 现状 spike：确认 `talos-app/web` 当前 reducer/store 结构是否已经支持抽出共享。如果不支持，需要单独立项；本阶段只做 transport 注入与已存在 store/domain 的迁移，不把 Web UI 重构扩大成本阶段主线。

Desktop actor credential 来源依赖 spec §17 的 Desktop 本地 auth 决策。执行 5.4 前必须关闭该开放问题，明确 credential 来自用户登录态、本地 keychain、配置文件或其他受控来源。

包含：

- 新增 `talos-app/ts-sdk/packages/app-server`。
- 保持 `core` 作为 runtime-neutral domain/client interface。
- Web 继续使用 `browser` HTTP/SSE transport。
- Desktop main process 管理 app-server lifecycle。
- Renderer 通过 typed IPC 调用统一 `TalosClient`。
- 共享 event reducer 和主要页面组件。

不包含：

- 重写 Web UI。
- 新增独立 Desktop 业务后端。
- 视觉大改版。

---

## 2. 子任务

### 5.1 Core contract 整理

**任务**:

- [x] 确认 `TalosClient` interface 覆盖 initialize/session/turn/event/config/model/tool approval。
- [x] 确认 frontend event envelope 包含 `subscriptionId`、`eventId`、`sessionId`、`turnId`。
- [x] 确认 event reducer 不依赖 HTTP/SSE 细节。
- [x] 建立 shared client contract tests。

**完成标准**:

- `browser` 与未来 `desktop` transport 可跑同一组 contract tests。

### 5.2 App-server transport package

**任务**:

- [x] 新增 `talos-app/ts-sdk/packages/app-server`。
- [x] 实现 child process spawn primitives。
- [x] 实现 JSONL reader/writer。
- [x] 实现 JSON-RPC request id 管理。
- [x] 实现 notification dispatch。
- [x] 实现 stdout/stderr 分离。
- [x] 实现 process exit / restart 信号。

**完成标准**:

- Node runtime 能直接连接 `talos app-server`。

### 5.3 Desktop transport adapter

**任务**:

- [x] 在 `talos-app/ts-sdk/packages/app-server` 内新增 desktop transport adapters。
- [x] 实现 `StdioAppServerClient`。
- [x] 实现 Electron main side client wrapper。
- [x] 实现 renderer preload IPC bridge contract。
- [x] Renderer 不暴露 Node 权限。
- [x] Renderer 不直接读写本地文件系统。

**完成标准**:

- Renderer 只看到统一 `TalosClient`。

### 5.4 Desktop shell

**任务**:

- [x] 建立 Desktop app 基础工程或接入现有 desktop 目录。
- [x] Main process 启动 app-server。
- [x] Main process 按已关闭的 Desktop auth 决策传入 actor credentials。
- [x] app-server 崩溃后可重启。
- [x] 重启后 renderer 重新 initialize。
- [x] 按 last seen event id 重新订阅 session events。

**完成标准**:

- Desktop smoke 可创建 session、start turn、接收事件。

### 5.5 Web/Desktop UI 共享

**任务**:

- [x] Web runtime 注入 `HttpSseTalosClient`。
- [x] Desktop runtime 注入 `ElectronAppServerClient`。
- [x] 共享 session store。
- [x] 共享 active session store。
- [x] 共享 turn lifecycle reducer。
- [x] 共享 tool approval state。
- [x] 共享 config/model cache。

**完成标准**:

- UI 组件不直接 import transport 包。

---

## 3. 测试

本阶段重点新增测试：

- `TalosClient` contract tests。
- `StdioAppServerClient` transport tests。
- Electron IPC bridge tests。
- shared event reducer tests。
- Desktop smoke。
- Playwright Web UI 回归。

全量质量门禁见 [PLAN.md §5](PLAN.md#5-全局质量门禁)。

---

## 4. 风险

| 风险 | 处理 |
|---|---|
| Desktop 绕开 app-server | Main process 只做 lifecycle 和 IPC，不承载业务 |
| TS SDK 平行体系 | 共享内核放入 `@talos/sdk-core`，`@talos/sdk-app-server` 只承载 JSON-RPC transport adapter |
| Renderer 权限过大 | 所有本地能力通过 preload 暴露的 typed IPC |
| Web 与 Desktop reducer 分裂 | event envelope 与 reducer 放在共享 core/domain |

---

## 5. 完成记录

**变更摘要**:  
已新增统一 TS app-server JSON-RPC client 包 `@talos/sdk-app-server`，共享方法内核 `AppServerClient` 已放入 `@talos/sdk-core`；stdio、desktop main、desktop renderer 只是不同 transport adapter。原 browser HTTP/SSE client 已改为明确的 `@talos/sdk-browser`，避免本地 JSON-RPC SDK 与 HTTP SDK 混淆；renderer 侧只依赖 `DesktopRendererBridge`，不暴露 Node 或文件系统能力。已新增 `talos-desktop` Electron shell，main process 只负责 app-server lifecycle、typed IPC、crash restart 与 event replay；Web client 创建点已支持 runtime factory 注入。

**关键文件**:  
- `talos-app/ts-sdk/packages/core/src/app-server.ts`
- `talos-app/ts-sdk/packages/app-server/src/stdio.ts`
- `talos-app/ts-sdk/packages/app-server/src/desktop.ts`
- `talos-app/ts-sdk/packages/app-server/src/desktop-main.ts`
- `talos-app/desktop/src/main.ts`
- `talos-app/desktop/src/preload.ts`
- `talos-app/desktop/src/app-server-controller.ts`
- `talos-app/web/src/shared/api/client.ts`
- `talos-app/ts-sdk/packages/core/src/generated/app-protocol.ts`
- `talos-app/ts-sdk/packages/browser/package.json`

**测试结果**:  
- `cargo test -p talos-client` PASS
- `cargo test -p talos-client -p talos-gateway-http -p talos-coordinator` PASS
- `cargo check -p talos-cli -p talos-ui-tui -p talos-app-server-test-client` PASS
- `pnpm --dir talos-app --filter @talos/sdk-app-server test` PASS
- `source .env && cd ../talos-app && TALOS_APP_SERVER_BIN=/Users/xujiang/data/ailabs/talos/talos-rs/target/debug/talos TALOS_APP_SERVER_CWD=/Users/xujiang/data/ailabs/talos/talos-rs pnpm --filter @talos/sdk-app-server test:integration` PASS
- `pnpm --dir talos-app --filter talos-desktop test` PASS
- `pnpm --dir talos-app --filter talos-desktop build` PASS
- `pnpm --dir talos-app lint` PASS
- `pnpm --dir talos-app test` PASS
- `pnpm --dir talos-app build` PASS
- `pnpm --dir talos-app typecheck` PASS
- `pnpm --dir talos-app --filter @talos/sdk-app-server lint` PASS
- `source .env && tests/integration/run_e2e.sh --case 05_cli/app_server_e2e` PASS
- `source .env && tests/integration/run_e2e.sh --case 09_app_server/protocol_e2e` PASS

**遗留临时路径（Phase 6 待删）**:  
- 无新增长期 fallback。

**未覆盖风险**:  
- Desktop renderer 已具备 typed bridge 与 runtime 注入点；`project.*` 与 `session.turns` parity 已在 Phase 6 补齐，剩余限制集中在 admin 与 dynamic tool result deferred methods。
- Web/Desktop 当前通过复用同一 Web UI store 与 reducer 达成共享；后续若拆成独立共享 UI 包，需保持不引入第二套状态机。

**后续移交**:  
- 后续补齐 admin / dynamic tool result protocol freeze 后，Desktop renderer 可继续扩大完整 Web feature API 的 local app-server 覆盖面；Desktop crash restart 当前有 controller 单测，仍需产品级 smoke 复核。
