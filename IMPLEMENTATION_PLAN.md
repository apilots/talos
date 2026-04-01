## 阶段 1: Baseline And Type System
**目标**: 完成 Talos tools 子系统的基线梳理与强类型抽象落地，建立 `ToolCatalog`、`ToolInvocation`、`ToolOutputPayload`、`ToolHandler` 等核心类型，并保持现有 general turn 可继续工作。  
**成功标准**: `action/tools` 内完成 `spec + registry + router + handler` 基础骨架；`PromptBuilder` 仍可注入工具定义；`agent-kernel` 能通过新入口执行现有本地工具。  
**测试**: `action/tools` 单测覆盖 spec 构建、invocation 标准化、registry dispatch；`agent-kernel` 回归现有 tool loop 测试。  
**状态**: 已完成

## 阶段 2: Coding Tools Migration
**目标**: 将高价值 coding tools 迁移为新 runtime，包括 `exec_command`、`write_stdin`、`read_file`、`list_dir`、`grep_files`、`apply_patch`，并将旧 `read/write/edit/exec` 标记为 legacy。  
**成功标准**: 新工具能通过统一 handler/runtime 路径执行；`apply_patch` 作为独立工具可用；sessionized exec 可在多轮 tool call 中持续交互。  
**测试**: `action/tools` 单测覆盖 exec 生命周期、apply_patch 校验、文件读取/遍历/搜索；增加 `agent-kernel` 集成测试验证工具调用与结果回注。  
**状态**: 已完成

## 阶段 3: Parallelism And Sandbox Orchestrator
**目标**: 落地 per-tool parallel safety 与统一 approval/sandbox orchestrator，填充 `action/tools-sandbox` 的实际能力。  
**成功标准**: 工具批次可根据 tool metadata 决定并发或串行；mutating tool 不会与 parallel-safe read tool 混跑；approval/sandbox 策略统一由 orchestrator 管理。  
**测试**: `action/tools-sandbox` 单测覆盖审批、sandbox 选择、失败重试、审批缓存；`agent-kernel` 集成测试覆盖并发批次、混合批次、sandbox 拒绝场景。  
**状态**: 已完成

## 阶段 4: External Tool Federation
**目标**: 接入 MCP tool、MCP resource、dynamic tool，使 Talos tools 从本地工具容器升级为统一工具编排层。  
**成功标准**: MCP schema 可进入 prompt；router 能解析外部工具调用；dynamic tool 可通过 event/gateway 完成请求与结果回填；general turn 不需要新增第二条执行通路。  
**测试**: `action/tools` 单测覆盖 MCP/dynamic payload 路由；`agent-kernel` 集成测试覆盖外部工具结果注入；必要时增加网关事件链路测试。  
**状态**: 已完成

## 阶段 5: A2A And Production Hardening
**目标**: 结合 Talos 现有 orchestration 能力，将 `skill` / `A2A` 封装为特殊 tool，并完成文档、配置、e2e 回归与 legacy 清理。  
**成功标准**: 协作型工具协议与 Talos orchestration 对齐；配置项进入 `config` 模块；`docs/plans` 与实现一致；旧工具别名和 demo/mock 工具完成清理或降级。  
**测试**: 相关 crate 单元测试全绿；`tests/integration/run_e2e.sh` 全量通过；新增 A2A/skill 回归用例。  
**状态**: 已完成

**实际完成内容**：
- `main.rs` 接入 MCP bootstrap（读取 `config.agent.kernel.mcp.servers`，启动时连接 MCP 服务器并注册工具）
- `main.rs` 补充 `register_network_tools` 调用（原来缺失）
- `register_core_local_tools` 移除冗余的 `ReadTool`（已被 `ReadFileTool` 替代）；保留 `ExecTool`（与 `exec_command` 接口不同，有各自用途）
- `tools-policy` default 同步更新，移除 `read` 条目
- e2e 全量 7 个场景通过

## 阶段 6: Review Fixes And Regression Closure
**目标**: 修复 CR 暴露的审批链路、trait 落地与 MCP fail-fast 遗留问题，恢复构建与回归门禁。  
**成功标准**: `SessionService` 新接口在所有实现与 mock 上补齐；CLI 可显式 reject tool approval；coordinator 按 session 实际 agent 路由审批；MCP server 连接失败时启动直接报错。  
**测试**: 补充 CLI/coordinator/MCP 单测；相关 crate 单测通过；`tests/integration/run_e2e.sh` 全量通过。  
**状态**: 已完成

## 阶段 7: Review Fix Commit
**目标**: 修复本轮 review 暴露的 3 个问题，恢复构建、审批闭环和 exec session 隔离。  
**成功标准**: `talos-tools` 可重新通过编译；tool approval 提交后会主动恢复当前 pending turn；`write_stdin` 只能访问同一 Talos session 创建的 exec session。  
**测试**: 相关 crate 单测覆盖 approval resume 与 exec session ownership；执行针对性 `cargo test` 与 `cargo check --workspace`。  
**状态**: 已完成
