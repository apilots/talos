# Phase D Agent And Feature-Gated Tools Plan

## 1. 目标

在平台、核心工具和状态工具全部稳定后，完成：

- 高层 `Agent` facade
- feature-gated tools 的 availability catalog 与正式接入

范围：

- `Agent`
- `EnterWorktree` / `ExitWorktree`
- `LSP`
- `WebBrowserTool`
- scheduler / trigger / workflow / remote / monitor / terminal / repl / config / brief / snip

非目标：

- `PowerShell`

## 2. Agent 实施包

### 2.1 Package 06-D: AgentToolFacade

交付物：

- `tools/agent`
- `runtime/agent_facade`
- 对 `CollaborationRuntime` 的上层封装

约束：

- 模型公开层只保留高层 `Agent`
- `spawn_agent/*` 从模型公开协议中移除
- `CollaborationRuntime` 继续作为内部 primitive 保留

依赖：

- `Phase C`
- collaboration runtime 审计完成

## 3. Feature-Gated 实施包

### 3.1 Package 07-A: Availability Catalog

交付物：

- feature-gated tool visibility
- unavailable reason
- `ToolSearch` 集成

### 3.2 Package 07-B: Worktree

交付物：

- `EnterWorktree`
- `ExitWorktree`
- session-level cwd isolation

### 3.3 Package 07-C: LSP

交付物：

- definition / references / diagnostics / symbols
- language server lifecycle

依赖：

- `07-B` 最终验收先完成

### 3.4 Package 07-D: Browser

交付物：

- `WebBrowserTool`
- screenshot / snapshot / event collection

### 3.5 Package 07-E: Scheduler And Workflow

交付物：

- cron / remote / monitor / workflow 类工具

### 3.6 Package 07-F: Product And Host UX Tools

交付物：

- config / brief / snip / terminal / repl 类工具

## 4. 并行波次

### Wave 3

- Worker G: `06-D Agent`
- Worker H: `07-B Worktree`
- Worker I: `07-C LSP`
- Worker J: `07-D Browser`
- Worker K: `07-E Scheduler/Workflow`
- Worker L: `07-F Product/Host UX`

硬约束：

- `06-core` 先于 `06-D`
- `05-C ToolSearch` 先于 `07-A`
- `07-B` 先于 `07-C` 最终验收

## 5. 测试矩阵

Agent：

- 集成：spawn/wait/result flow
- e2e：multi-turn continuation

feature-gated：

- 单测：availability gating、unavailable reason
- 集成：worktree/LSP/browser/scheduler
- e2e：ToolSearch availability reason rendering

## 6. 阶段完成定义

- `Agent` 高层抽象正式接管模型公开 subagent 能力
- feature-gated tools 均有明确可用性判定和不可用原因展示
- 不再通过 mock 或旁路方式暴露条件工具
