# Desktop 与 Web UI 共享运行时落地方案

**状态**: 初版已落地  
**最后更新**: 2026-05-05

## 1. 目标

Desktop 与 Web UI 不做两套产品逻辑。两种形态只保留运行壳差异：

- Web shell: 浏览器、HTTP/SSE、浏览器存储、登录态、URL 路由。
- Desktop shell: Electron window、preload IPC、app-server 子进程、本地生命周期、崩溃重启。
- Shared frontend core: 页面组件、状态机、事件 reducer、tool approval、memory/files/projects/settings 领域状态。
- Shared application core: `talos-application-service`，由 HTTP 与 JSON-RPC app-server 两个 adapter 共同调用。

本方案的目标是让 Desktop renderer 复用现有 Web UI 主体，通过 runtime client injection 切换到本地 app-server。

## 2. 当前基础

当前代码已经具备以下基础：

- `talos app-server` 已通过 `JSON-RPC over stdio JSONL` 暴露本地协议。
- `@talos/sdk-app-server` 已提供 `desktop-main` 与 renderer 侧 `createElectronAppServerClient`。
- `talos-app/desktop` 已有 Electron main / preload / IPC bridge，并能管理 app-server lifecycle。
- Web UI 已通过 `getTalosClient()` / `setTalosClientFactory()` 统一获取客户端实例。

当前缺口不是后端协议，而是 Web UI 与 Desktop runtime 的接入层还没有固定成共享边界。

## 3. 架构原则

1. 新能力默认进入 shared frontend core，Web shell 与 Desktop shell 只接入它。
2. 新后端能力默认进入 `talos-application-service`，HTTP 与 app-server 只做协议适配。
3. Desktop renderer 不直接访问 Node、文件系统或 Rust 内核。
4. Desktop main 只处理窗口、子进程、IPC、安全边界和本地生命周期。
5. 业务页面不判断 transport；只依赖统一 Talos client 形状。
6. 平台差异只能出现在 runtime bootstrap、client factory、lifecycle state 和平台动作入口。

## 4. 分层设计

```text
Web shell
  -> createBrowserTalosClient
  -> HTTP/SSE

Desktop shell
  -> window.talos preload bridge
  -> createElectronAppServerClient
  -> JSON-RPC stdio app-server

Shared Web UI
  -> getTalosClient()
  -> chat/session/memory/files/projects/settings
```

### 4.1 Runtime client 注入

Web UI 保留现有 `getTalosClient()` 入口。默认行为：

- 如果 `window.talos` 存在，说明运行在 Electron renderer，创建 Desktop app-server client adapter。
- 否则创建 Browser HTTP/SSE client。

这样页面、feature API、store 不需要感知运行壳。

### 4.2 Desktop client adapter

Desktop renderer 原生拿到的是 `AppServerClient` 方法集合，而 Web UI 现有代码依赖 `TalosClient` 的嵌套接口。需要一个薄 adapter：

- `models.list()` -> `appServer.listModels()`
- `sessions.*` -> `session.*` JSON-RPC
- `projects.*` -> `project.*` JSON-RPC
- `messages.sendEvents()` -> `turn.start` + `event.subscribe`
- `toolApprovals.submit()` -> `tool.approval.submit`
- `memory.*` / `workspaceDocs.*` -> 对应 app-server 方法

不在 adapter 内写业务规则，只做形状映射和事件格式转换。

### 4.3 Auth 与连接状态

Desktop 本地模式不走 Web HTTP auth bootstrap：

- Desktop health 直接返回 local open 状态。
- Desktop actor credentials 由 Electron main 初始化 app-server 时注入。
- UI 连接状态可显示 app-server `starting / ready / restarting / error`，但该状态不进入业务 reducer。

## 5. 首轮落地范围

本轮只做最小可用闭环：

1. 增加 Desktop runtime client adapter。
2. Web client factory 自动识别 `window.talos`。
3. Desktop 构建脚本确保 Web UI bundle 可被 Electron 加载。
4. 保留 Web HTTP/SSE 行为不变。
5. 保留后端 app-server 协议不变。

## 6. 后续扩展

后续能力按以下顺序推进：

1. 抽出更中性的 shared frontend core 包或目录。
2. 给 Desktop runtime state 增加 UI 展示入口。
3. 补齐 Desktop settings 中的 app-server binary path、workspace、local account profile。
4. 增加 Desktop smoke 覆盖 session create、turn start、event replay、app-server restart。
5. 按 feature 补齐 Web/Desktop client parity tests。

当前 adapter 覆盖 Desktop 主流程需要的 session、project、model、message stream、tool approval、memory、workspace docs，并补齐 `account.get` 与整段 `llm` 配置写入。以下能力仍作为后续 parity 工作处理：

- HTTP auth / admin user management
- session retry / compact / clear / active-turn snapshot
- user token management
- skill/app/tool list 当前返回空列表，后续应接真实 registry / capability service

这些能力不应在页面层另写 Desktop 专用逻辑，应继续通过 app-server protocol 与 shared client adapter 补齐。

## 7. 验收标准

- Web build 仍可使用 browser client。
- Desktop renderer 能通过 `window.talos` 使用 app-server client。
- Desktop 能加载 Web UI bundle。
- Chat/session/projects/memory/workspace docs 主路径不新增双份 UI 逻辑。
- Electron renderer 不获得 Node 权限。
