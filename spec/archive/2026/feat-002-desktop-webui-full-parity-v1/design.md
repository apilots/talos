# Desktop v1 与 WebUI 能力对齐方案

**状态**: v1 能力落地中  
**最后更新**: 2026-05-05

## 1. 目标

Desktop v1 的目标不是复制一套前端或后端，而是在 Electron 壳内复用现有 WebUI 与共享应用内核，让桌面端具备 WebUI 主路径能力：

- 复用 WebUI 页面、状态、事件 reducer、feature API。
- 复用 `talos-application-service`，HTTP 与 app-server 只做协议适配。
- Desktop renderer 只通过 `window.talos` IPC bridge 调用本地 app-server。
- 新能力优先补 app-server protocol，不在页面层写 Desktop 专用业务分支。

## 2. v1 对齐范围

### 2.1 必须对齐

- Session: create/list/read/patch/close/delete/turns/cancel。
- Message: send/sendEvents/retry/retryEvents。
- Runtime events: replay/live stream/tool approval/dynamic tool result。
- Projects: list/get/create/patch/delete/admin list。
- Memory: list/read/write/search。
- Workspace docs: list/read/write。
- Models and LLM config: list/read/write/reload。
- Account: current user for local desktop actor。
- Capability list: tool list backed by real `ToolRegistry` metadata。
- Build/runtime: Desktop build reuses WebUI bundle; Electron main owns app-server lifecycle。

### 2.2 Desktop 本地模式差异

以下能力在 Desktop v1 不做 HTTP 形态照搬：

- Browser login/register/logout: Desktop 使用 local launch token 初始化本地 actor。
- Admin user create/patch/delete/token management: 这是 remote HTTP server 的多用户治理能力，Desktop v1 暂不暴露写操作。
- `skill.list` / `app.list`: 当前没有独立 skill/app registry，协议保留并返回空集合；真实可执行能力以 `tool.list` 为准。

这类差异必须显式存在于 app-server adapter 层，不允许页面业务绕路实现。

## 3. 架构边界

```text
Web shell
  -> createBrowserTalosClient
  -> HTTP/SSE

Desktop shell
  -> window.talos
  -> createElectronAppServerClient
  -> JSON-RPC stdio app-server

Shared WebUI
  -> getTalosClient()
  -> unified TalosWebClient shape

Shared application core
  -> talos-application-service
  -> session/memory/workspace/config/event/tool registry
```

## 4. 关键改造

### 4.1 Protocol parity

app-server protocol 必须补齐 WebUI 主路径依赖的方法：

- `session.compact`
- `session.clear`
- `session.active_turn`
- `turn.retry`
- `tool.list`

这些方法直接映射共享 application service 或真实 `ToolRegistry`，不新增第二套桌面业务逻辑。

### 4.2 Desktop client adapter

`createDesktopTalosClient()` 必须保持与 browser client 同形：

- `sessions.compact/clear/activeTurn` 直接调用 app-server client。
- `messages.retry/retryEvents` 通过 `turn.retry` 与 `event.subscribe` 组合。
- `admin.llm.update` 通过 `config.value.write` 写入完整 `llm` 配置。
- `user.current` 通过 `account.get` 获取 local actor。

### 4.3 Capability source

`tool.list` 不能返回占位空数组，必须读取运行时实际注册的 `ToolRegistry`：

- tool name / description / schema
- source / visibility / payload kind
- behavior flags / result policy
- mutating / parallelism

`skill.list` 与 `app.list` 在没有独立 registry 前可以返回空集合，但文档和 adapter 必须明确这是产品范围差异，不是方法缺失。

## 5. 验收标准

- Desktop renderer 可加载 WebUI bundle。
- Web 模式仍使用 browser HTTP/SSE client。
- Desktop 模式自动使用 `window.talos` app-server client。
- WebUI 主路径不再出现 Desktop unsupported：message retry、session compact/clear/activeTurn、tool list。
- `cargo test -p talos-app-protocol` 通过。
- `cargo test -p talos-app-server` 通过。
- `cargo check -p talos-server` 通过。
- `pnpm --filter talos-web typecheck` 通过。
- `pnpm --filter talos-desktop check` 通过。
- `pnpm --filter @talos/sdk-app-server test` 通过。

## 6. 后续非 v1 项

- Electron packaging / signing / updater。
- Desktop runtime state UI: app-server starting/ready/restarting/error。
- 本地账号资料编辑与多用户管理。
- 独立 skill/app registry 接入。
- Desktop full smoke/e2e 场景固化。
