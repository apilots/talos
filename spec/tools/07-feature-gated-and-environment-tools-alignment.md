# Feature-Gated And Environment Tools Alignment

## 1. 范围

本篇覆盖 Claude Code 中依赖运行环境、host 能力、feature flag 或产品模式的工具：

- `LSP`
- `EnterWorktree` / `ExitWorktree`
- `WebBrowserTool`
- `PowerShell`
- `ScheduleCron*`
- `RemoteTriggerTool`
- `MonitorTool`
- `WorkflowTool`
- `ConfigTool`
- `BriefTool`
- `SnipTool`
- `TerminalCaptureTool`
- `REPLTool`

---

## 2. 分类原则

这些工具不应与核心公开工具混在一个交付批次里，必须先按依赖分层。

### 2.1 Host-bound

需要本地宿主能力：

- `LSP`
- `WebBrowserTool`
- `TerminalCaptureTool`
- `REPLTool`
- `PowerShell`

### 2.2 Workspace / Git-bound

- `EnterWorktree`
- `ExitWorktree`

### 2.3 Scheduler / Trigger-bound

- `ScheduleCron*`
- `RemoteTriggerTool`
- `MonitorTool`
- `WorkflowTool`

### 2.4 Product / UX-bound

- `ConfigTool`
- `BriefTool`
- `SnipTool`

---

## 3. 当前源码基线

本篇基于本轮对以下源码的对照探索：

- Claude Code:
  - `src/tools/LSPTool/LSPTool.ts`
  - `src/tools/EnterWorktreeTool/EnterWorktreeTool.ts`
  - `src/tools/ExitWorktreeTool/ExitWorktreeTool.ts`
  - `src/tools/PowerShellTool/PowerShellTool.tsx`
- Talos:
  - `crates/action/tools/src` 当前无对应 `LSP/worktree/PowerShell/browser/cron/workflow` 工具实现

### 3.1 Claude Code 当前现状

这些 feature-gated 工具并不是“占位工具”，而是依赖完整 runtime 的正式工具：

- `LSP`
  - deferred tool
  - read-only / concurrency-safe
  - file validation + permission checking
  - 依赖 LSP server manager、file open/sync/request
- `EnterWorktree` / `ExitWorktree`
  - 是真实 session cwd / git worktree 状态切换工具
  - 具备校验、状态恢复、风险提示、删除保护
- `PowerShell`
  - 不是 Bash 的壳，而是独立 Windows shell tool
  - 具备 read-only validation、command semantics、sandbox policy handling、background task

这些工具的共同点是：

- 先有 runtime，再有工具
- 工具层只负责把 runtime 能力纳入统一工具协议
- 不使用 mock 填位

### 3.2 Talos 当前现状

Talos 在这一域目前基本是空白：

- 没有 `LspRuntime`
- 没有 worktree tool/runtime
- 没有 `PowerShell` tool
- 没有 browser / cron / monitor / workflow tool runtime

因此这一域的重点不是“如何重写旧实现”，而是：

- 明确哪些能力可以进入第一批对齐
- 明确哪些能力必须依赖新 runtime 落地
- 明确未具备 runtime 前不得暴露 mock 工具

### 3.3 本篇方案落点

因此本篇不是细化每个工具内部逻辑，而是：

- 对 feature-gated 工具做 runtime 依赖分层
- 明确它们如何进入 deferred catalog / ToolSearch
- 明确哪些工具在当前 Talos 阶段属于后续批次

当前方向已确认：

- 除 `PowerShell` 外，feature-gated tools 按完整交付目标纳入本轮总体对齐范围
- `PowerShell` 当前明确不考虑支持
- 未具备 runtime 的工具不能用 mock 顶上，必须补 runtime 后正式进入工具集

---

## 4. 落地原则

### 3.1 先有平台接口，再有工具

例如：

- 没有 `LspRuntime`，就不应先暴露 `LSP`
- 没有 browser automation runtime，就不应先暴露 `WebBrowserTool`
- 没有 durable scheduler，就不应先暴露 `ScheduleCron*`

### 3.2 不把条件工具伪装成核心工具

这类工具若尚未具备运行条件：

- 不出现在默认 tool catalog
- 可由 `ToolSearch` 暴露为“当前环境不可用”
- 不使用 mock 代替

### 4.3 完整交付边界

本篇当前交付边界已确认：

- `LSP`
- `EnterWorktree`
- `ExitWorktree`
- `WebBrowserTool`
- `ScheduleCron*`
- `RemoteTriggerTool`
- `MonitorTool`
- `WorkflowTool`
- `ConfigTool`
- `BriefTool`
- `SnipTool`
- `TerminalCaptureTool`
- `REPLTool`

以上工具都属于完整对齐范围。

唯一明确排除项：

- `PowerShell`

---

## 5. 重点工具说明

### 4.1 `LSP`

目标：

- definition / references / diagnostics / symbols 等代码智能能力

前置依赖：

- 语言服务器管理器
- 文件打开/同步协议
- 按文件类型选择 server

### 4.2 `EnterWorktree` / `ExitWorktree`

目标：

- 独立工作树切换与恢复

前置依赖：

- git worktree runtime
- session 级 cwd 隔离

### 4.3 `WebBrowserTool`

目标：

- 真实浏览器交互，而不是 `WebFetch`

前置依赖：

- Playwright / browser host
- 截图、snapshot、事件采集

---

## 6. 验收标准

- 所有 feature-gated tools 都有明确的依赖归类
- 没有运行时条件的工具不以 mock 形式伪上线
- ToolSearch 可以感知这些工具的存在与不可用原因
