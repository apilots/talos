# Migration Compatibility And Rollout

## 1. 目标

本篇定义从 Talos 当前工具体系迁移到 Claude Code 对齐体系的实施策略。

这里的“migration / rollout”仅表示实施顺序，不表示要长期保留兼容层。

关键问题：

1. 工具名变更怎么处理
2. 旧工具何时下线
3. mock 工具如何重写
4. 新平台如何分批切换

---

## 2. 命名迁移策略

### 2.1 外部模型可见工具名

目标直接切到 Claude Code 风格：

| 当前 Talos | 目标 External Name |
|---|---|
| `read_file` | `Read` |
| `edit` | `Edit` |
| `write` | `Write` |
| `grep_files` | `Grep` |
| `list_dir` | `Glob` 的补充，不再直接等价暴露 |
| `exec_command` | `Bash` |
| `write_stdin` | 迁移到 `TaskOutput` / internal session API |
| `spawn_agent` 等 | 内部 collaboration runtime API，不再作为长期公开工具名 |
| `web_search` | `WebSearch` |
| `web_fetch` | `WebFetch` |

### 2.2 Canonical name 原则

迁移完成后，Talos 对模型只保留一套公开工具名。

也就是说：

- `Read` 是唯一公开名
- 不允许 `Read` 与 `read_file` 长期同时暴露
- 不允许 `Bash` 与 `exec_command` 长期同时暴露
- 不允许 UI 展示标签反向渗透成另一套协议名

### 2.3 Internal alias 兼容

迁移窗口内允许：

- old name -> new executor
- hidden alias
- 仅用于老 prompt / 老测试 / 内部兼容

但有两个硬约束：

1. alias 不进入长期 tool catalog
2. alias 兼容结束后必须清理，不得固化成平台常态

### 2.4 不做兼容性框架分叉

Talos 当前阶段不做“旧框架一套 + 新框架一套”的长期双轨设计。

因此这里的迁移原则是：

- 保留现有工具主链路骨架
- 直接增强平台层抽象
- 工具逐步切换到新平台能力
- 切换完成后旧语义直接清理

不采用的做法：

- 为旧工具单独保留一套治理接口
- 为新旧工具长期维护两套结果协议
- 因为个别工具未完成重写，就长期保留旧 prompt/tool catalog 语义

---

## 3. 分阶段 rollout

### 阶段 A：平台层落地

- 新 `ToolBehavior`
- 新 `ToolContext`
- validation / permission / result policy
- canonical public name / alias discipline
- 平台规范先抽象完成，再允许工具接入

### 阶段 B：核心工具切换

- `Read/Edit/Write/Grep/Glob`
- `Bash/TaskOutput/TaskStop`
- `WebFetch/WebSearch`
- `ListMcpResourcesTool/ReadMcpResourceTool/ToolSearch`

### 阶段 C：状态与协作工具切换

- `TodoWrite`
- `Plan mode`
- `AskUserQuestion`
- 为后续高层 `Agent` 抽象预留状态与交互基础

### 阶段 D：Agent 与特性工具补齐

- `Agent`
- 当前 `spawn_agent/send_input/wait_agent/close_agent/resume_agent` 从模型公开工具集中移除
- `LSP`
- worktree
- browser
- schedule/workflow

---

## 4. 测试矩阵

### 4.1 单测

- validation
- permission
- stale-write
- shell classification
- result budget
- mcp blob persistence

### 4.2 集成测试

- Read -> Edit -> Write 闭环
- Bash background task lifecycle
- WebSearch/WebFetch provider integration
- MCP resource/text/blob flow
- Agent spawn/wait/result flow

### 4.3 e2e

- TUI transcript tool rendering
- SSE tool progress
- approval / sandbox
- multi-turn tool continuation

---

## 5. 风险与对策

| 风险 | 对策 |
|---|---|
| 名字切换影响 prompt 与测试 | 仅保留短期 alias 过渡，尽快清理旧 prompt |
| 平台层改动面大 | 先平台，后工具，按层切换 |
| mock 重写带来行为变化 | 将其视为修正，不做旧行为兼容 |
| tool result 预算变化影响上下文 | 增加 persisted output 与 preview 方案 |
| Agent 工具和现有 collaboration runtime 冲突 | 保留 runtime，公开层统一收敛到高层 `Agent` facade |

### 5.1 规模估算

当前更现实的量级判断如下：

- Phase A 平台层：
  约 `2~3 周`
- Phase B 文件/Shell/网络/MCP 核心工具：
  约 `3~5 周`
- Phase C 状态工具：
  约 `1~2 周`
- Phase D Agent + feature-gated tools：
  约 `4~6 周`

整体严格执行的量级大致为：

- `2~3 个月`

这意味着：

- Phase A 本身就不是“顺手改一下 trait”的小活
- `Agent` 不应过早排到平台层刚落地之后立即并行实现
- 需要按依赖顺序推进，而不是所有域同时开工

---

## 6. Definition Of Done

- `spec/tools` 下总领与分项 spec 全部完成
- 核心公开工具全部有明确对齐目标
- 条件工具全部有依赖判定
- 迁移顺序、兼容策略、测试矩阵全部写清楚
