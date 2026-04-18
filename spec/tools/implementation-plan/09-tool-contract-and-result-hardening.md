# Tool Contract And Result Hardening Plan

## 1. Scope

本阶段只处理工具契约、结果协议和失败反馈，不扩大工具集合。

重点：

- input schema consistency
- structured output contract
- progress / rejected / error / summary rendering
- failure-to-retry guidance

## 2. Hard Gate

开始编码前必须先阅读：

1. `claude-code/src/Tool.ts`
2. 代表性工具：
   - `claude-code/src/tools/FileReadTool/*`
   - `claude-code/src/tools/FileEditTool/*`
   - `claude-code/src/tools/FileWriteTool/*`
   - `claude-code/src/tools/BashTool/*`
   - `claude-code/src/tools/WebFetchTool/*`
   - `claude-code/src/tools/WebSearchTool/*`
3. Talos 当前：
   - `talos-rs/crates/action/tools/src/trait.rs`
   - `talos-rs/crates/action/tools/src/spec.rs`
   - `talos-rs/crates/action/tools/src/output.rs`
   - `talos-rs/crates/action/tools/src/orchestrator.rs`
   - `talos-rs/apps/ui-tui/src/*`

## 3. Work Packages

### WP-1 Input Contract Cleanup

实施项：

- 统一公开工具的 input schema 风格
- 校验必填字段、枚举字段、错误语义

完成标准：

- 不再出现工具间 schema 风格明显分裂

### WP-2 Result Layer Split

实施项：

- 拆分 raw execution result / model-visible result / renderable result
- 平台层统一 summary 与 preview 策略

完成标准：

- 不再以“字符串包 JSON”作为主出口

### WP-3 Render Contract

实施项：

- 平台补齐：
  - `render_progress`
  - `render_rejected`
  - `render_error`
  - `summarize_for_model`
  - `summarize_for_transcript`

完成标准：

- CLI/TUI/Web 不再各自发明工具结果展示协议

### WP-4 Failure Guidance

实施项：

- 统一区分：
  - validation failure
  - permission rejection
  - governance rejection
  - runtime failure
  - stale-write failure
  - deferred tool unavailable

完成标准：

- 模型能拿到足够结构化的失败反馈来决定重试、换工具或总结

## 4. Verification

- `cargo test -p talos-tools --lib`
- `cargo test -p talos-ui-tui --lib`
- `cargo test -p talos-cli`
- `pnpm --filter talos-web test`

说明：

- 本阶段不跑 e2e
- 本阶段不要求真实模型回归
