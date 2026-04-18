# Tools Confirmed Decisions

## 1. 目的

本篇收敛 `spec/tools` 当前已经确认的关键决策，作为后续实现的直接输入。

这里不再保留“待拍板”表述，除非后续范围再次变化。

---

## 2. 已确认决策

### 2.1 `http_request`

已确认：

- `http_request` 不再作为模型公开工具保留
- 本轮 Claude Code 对齐范围只保留 `WebFetch` + `WebSearch`
- 当前 mock `http_request` 直接退出对齐目标集

### 2.2 低层 subagent tools

已确认：

- `spawn_agent/send_input/wait_agent/close_agent/resume_agent` 不再作为长期模型公开工具保留
- Talos 的模型公开 subagent 抽象统一收敛到高层 `Agent`
- 当前 collaboration runtime 继续保留，但只作为内部 runtime 依赖

### 2.3 `Skill`

已确认：

- `Skill` 作为正式工具纳入 Talos 对齐范围
- 能力语义对齐 Claude Code
- 当前 Talos 仍是缺失/假实现状态，需要按正式工具重做
- 默认仍按 Claude Code 风格进入 deferred / ToolSearch 路径

### 2.4 feature-gated tools

已确认：

- 除 `PowerShell` 外，feature-gated tools 按完整交付目标纳入总体对齐范围
- 不再把 `LSP/worktree/browser/cron/workflow/monitor/...` 视为后续可选项
- 前提仍然不变：没有 runtime 就不能用 mock 顶替

### 2.5 `PowerShell`

已确认：

- 当前暂不考虑支持 `PowerShell`
- 因此它不纳入当前实现范围

### 2.6 `AskUserQuestion`

已确认：

- 第一版先完成协议、runtime 与多端接线
- UI 样式和交互细节后续迭代
- 目标是先把结构化问答协议和状态流闭环做对

---

## 3. 实现影响

上述决策确认后，后续实现直接按以下方向推进：

- 网络域只保留 `WebFetch/WebSearch`
- Agent 域以高层 `Agent` 作为公开 subagent 抽象，当前 collaboration runtime 退到内部
- `Skill` 进入正式工具体系，而不是继续停留在 prompt 旁路
- feature-gated tools 除 `PowerShell` 外全部进入完整对齐范围
- `AskUserQuestion` 先做协议闭环，再迭代 UI

---

## 4. 当前仍可能产生的新决策

目前没有必须立即拍板的新问题。

如果后续实现阶段再出现新的范围调整，应在本篇追加，而不是回到分散讨论。
