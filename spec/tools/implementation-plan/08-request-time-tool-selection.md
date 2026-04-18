# Request-Time Tool Selection Plan

## 1. Scope

本阶段目标是把 Talos 从“默认把大部分工具都暴露给模型”迁移到 Claude Code 风格的：

- tools universe
- request-time filtered active tools
- deferred tools
- ToolSearch-driven discovery

## 2. Hard Gate

开始编码前必须先阅读：

1. `claude-code/src/tools.ts`
2. `claude-code/src/services/api/claude.ts`
3. `claude-code/src/tools/ToolSearchTool/ToolSearchTool.ts`
4. Talos 当前：
   - `talos-rs/crates/action/tools/src/spec.rs`
   - `talos-rs/crates/action/tools/src/registry.rs`
   - `talos-rs/crates/action/tools/src/orchestrator.rs`
   - `talos-rs/crates/common/prompt/src/*`

## 3. Work Packages

### WP-1 Selection Model

实施项：

- 引入 request-time tool selection 数据结构
- 区分：
  - `active_tools`
  - `deferred_tools`
  - `always_loaded_tools`

完成标准：

- 不再由 `ToolVisibility` 单独决定模型最终可见工具集合

### WP-2 Deterministic Selector V1

实施项：

- 基于 request context / permission context / planner mode 做第一版 deterministic selector
- 先不引入独立 intent classifier

完成标准：

- 高频主工具进入 active set
- workflow/state tools 按上下文选择进入 active set
- `Skill` 等工具默认 deferred

### WP-3 ToolSearch Integration

实施项：

- `ToolSearch` 与 deferred tool catalog 接线
- prompt 中体现“可搜索但未默认加载”的工具事实

完成标准：

- deferred tools 不再只是 registry 静态标签
- `ToolSearch` 成为真实入口

### WP-4 Prompt And Planner Sync

实施项：

- PromptBuilder 消费 request-time selected tools，而不是完整默认 catalog
- planner 文案同步新模式

完成标准：

- prompt 不再暗示“模型总能看到全部工具”

## 4. Verification

- `cargo test -p talos-tools --lib`
- `cargo test -p talos-agent-kernel --lib`
- `cargo test -p talos-prompt --lib`

说明：

- 本阶段不跑 e2e
- 本阶段不要求真实模型验证
