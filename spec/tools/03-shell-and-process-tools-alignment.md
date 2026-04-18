# Shell And Process Tools Alignment

## 1. 范围

本篇覆盖：

- `Bash`
- `PowerShell`
- `TaskOutput`
- `TaskStop`

以及它们依赖的：

- 后台任务
- 进程取消
- 输出持久化
- 命令语义分类

---

## 2. 当前 Talos 判断

Talos 当前 `exec_command` / `write_stdin` 的 session 化执行骨架是有价值的，不应丢弃。  
但它与 Claude Code 的 `Bash` 还差四层能力：

1. 输入契约能力
2. 权限与危险命令治理
3. read-only / concurrency-safe 判定
4. 大输出与后台任务体验

---

## 3. 当前源码基线

本篇基于本轮对以下源码的对照探索：

- Claude Code:
  - `src/tools/BashTool/BashTool.tsx`
- Talos:
  - `crates/action/tools/src/tools/unified_exec.rs`

### 3.1 Claude Code 当前现状

`Bash` 当前已经是一个完整的系统工具平台入口，而不是简单 shell 包装器。工具层直接承载了：

- 丰富输入契约：`command/timeout/description/run_in_background/dangerouslyDisableSandbox`
- 语义分类：search/read/list/silent command
- AST/security parse
- read-only validation
- 背景任务与 auto-background
- 大输出持久化与 preview
- 图像型 shell 输出支持
- UI/progress/rejected/error rendering
- permission matcher 与 sandbox 决策辅助

也就是说，Claude Code 的 `Bash` 已经把“命令执行体验”和“平台治理能力”合在同一套工具契约下。

### 3.2 Talos 当前现状

Talos 当前 `exec_command` / `write_stdin` 的优势在于：

- 已经有 session 化执行主骨架
- 已经支持 `tty`
- 已经支持 `write_stdin`
- 已经支持 sandbox escalation 参数

但工具层仍然很薄：

- 参数仍围绕 executor 请求对象组织
- 工具层没有命令语义分类
- 工具层没有 read-only / destructive / search/read/list 判断
- 工具层没有结果预览/持久化策略
- `write_stdin` 仍是 executor session API，不等于 Claude Code 的后台任务工具模型

### 3.3 本篇方案落点

因此 Shell 域不是推倒重来，而是：

- 保留 Talos 当前 session/process runtime
- 对外把模型契约收敛到 Claude Code 风格的 `Bash/TaskOutput/TaskStop`
- 把危险命令治理、结果治理、后台任务体验统一上收到平台层与 shell runtime

---

## 4. 目标设计

### 3.1 外部工具契约

Talos 对外不再暴露 `exec_command` / `write_stdin` 给模型，而改成：

- `Bash`
- `TaskOutput`
- `TaskStop`
- 条件环境下 `PowerShell`

内部依然可以复用已有 session 化进程管理器。

### 3.2 `Bash` 目标输入

```json
{
  "command": "cargo test -p talos-tools",
  "timeout": 30000,
  "description": "Run tool crate tests",
  "run_in_background": false,
  "dangerouslyDisableSandbox": false
}
```

---

## 5. `Bash` 必须补齐的能力

### 4.1 命令语义分析

Talos 需要引入 shell 语义层，至少区分：

- read-only command
- mutating command
- destructive command
- search/read/list command
- silent success command

### 4.2 权限与治理

- 普通命令走默认 sandbox
- 升权命令必须经过 governance
- 危险命令需要更高等级提示
- 对 `sed/heredoc` 这类伪文件编辑命令做专门收敛

### 4.3 并发与取消

- 只读命令可并发
- 变更命令串行
- 并发命令一旦某个 sibling error，可按策略取消同批次工具
- 支持用户中断、streaming fallback、tool discard

### 4.4 后台任务

- 长任务允许后台化
- 返回 task id
- `TaskOutput` 拉取任务输出
- `TaskStop` 终止任务

### 4.5 结果治理

- stdout/stderr 分离
- 大输出落盘并返回预览
- 结构化输出支持 image/rawOutputPath/persistedOutputPath

---

## 6. `PowerShell`

PowerShell 不应和 `Bash` 强耦合，而应作为 host-specific tool：

- 仅在 Windows runtime 可用
- 外部模型契约对齐 Claude Code `PowerShell`
- 执行层可复用相同后台任务框架

---

## 7. `TaskOutput` / `TaskStop`

### 6.1 作用

它们是后台任务模型的最小闭环：

- `Bash(run_in_background=true)` 创建任务
- `TaskOutput` 读取输出
- `TaskStop` 终止任务

### 6.2 Talos 当前状态

当前 `write_stdin` 只能连接交互 session，不等于后台任务工具，需要单独抽象。

### 6.3 目标伪代码

```rust
pub trait BackgroundTaskRuntime {
    async fn spawn(&self, spec: ShellTaskSpec) -> TalosResult<TaskId>;
    async fn read_output(&self, task_id: &str, offset: u64) -> TalosResult<TaskOutputChunk>;
    async fn stop(&self, task_id: &str) -> TalosResult<()>;
}
```

---

## 8. Talos 模块拆分建议

| 模块 | 职责 |
|---|---|
| `tools/bash` | 对外 `Bash` 契约 |
| `tools/powershell` | 对外 `PowerShell` 契约 |
| `tools/task_output` | 对外 `TaskOutput` 契约 |
| `tools/task_stop` | 对外 `TaskStop` 契约 |
| `runtime/shell_ast` | 命令语义分类 |
| `runtime/shell_permissions` | Shell 权限与危险命令治理 |
| `runtime/background_tasks` | 后台任务调度 |
| `runtime/process_sessions` | 交互 session / stdout / stderr |

---

## 9. 与现有 `exec_command` 的关系

### 8.1 不建议直接删除

`exec_command` 现有能力可以作为内部 runtime API 保留。

### 8.2 建议处理方式

- 外部模型契约迁移到 `Bash`
- `exec_command` 退化为 internal executor API
- 视兼容窗口决定是否保留 alias

---

## 10. 验收标准

- Talos 具备与 Claude Code 对齐的 `Bash` 契约
- Shell 命令不再用“固定 tool name = mutating/read-only”的粗糙方式判断并发
- 后台任务形成 `Bash + TaskOutput + TaskStop` 闭环
- PowerShell 成为条件可用工具，而不是 Bash 特判分支
