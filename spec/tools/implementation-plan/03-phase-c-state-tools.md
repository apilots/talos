# Phase C State Tools Plan

## 1. 目标

把状态类工具从旁路能力提升为正式工具协议：

- `TodoWrite`
- `EnterPlanMode`
- `ExitPlanMode`
- `AskUserQuestion`

本阶段不做高层 `Agent` facade，只给 `Agent` 铺状态与交互底座。

## 2. Package 划分

### 2.1 Package 06-A: Task State Store + TodoWrite

交付物：

- `task_state_store`
- `TodoWrite`
- `AppendTaskStateEntry` mutation 提交

目标模块：

- `tools/todo_write`
- `runtime/task_state_store`

### 2.2 Package 06-B: EnterPlanMode / ExitPlanMode

交付物：

- `EnterPlanMode`
- `ExitPlanMode`
- permission mode state update

目标模块：

- `tools/enter_plan_mode`
- `tools/exit_plan_mode`

### 2.3 Package 06-C: AskUserQuestion Runtime

交付物：

- `AskUserQuestion` 协议
- runtime roundtrip
- TUI/SSE/UI adapter 接口

目标模块：

- `tools/ask_user_question`
- `runtime/question_flow`

约束：

- 当前先完成协议和 runtime
- UI 只保留后续体验增强，不阻塞协议落地

## 3. 依赖关系

```text
Phase A -> 06-A
Phase A -> 06-B
Phase A -> 06-C
06-A + 06-B + 06-C -> Phase D Agent
```

## 4. 并行建议

- Worker F1: `06-A`
- Worker F2: `06-B`
- Worker F3: `06-C`

合流点：

- task state mutation
- permission mode mutation
- question-answer transcript render

## 5. 测试矩阵

单测：

- task state transition
- permission mode update
- structured question payload validation

集成：

- `TodoWrite` roundtrip
- `EnterPlanMode -> ExitPlanMode`
- `AskUserQuestion` request/response roundtrip

e2e：

- SSE/UI 状态回写
- transcript 渲染
- 多轮计划模式切换

## 6. 阶段完成定义

- 状态类工具全部进入正式工具体系
- 不再依赖自然语言旁路实现任务状态和结构化追问
- 为高层 `Agent` facade 留出稳定状态接口
