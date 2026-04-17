## 实现约定: 浏览器调试与联调工具
**目标**: 明确前端实现过程中默认使用的浏览器调试手段，避免在实现期重复决策。
**成功标准**:
- 本地 Web 应用联调默认使用 `webapp-testing` skill
- 快速页面检查、点击流验证、截图、快照默认使用 `playwright` skill
- 不在实现过程中临时引入额外浏览器自动化方案，除非上述两者无法覆盖
**测试**:
- 启动本地 dev server 后，至少使用一次 `webapp-testing` 验证主链路
- 在关键交互完成后，使用一次 `playwright` 做页面快照或截图确认
**状态**: 已完成

## 本轮硬约束: Web UI First
**目标**: 本轮先完整实现 Web UI 的页面、交互和联调效果，严格限制范围，不并行推进服务端改造。
**成功标准**:
- 按设计文档把 Web UI 侧页面和主要交互完整做出来
- 优先复用现有通用 API 与现有 Rust SDK / TUI 已验证能力
- 不修改服务端代码，不新增后端 API，不调整现有服务端主链路
- 如遇到后端能力缺口，只记录为单独议题，不在本轮落代码
- Web UI 需要做到本地可运行、可调试、主要链路可演示
**测试**:
- Web UI 本地 dev server 可启动
- 主要页面可访问，主要交互可通过浏览器联调验证
- 对现有可用 API 的接入链路完成真实联调
**状态**: 已完成

## 阶段 1: Common API Baseline Audit
**目标**: 先以现有 TUI 已经走通的 HTTP API 为基线，明确 Web UI 可以直接复用的能力边界，为本轮前端完整实现提供约束。
**成功标准**:
- 列清现有可直接复用的路由：`/v1/models`、`/v1/sessions`、`/v1/sessions/{id}`、`/v1/sessions/{id}/messages`、`/tool-approvals`、`/cancel`、`/turns`、`/compact`、`/clear`
- 明确 Web UI MVP 中哪些页面和交互可以完全复用现有 API，不改服务端
- 明确本轮不做服务端改动；缺口仅记录，不实现
- 明确“不允许”为 Web UI 单独发明一套平行协议，新增能力必须先尝试挂在现有通用 API 上
**测试**:
- 对照 Rust SDK 与 TUI client 调用链做接口核对
- 确认 Web MVP 主链路可以用现有 API 覆盖到可运行水平
**状态**: 已完成

## 阶段 2: TS SDK Mirror Existing APIs
**目标**: 先让 `talos-app/ts-sdk` 对齐现有 Rust SDK / TUI 正在使用的通用 API，而不是先设计新协议。
**成功标准**:
- 建立 `talos-app/ts-sdk` monorepo 与基础包结构
- `@talos/sdk-core` 和 browser 适配层先覆盖现有能力：`listModels`、Session CRUD、`sendMessage`、`sendMessageStream`、`submitToolApproval`、`cancelActiveTurn`、`getSessionTurns`、`compactSession`、`clearSession`
- TS SDK 直接复用现有 HTTP 路由和 `TalosEvent` 流事件模型
- Web UI 通过 TS SDK 能跑通与 TUI 同一条服务端主链路
**测试**:
- Vitest 单测覆盖现有 API 映射与 URL 组装
- Mock SSE 测试覆盖 `TalosEvent` 事件消费
- 与本地 `talos-server` 的最小联调验证会话创建、发消息、审批、取消
**状态**: 已完成

## 阶段 3: Web MVP On Shared APIs
**目标**: 基于通用 API 完成 Web UI MVP，先做到“功能打通”，再判断是否存在真实后端缺口。
**成功标准**:
- 会话列表、会话详情、消息流、审批卡片、历史加载均通过现有通用 API 跑通
- `web/shared/api` 仅负责初始化 TS SDK client
- `web/shared/sse` 仅负责封装 React hook，不再自行实现另一套协议
- 历史视图优先复用现有 `/turns` 响应，由 SDK 或前端做最小归一化，不先改服务端 DTO
**测试**:
- Browser 冒烟测试覆盖会话切换、发送消息、工具审批、历史加载
- 使用 `webapp-testing` 与 `playwright` 验证主页面链路
**状态**: 已完成

