# talos-app

Talos Web 前端应用，基于 React 19 + Vite + pnpm monorepo。

## 目录结构

```
talos-app/
├── web/          # 主 Web 应用（React + Vite）
├── desktop/      # Electron 桌面端，复用 Web UI 与本地 app-server
├── ts-sdk/       # TypeScript SDK（供 web、desktop 和外部使用）
│   └── packages/
│       ├── core/        # 共享类型、协议生成物与 app-server client 内核
│       ├── browser/     # HTTP/SSE 浏览器适配器
│       └── app-server/  # JSON-RPC stdio 与 Electron IPC 适配器
├── scripts/      # 构建辅助脚本
└── pnpm-workspace.yaml
```

## 环境要求

- Node.js >= 20
- pnpm >= 9.14.4（`npm install -g pnpm@9.14.4`）

## 快速启动（开发调试）

### 1. 安装依赖

```bash
cd talos-app
pnpm install
```

### 2. 启动后端服务

Web 应用需要连接 Talos 后端（默认端口 `8080`）。在另一个终端启动后端：

```bash
cd talos-rs
cargo run -p talos-server --bin talos-server
# 或使用 make
make run
```

确认后端已就绪：`curl http://localhost:8080/health`

### 3. 启动前端开发服务器

```bash
cd talos-app
pnpm dev
```

开发服务器启动在 `http://127.0.0.1:5173`，支持热重载（HMR）。

### 4. 配置后端连接

首次打开 `http://127.0.0.1:5173`，进入 **Settings → Server** 配置：

| 字段       | 默认值                    |
|-----------|--------------------------|
| Server URL | `http://127.0.0.1:8080`  |
| Auth Token | （可选，开发模式可留空）    |

配置保存在浏览器 `localStorage`（key: `talos-web-ui`）。

## Desktop / 本地 app-server

Desktop 通过 Electron main process 启动 `app-server` 子进程，使用
JSON-RPC over stdio JSONL 通信，不占用本地端口。Renderer 只能通过 preload
暴露的 typed IPC bridge 使用 `@talos/sdk-app-server/desktop`，不直接访问 Node
或文件系统能力。

```bash
cd talos-app
pnpm desktop:dev
pnpm --filter talos-desktop test
pnpm desktop:build
pnpm desktop:check
```

常用本地 app-server SDK 验证：

```bash
cd talos-rs
cargo build -p app-server --bin app-server

cd ../talos-app
TALOS_APP_SERVER_BIN=../talos-rs/target/debug/app-server \
TALOS_APP_SERVER_CWD=../talos-rs \
TALOS_LOCAL_LAUNCH_TOKEN=local-token \
pnpm --filter @talos/sdk-app-server test:integration
```

## 预览构建产物

```bash
pnpm build     # 编译 TypeScript + Vite 打包
pnpm preview   # 启动预览服务器（http://127.0.0.1:4173）
```

> 当前 `http://127.0.0.1:4173` 运行的即是 preview 模式，对应 `vite preview`。

## 其他脚本

```bash
pnpm typecheck   # TypeScript 类型检查
pnpm lint        # ESLint 检查
pnpm test        # Vitest 单元测试
pnpm check       # lint + test + build 全量检查
```

## Admin 面板

访问 `http://127.0.0.1:5173/admin`（需要 admin 角色账号登录）。

Admin 面板包含以下功能模块：

- **Dashboard** — 系统指标概览
- **Users** — 用户管理（增删改查）
- **Sessions** — 会话监控
- **Tools** — 工具管理
- **Models** — 模型配置管理
- **Audit** — 审计日志
- **System** — 系统信息

## 项目工作空间默认路径

```
$HOME/.talos/workspace-talos
```

可通过环境变量覆盖：

```bash
VITE_DEFAULT_PROJECT_ROOT=/your/path pnpm dev
```
