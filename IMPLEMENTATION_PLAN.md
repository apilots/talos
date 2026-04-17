## 阶段 0: Web Frontend Architecture Remediation
**目标**: 先修复 `talos-app` 当前最明显的前端架构偏离，优先解决分层反向依赖、feature 公共出口缺失、widget 越界取数和基础 lint 门禁缺失问题。
**成功标准**:
- `shared` 不再反向依赖 `features/chat`
- `features/chat` 建立统一 `index.ts` 公共 API
- `Sidebar` 变为纯展示 widget，由 feature/container 注入数据
- ESLint 增加 feature 边界限制，能自动拦截 shared/widgets 越界 import
- `serverUrl` 默认值改为 `VITE_API_BASE_URL`
**测试**:
- `pnpm --dir talos-app --filter talos-web lint`
- `pnpm --dir talos-app --filter talos-web typecheck`
- `pnpm --dir talos-app --filter talos-web test`
**状态**: 已完成

## 阶段 0.1: Web Frontend P1 P2 P3 Convergence
**目标**: 一次性收敛 `talos-app` 剩余的 `P1 / P2 / P3` 前端架构整改项，补齐 i18n、feature 目录、store 切片归属、message/tool widget 拆分和基础测试缺口。
**成功标准**:
- 引入 `react-i18next` 并建立 `shared/i18n` 资源目录，页面主文案不再直接硬编码
- `memory / files / settings / admin / connection` 建立对应 feature 目录，并由页面消费 feature hooks
- `MessageList` 拆分为独立 block 组件，`ToolCallCard` 升级为 compound components
- `useChatSession` 拆成多个职责清晰的小 hook
- 协议类型与 UI 类型分离，`shared/types/protocol.ts` 与 `shared/types/ui.ts` 成为明确边界
- 新增 domain/helper 测试，测试文件数不再停留在早期的 2 个
**测试**:
- `pnpm --dir talos-app --filter talos-web lint`
- `pnpm --dir talos-app --filter talos-web typecheck`
- `pnpm --dir talos-app --filter talos-web test`
**状态**: 已完成

## 阶段 0.2: Frontend Governance Automation
**目标**: 为 `talos-app` 建立统一的本地质量入口、repo 级治理检查脚本和 GitHub Actions workflow，让前端规范真正进入自动门禁。
**成功标准**:
- `talos-app/web/package.json` 提供统一 `check` 入口
- `talos-app/package.json` 提供 monorepo 级 `check` 与 `check:repo` 入口
- `talos-app/scripts/check-repo.mjs` 落地基础 repo 规则校验
- `.github/workflows` 新增明确标注为前端 app 的 workflow，仅覆盖 `talos-app/**`
- CI 与本地复用同一组脚本入口，不再各自维护一套命令
**测试**:
- `pnpm --dir talos-app check`
- `pnpm --dir talos-app check:repo`
- `pnpm --dir talos-app --filter talos-web build`
**状态**: 已完成

## 阶段 1: Scope Freeze And Reuse Audit
**目标**: 明确第二阶段服务端改造的真实范围，并先踩准现有模块的复用边界，避免为 Web UI 重新发明一套平行后端。
**成功标准**:
- 明确本阶段优先级：`Project`、`Session delete`、`Memory markdown`、`Workspace markdown docs`
- 明确延期项：`Replayable session stream`、`Admin`
- 对每一类能力标注“复用现有模块”与“必须新增持久化/API”的边界
- 输出一份可 review 的总体方案 spec
**测试**:
- 对照 `talos-rs` 中现有 `gateway / coordinator / session-store / cognition memory` 调用链完成复用核对
- 方案文档包含清晰的 in-scope / out-of-scope 划分
**状态**: 进行中

## 阶段 2: Project And Session Lifecycle APIs
**目标**: 为 Web UI 提供真实的 `Project` CRUD、session 可选 project 关联，以及真实的 session delete 能力。
**成功标准**:
- 提供项目列表、创建、编辑、删除 API
- `Project` 新表持久化 `default_root_path`
- `Session` 支持可选 `project_id`
- 新增真实 delete session API，而不是继续误用 close
- Web UI 欢迎页、项目切换器、session 删除动作脱离浏览器本地状态
**测试**:
- Rust 单测覆盖项目 CRUD、session project 绑定、session delete
- HTTP handler 单测覆盖列表、创建、更新、删除、delete session
- Web UI 联调验证项目创建、按 project 展示 session、delete session
**状态**: 未开始

## 阶段 3: Memory Markdown APIs
**目标**: 让 `/memory` 页面直接操作 workspace memory 目录下的按天 Markdown 文件与索引，而不是再设计新的 memory 表。
**成功标准**:
- 提供 memory 搜索 API
- 提供按天 memory 文件列表、读取、保存 API
- 保存后自动刷新相关索引
- Web UI `/memory` 页面脱离本地 placeholder records
**测试**:
- Rust 单测覆盖 memory 文件读取、保存、索引刷新、搜索
- Web UI 联调验证 memory 搜索、打开某天文件、保存后刷新一致
**状态**: 未开始

## 阶段 4: Workspace Markdown Docs APIs
**目标**: 给 Settings 新增一个真实的 workspace markdown docs 编辑面，支持固定白名单文件的读取与保存。
**成功标准**:
- 提供 workspace docs 列表 API
- 提供单文件读取与保存 API
- 仅支持白名单文件：`AGENTS.md`、`SOUL.md`、`IDENTITY.md`、`USER.md`、`HEARTBEAT.md`
- Settings 对应 tab 脱离本地假数据
**测试**:
- Rust 单测覆盖 docs 白名单映射、读取、保存
- Web UI 联调验证左侧列表切换、右侧编辑与保存
**状态**: 未开始

## 阶段 5: Deferred Phase Decision
**目标**: 在 P0/P1 真实能力落地后，再单独决策 `Replayable session stream` 与 `Admin` 的服务端设计，不把这两项混进当前主链路。
**成功标准**:
- 对 replayable stream 单独产出设计决策：做/不做、何时做、需要哪些底层改造
- 对 admin 数据面单独产出设计决策：聚合来源、权限边界、缓存策略
- 不让延期项阻塞 `Project / Session delete / Memory markdown / Workspace docs` 的交付
**测试**:
- 输出单独 decision/spec 文档
- 明确与当前共享 API 的兼容策略
**状态**: 未开始
