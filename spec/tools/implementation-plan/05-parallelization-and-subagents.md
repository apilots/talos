# Parallelization And Subagent Execution Plan

## 1. 目标

本篇用于指导多 subagent / 多 worker 并行实施时的拆分方式，避免多个执行线程互相踩文件或把平台层接口同时改崩。

原则：

- 并行前先冻结阶段接口
- 每个 worker 必须有清晰 ownership
- 共享边界先由主线程落文档或 trait，再允许 worker 开工
- 不允许两个 worker 同时改同一组核心平台文件

## 2. Wave 划分

### Wave 0: Platform Freeze

只允许一个主线程或一个主 worker：

- 冻结 ADR
- 冻结 `ToolExecutor` / `ToolDescriptor` / `ToolScheduler` / `ToolContext`

原因：

- 这是所有后续并行的共享接口层

### Wave 1: First Core Split

- Worker A: Files `02-A` / `02-B`
- Worker B: Shell `03-A` / `03-B` / `03-C`
- Worker C: MCP `05-A` / `05-B` / `05-C`

主线程职责：

- 维护平台层公共接口
- 处理跨域冲突
- 统一 review `ToolResultPolicy` / permission / scheduling 变化

### Wave 2: Second Core Split

- Worker D: Files `02-C` / `02-D`
- Worker E: Network `04-A` / `04-B` / `04-C`
- Worker F: State tools `06-A` / `06-B` / `06-C`

主线程职责：

- 验证 `ToolSearch` 是否已稳定
- 清理 `http_request`
- 维护公共 render / progress / result 协议

### Wave 3: Agent And Feature-Gated

- Worker G: `06-D Agent`
- Worker H: `07-B Worktree`
- Worker I: `07-C LSP`
- Worker J: `07-D Browser`
- Worker K: `07-E Scheduler/Workflow`
- Worker L: `07-F Product/Host UX`

主线程职责：

- 统一 availability catalog
- 统一 ToolSearch exposure
- 统一 final acceptance

## 3. Ownership 规则

平台层共享文件：

- `tools/registry.rs`
- `tools/router.rs`
- `tools/orchestrator.rs`
- `tools/spec*`
- `runtime/tool_context/*`
- `runtime/scheduler/*`
- `prompt/*`

规则：

- 这些文件默认只允许主线程或单独平台 worker 修改
- 域 worker 只能通过稳定接口接入，不直接重构共享核心文件

域级 ownership：

- Files worker 只改 `tools/file_*`、`runtime/fs_*`、`runtime/notebook/*`
- Shell worker 只改 `tools/bash*`、`tools/task_*`、`runtime/shell_*`、`runtime/background_tasks/*`
- MCP worker 只改 `mcp_*`、`tool_search/*`、`skill_registry/*`
- Network worker 只改 `tools/web_*`、`runtime/web_*`
- State worker 只改 `tools/todo_*`、`tools/*plan_mode*`、`tools/ask_user_question*`、`runtime/task_state_store/*`
- Agent worker 只改 `tools/agent*`、`runtime/agent_facade/*`
- Feature-gated workers 只改各自 runtime 子树

## 4. 合流节点

以下节点必须主线程集中合流：

- platform trait / struct 变更
- result protocol 变更
- permission policy 变更
- scheduler/barrier 变更
- public tool name / alias 变更
- prompt/catalog rendering 变更

## 5. 子 agent 使用建议

适合用 explorer：

- 某域源码现状盘点
- 测试缺口梳理
- Claude Code 对照点二次核验

适合用 worker：

- 某一个域的独立实现包
- 某组测试补齐
- 某个 runtime 子模块重构

不适合并行派发：

- 平台层 trait 边界尚未冻结时
- 多个域同时改 `ToolContext` / `ToolResultPolicy`
- 多个 worker 同时触碰 `ToolRegistry` / `RegistryOrchestrator`

## 6. 验收顺序

1. 先验平台层共享接口
2. 再验每个并行域自己的测试矩阵
3. 再验跨域集成
4. 最后跑全量 e2e

## 7. 失败回退规则

- 任一 worker 连续三次因共享接口变更返工，立即停止并回到主线程统一改接口
- 任一 Wave 出现跨域文件 ownership 冲突，优先缩回到更少 worker
- 若 Phase A 仍持续变动，不允许继续扩张并行度