## 阶段 4: Gap Register Only
**目标**: 对本轮 Web UI 实现中暴露出来的后端缺口做收集和归档，但不修改服务端代码。
**成功标准**:
- 每一个缺口都对应具体页面或交互场景
- 记录“为什么现有 API 无法复用”以及“前端当前如何临时处理”
- 缺口清单独立沉淀，供后续单独讨论服务端方案
- 本阶段不提交任何服务端实现代码
**测试**:
- 缺口清单覆盖本轮所有未打通或仅部分打通的交互
**状态**: 已完成

## 阶段 5: Stabilization And Consumer Expansion
**目标**: 在本轮 Web UI 完整实现稳定后，做前端收尾和文档一致性整理；Node/Electron 与后端扩展后置。
**成功标准**:
- `spec/web-ui-design.md`、`spec/web-ui-frontend-arch.md`、`spec/ts-sdk-design.md` 与最终 API 一致
- 清理重复定义和无效方案分支
- 输出后端缺口清单与后续扩展顺序：Project / Memory / Files / Admin
**测试**:
- Web UI 端到端回归
- Web UI 关键路径手工联调验证
**状态**: 已完成

## 阶段 6: Visual Polish Pass
**目标**: 基于真实页面截图做一轮自主视觉收尾，不等待逐项反馈，直接把明显不协调的布局和信息层级收敛到可验收状态。
**成功标准**:
- Chat、Memory、Files、Settings 的首屏层次更统一
- 会话正文、工具卡片、输入区列宽与视觉权重关系合理
- 页面头部、摘要卡、侧栏概览等高噪音区块完成减法
- 主要页面都有最新截图可供验收
**测试**:
- `pnpm --dir talos-app --filter talos-web typecheck`
- `pnpm --dir talos-app --filter talos-web test`
- `pnpm --dir talos-app --filter talos-web build`
- 使用 Playwright 对关键页面重新截图复查
**状态**: 已完成

## 阶段 7: Chat Stream UX Recovery
**目标**: 收敛聊天发送、等待、停止、断线恢复这条主链路的前端体验，严格复用现有流式发送和取消 API，不新增服务端改动。
**成功标准**:
- 发送后聊天区立即出现等待占位动画，而不是只剩右下角按钮变化
- `Send` 与 `Stop` 合并为单个主按钮，执行中可直接停止当前回答
- 刷新页面或本地流中断后，聊天区内出现可理解的重连提示与 `Retry` / `Stop` 操作
- 侧栏与会话页不会把“本地断流”误判成已完成
**测试**:
- `pnpm --dir talos-app --filter talos-web typecheck`
- `pnpm --dir talos-app --filter talos-web test`
- `pnpm --dir talos-app --filter talos-web build`
- 结合本地页面截图复查发送中与断线提示状态
**状态**: 已完成

## 阶段 8: Server Phase 2 Discovery
**目标**: 在 Web UI 第一阶段完成后，梳理第二阶段服务端改造范围，明确哪些真实页面能力需要补到共享 API。
**成功标准**:
- 基于 `GAP_REGISTER.md` 输出明确的服务端优先级顺序
- 优先级至少覆盖：Project、Memory、Files、Session metadata、Replayable stream、Admin
- 每一类能力都明确“可复用现有模块”与“需要新增 API”的边界
- 不在本阶段直接落服务端代码，只做设计收敛与实现拆分
**测试**:
- 对照现有 Rust server、SDK、TUI 能力做一次复用边界核对
- 确认第二阶段拆分不会破坏现有通用 API 路径
**状态**: 进行中

## 阶段 9: Server Phase 2 Implementation
**目标**: 按第二阶段优先级逐步把真实后端能力接到共享 API，替换当前 Web UI 的本地占位状态。
**成功标准**:
- 至少完成 P0 级真实能力接入，并让对应 Web 页面脱离本地 placeholder
- 复用既有 server/store/runtime 模块，不引入平行子系统
- TUI 与 Web UI 继续走同一条共享 API 主链路
**测试**:
- 相关 Rust crate 单测
- `tests/integration/run_e2e.sh`
- Web UI 对接真实 API 的联调回归
**状态**: 未开始
