# Bash Public Tool Migration

## 1. Background

Talos 当前已经有可运行的 shell runtime，但对模型公开的稳定入口仍然依赖：

- `exec_command`
- `write_stdin`

与此同时，Claude Code 风格的正式工具已经具备初始实现，但尚未真正切到默认公开面：

- `Bash`
- `TaskOutput`
- `TaskStop`

当前状态是一个明确的过渡态：

- `exec_command` 仍然默认可见，避免真实模型和现有 e2e 场景退化
- `write_stdin` 已经收回 deferred，不再暴露给模型 catalog
- `Bash` / `TaskOutput` / `TaskStop` 仍然 deferred

这份计划的目标不是立刻切换，而是为下一轮迁移提供可执行步骤。

## 2. Migration Goal

目标状态：

1. 模型公开 shell 能力只剩 `Bash` / `TaskOutput` / `TaskStop`
2. `exec_command` / `write_stdin` 退化为内部 runtime compatibility layer
3. prompt、CLI、TUI、approval、integration scenarios 全部以最终名口径工作
4. 回放、审计和历史兼容仍能识别旧名称，但不再向模型公开

## 3. Preconditions

迁移前必须满足以下条件：

1. `Bash` 的输入输出协议与现有 `exec_command` 真实能力完全对齐
2. `TaskOutput` 不是简单 `write_stdin` 换名，而是清晰表达“读取后台任务输出”
3. `TaskStop` 能独立终止后台任务，不依赖模型继续调用低层 session API
4. prompt catalog 切到 `Bash` 后，真实模型在 e2e 中稳定优先调用 `Bash`
5. approval、tool event、SSE transcript 的工具命名策略明确，不再混用兼容名和最终名

## 4. Migration Scope

需要迁移的依赖方：

| Area | Required Change |
| --- | --- |
| `talos-tools` | 调整 shell tool visibility、aliases、descriptions、result policy |
| `talos-prompt` | prompt 文案从兼容口径切到 `Bash/TaskOutput/TaskStop` |
| `talos-agent-kernel` | 工具选择、tool replay、失败恢复与 approval 逻辑使用最终名 |
| `talos-server` | SSE / transcript / approval payload 的展示和回放口径统一 |
| `talos-cli` | stream trace 与 approve-tool 输出使用最终名 |
| `talos-ui-tui` | tool block、approval、right panel、copy export 全部切换 |
| `tests/integration` | 场景 prompt、日志断言、tool name 断言全部更新 |

## 5. Phased Execution

### Phase 1: Runtime And Contract Parity

目标：

- 让 `Bash` 成为 `exec_command` 的协议级等价替身
- 让 `TaskOutput` / `TaskStop` 成为完整后台任务闭环

实施项：

1. 对照 Claude Code 的 `BashTool` 输入字段补齐 `run_in_background`、后台输出拉取、停止控制
2. 明确 `Bash` 返回的任务标识和 `TaskOutput` / `TaskStop` 输入字段
3. 为 `Bash` / `TaskOutput` / `TaskStop` 增加工具级 execute tests
4. 为 shell 后台任务增加 e2e 场景，不再依赖 `write_stdin`

门禁：

- `Bash` foreground/background 都有稳定测试
- `TaskOutput` / `TaskStop` 可独立工作

### Phase 2: Dual-Publish Migration Window

目标：

- 在一小段窗口内同时保留兼容入口和正式入口，但默认 prompt 与展示只强调正式入口

实施项：

1. `Bash` 从 deferred 提升为默认可见
2. `TaskOutput` / `TaskStop` 从 deferred 提升为默认可见
3. `exec_command` 变为 deferred compatibility alias
4. `write_stdin` 保持 deferred internal-only compatibility
5. prompt 静态策略与 expanded tool catalog 切到 `Bash/TaskOutput/TaskStop`

门禁：

- 真实模型在核心 shell 场景中优先调用 `Bash`
- 兼容入口仅在历史回放或异常重试时触发

### Phase 3: Consumer Migration

目标：

- 清理所有面向用户和测试的旧命名依赖

实施项：

1. 更新 CLI stream trace 和 approval examples
2. 更新 TUI headers、approval card、copy markdown、tool event summary
3. 更新 integration scenario prompts 与日志断言
4. 更新文档、示例、测试夹具中的 `exec_command` / `write_stdin`

门禁：

- 所有用户可见文案都只使用最终名
- e2e 场景不再依赖 `exec_command` 作为显式工具名

### Phase 4: Compatibility Retire

目标：

- 让 `exec_command` / `write_stdin` 真正退到内部层

实施项：

1. 从默认 policy allow-list 中移除 `exec_command` / `write_stdin`
2. 从 model-visible catalog 中彻底移除兼容入口
3. 保留 router aliases 仅用于历史 transcript/replay
4. 清理 prompt、UI、tests 中所有兼容层假设

门禁：

- 全量单测通过
- `tests/integration/run_e2e.sh` 全绿
- 至少一轮真实模型回归验证通过

## 6. Rollback Rule

若在 Phase 2 或 Phase 3 发现真实模型显著回退到错误工具选择：

1. 允许临时把 `exec_command` 恢复为默认可见
2. 但必须保留 `Bash` 展示口径，不允许重新把用户文案回退成旧名
3. 回滚后先补 prompt/tool selection 证据，再重新推进迁移

## 7. Verification Matrix

每一阶段至少执行：

- `cargo test -p talos-tools --lib`
- `cargo test -p talos-agent-kernel --lib`
- `cargo test -p talos-prompt --lib`
- `cargo test -p talos-ui-tui --lib`
- `cargo test -p talos-cli`
- `tests/integration/run_e2e.sh`

重点场景：

1. shell foreground command
2. shell background command + output polling + stop
3. tool approval flow
4. CLI `--stream` trace
5. TUI transcript replay

## 8. Done Definition

迁移完成的定义：

- 模型公开 shell 工具只剩 `Bash` / `TaskOutput` / `TaskStop`
- `exec_command` / `write_stdin` 不再出现在默认 catalog、默认 policy 和用户可见文案里
- 所有 shell 相关集成场景和真实模型回归通过
