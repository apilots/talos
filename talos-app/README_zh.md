# Talos 应用 workspace

[English](README.md)

`talos-app` 包含 React Web UI、Electron Desktop shell 和 TypeScript SDK packages。
产品功能消费共享 SDK 领域对象和后端语义，不定义客户端特有的业务模型。

## 目录

```text
talos-app/
|-- web/                  React 19 and Vite Web UI
|-- desktop/              Electron shell using the shared Web UI
|-- ts-sdk/packages/
|   |-- core/             Generated protocol types and client primitives
|   |-- browser/          HTTP and SSE adapter
|   `-- app-server/       JSON-RPC stdio, Electron, and IPC adapters
|-- scripts/              Build and verification helpers
`-- pnpm-workspace.yaml   Workspace definition
```

## 环境要求

- 推荐 Node.js 24；20.x 最低支持 20.19.0
- pnpm 9.14.4 或兼容的仓库版本

## 运行 Web UI

从仓库根目录在两个终端分别启动服务端和 Web UI：

```bash
just dev-server
just dev-web
```

开发 UI 位于 `http://127.0.0.1:5173`，默认连接
`http://127.0.0.1:8080` 的 Talos server。

## 运行 Desktop

```bash
pnpm --dir talos-app desktop:dev
pnpm --dir talos-app desktop:check
```

Desktop 在 Electron main process 启动 `app-server`，通过 JSON-RPC stdio 通信。
Renderer 使用 typed preload bridge，不能直接访问 Node.js 或文件系统。

## 验证

```bash
pnpm --dir talos-app lint
pnpm --dir talos-app test
pnpm --dir talos-app build
```

本地迭代使用 package filter。协议生成文件通过 `just gen-app-protocol` 从 Rust 刷新，
不能手工修改。

## 边界规则

- UI feature 消费 `@talos/sdk-browser` 或 `@talos/sdk-app-server`。
- 共享协议和领域类型来自 `@talos/sdk-core`。
- Browser 和 App Server facade 必须暴露对齐的 TypeScript SDK 能力族。
- TypeScript、Rust 和 embedded Rust SDK 能力共同 Review、共同完成。
- UI 代码不重复实现后端搜索、匹配、生命周期和权限规则。
- 用户可见 Timeline 变化必须有渲染和交互覆盖。

跨客户端 transport 和职责模型参见
[客户端架构](../docs/architecture/CLIENT_ARCHITECTURE_zh.md)。
