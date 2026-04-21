# talos-app

Talos Web 前端应用，基于 React 19 + Vite + pnpm monorepo。

## 目录结构

```
talos-app/
├── web/          # 主 Web 应用（React + Vite）
├── ts-sdk/       # TypeScript SDK（供 web 和外部使用）
│   └── packages/
│       ├── core/     # 核心 HTTP 客户端 (TalosClient)
│       └── browser/  # 浏览器适配器
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
cargo run --bin talos-server
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
