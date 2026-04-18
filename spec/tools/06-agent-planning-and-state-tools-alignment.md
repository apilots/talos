# Agent Planning And State Tools Alignment

## 1. 范围

本篇覆盖：

- `Agent`
- `TodoWrite`
- `EnterPlanMode`
- `ExitPlanMode`
- `AskUserQuestion`
- 相关 task/state 工具

---

## 2. 现状判断

Talos 当前 collaboration 工具：

- `spawn_agent`
- `send_input`
- `wait_agent`
- `close_agent`
- `resume_agent`

这套工具更接近“底层 session control API”，并不等于 Claude Code 的 `Agent` 工具能力。

当前方向已更新：

- 这组低层工具不再作为长期模型公开协议保留
- Talos 的模型公开 subagent 抽象改为对齐 Claude Code 的高层 `Agent`
- 当前 collaboration runtime 仅作为内部执行依赖保留

---

## 3. 当前源码基线

本篇基于本轮对以下源码的对照探索：

- Claude Code:
  - `src/tools/AgentTool/AgentTool.tsx`
  - `src/tools/TodoWriteTool/TodoWriteTool.ts`
  - `src/tools/EnterPlanModeTool/EnterPlanModeTool.ts`
  - `src/tools/ExitPlanModeTool/ExitPlanModeV2Tool.ts`
  - `src/tools/AskUserQuestionTool/AskUserQuestionTool.tsx`
- Talos:
  - `crates/action/tools/src/collaboration.rs`
  - `crates/action/tools/src/tools/collaboration.rs`

### 3.1 Claude Code 当前现状

Claude Code 的这一组工具已经形成高层工作流体系：

- `Agent`
  - 不是 session API，而是高层子 agent 启动工具
  - 支持 `subagent_type/model/run_in_background/isolation/cwd`
  - 支持 team / teammate / remote / worktree 等模式
  - 与 tool progress、agent output、background task 集成
- `TodoWrite`
  - 是结构化 session task checklist 工具
  - 直接写 app state
  - 与验证 agent / UI 展示联动
- `EnterPlanMode` / `ExitPlanMode`
  - 是正式工具，而不是 prompt 文案约定
  - 会真实修改 permission mode / session state / plan approval 流程
- `AskUserQuestion`
  - 是结构化多选问题协议
  - 支持选项、说明、preview、用户回答回写
  - `requiresUserInteraction` 是正式工具语义

### 3.2 Talos 当前现状

Talos 当前 collaboration 实现的优势在于：

- 已有 `CollaborationRuntime`
- 已有 `spawn_agent/send_input/wait_agent/close_agent/resume_agent`
- 已有 session service + request handler 驱动的子 session 能力

但当前层级明显偏底层：

- 公开工具直接暴露 runtime control API
- 没有高层 `Agent` facade
- 没有 `TodoWrite`
- 没有 plan mode 切换工具
- 没有结构化 `AskUserQuestion`
- 没有高层状态存储与 UI 渲染协议

### 3.3 本篇方案落点

因此 Agent/状态域不是简单 rename，而是：

- 保留 Talos 当前 collaboration runtime 作为底层执行依赖
- 在其上新增高层 `AgentToolFacade`
- 把 todo/plan/question 这些状态型工具正式纳入平台工具体系
- 移除当前 `spawn_agent/send_input/wait_agent/close_agent/resume_agent` 作为长期模型公开工具的定位

---

## 4. `Agent`

### 3.1 目标定位

`Agent` 是高层子 agent 启动工具，而不是 session 服务直通接口。

### 3.2 Claude Code 对齐目标

- 支持 agent type
- 支持 prompt/description
- 支持 background 模式
- 支持模型/隔离/cwd 等选项
- 能与 team/swarms/workers 体系协同

### 3.3 Talos 决策

Talos 当前薄版 collaboration runtime 可以作为底层依赖，但需要在其上新增 `AgentToolFacade`。

当前方向已确认：

- `spawn_agent/send_input/wait_agent/close_agent/resume_agent` 不再作为长期模型公开工具保留
- 对模型公开的 subagent 抽象统一收敛到高层 `Agent`
- 当前 collaboration runtime 继续保留，但仅作为 `Agent` 工具和相关状态工具的内部支撑层

---

## 5. `TodoWrite`

### 4.1 目标定位

它不是普通文本写入，而是模型对当前任务分解状态的结构化管理工具。

### 4.2 目标行为

- 写入 todo list
- 更新状态
- 允许与 plan mode 联动
- 可被 UI 稳定渲染

---

## 6. `EnterPlanMode` / `ExitPlanMode`

### 5.1 当前 Talos 关系

Talos 已有 planner 模式与 plan-react 设计，但缺少以“工具”的形式显式切换工作模式的能力。

### 5.2 目标行为

- `EnterPlanMode` 负责显式切换为 plan mode
- `ExitPlanMode` 负责退出并恢复权限/上下文策略

---

## 7. `AskUserQuestion`

### 7.1 目标定位

这是结构化向用户追问的工具，不等同于 assistant 直接发文本。

### 7.2 目标能力

- 结构化问题
- 可选项
- 等待用户回答
- 与 transcript / SDK / Web UI 协同
- 第一版先完成协议、runtime 与多端接线，UI 样式后续迭代

---

## 8. 相关 task 工具

Claude Code 还有：

- `TaskCreate`
- `TaskGet`
- `TaskUpdate`
- `TaskList`
- `TaskOutput`
- `TaskStop`

Talos 建议将其统一归入“任务与状态工具簇”，避免与 agent session API 混杂。

---

## 9. 目标模块表

| 模块 | 职责 |
|---|---|
| `tools/agent` | 高层 Agent 工具 |
| `tools/todo_write` | 当前任务分解 |
| `tools/enter_plan_mode` | 模式切换 |
| `tools/exit_plan_mode` | 模式退出 |
| `tools/ask_user_question` | 结构化追问 |
| `runtime/agent_facade` | collaboration runtime 的上层封装 |
| `runtime/task_state_store` | todo/task 持久化状态 |
| `runtime/collaboration_runtime` | 内部子 session / agent 执行依赖，不直接暴露给模型 |

---

## 10. 关键决策

1. 当前 `spawn_agent/...` 不再作为长期模型公开工具保留
2. 模型公开的 subagent 抽象统一收敛到 Claude Code 风格的高层 `Agent`
3. `CollaborationRuntime` 继续作为内部 runtime 保留，并由 `AgentToolFacade` 承接
4. 对外同时补齐 Claude Code 风格的 `Agent/TodoWrite/Plan/AskUserQuestion` 能力簇

---

## 11. 验收标准

- Talos 具备 Claude Code 风格的 `Agent` 与状态工具族
- planner mode 切换能力进入正式工具体系
- 用户追问从自然语言旁路升级为结构化工具协议
