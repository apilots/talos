# Tool Surface Canonicalization Plan

## 1. Scope

本阶段只处理工具公开面与命名，不扩展新能力。

范围：

- `apply_patch`
- `list_dir`
- `memory_search`
- `memory_get`
- 相关 prompt / planner / policy / tests / UI 口径

非目标：

- request-time tool selection
- 结果协议重构
- 新工具功能扩展

## 2. Hard Gate

开始编码前必须先阅读：

1. `claude-code/src/tools.ts`
2. `claude-code/src/Tool.ts`
3. Talos 当前：
   - `talos-rs/crates/action/tools/src/lib.rs`
   - `talos-rs/crates/action/tools/src/registry.rs`
   - `talos-rs/crates/action/tools-policy/src/lib.rs`
   - `talos-rs/crates/common/prompt/src/*`

## 3. Work Packages

### WP-1 Remove Non-Target Public Tools

实施项：

- 从模型公开面移除 `apply_patch`
- 从模型公开面移除 `list_dir`
- 清理 prompt / planner / allow-list / tests 中的公开引用

完成标准：

- catalog 中默认不再出现这两个工具
- prompt 不再引导模型使用这两个工具

### WP-2 Rename Memory Tools

实施项：

- `memory_search` -> `MemorySearch`
- `memory_get` -> `MemoryGet`
- 同步 registry、policy、prompt、UI、tests

完成标准：

- canonical public names 统一
- alias 仅保留短期迁移用途

### WP-3 Rust Module Alignment

实施项：

- 评估并重命名与 canonical name 长期冲突的 rs 文件 / tool struct 名
- 收口 public name / internal name / module name 三套体系

完成标准：

- 不再继续新增明显偏离 canonical name 的文件与类型名

## 4. Verification

- `cargo test -p talos-tools --lib`
- `cargo test -p talos-prompt --lib`
- `cargo test -p talos-agent-kernel --lib`

说明：

- 本阶段不跑 e2e
- 本阶段不要求真实模型回归
