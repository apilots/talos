# 阶段 4：统一 talos-worker 运行时

历史阶段记录：当前可运行远程场景已收敛到 `17_multi_node`；旧 16 脚手架已移除。
本轮实际验收见[阶段 8](08-execution-boundaries.md)，以下保留当时结果。

状态：**Complete（结构迁移）**。两种 Host 已共用 Worker library；运行时完整验收仍有缺陷，见[阶段 5](05-multi-node-e2e.md)，不以结构迁移完成替代功能验收。

## 目标

将内嵌与独立节点重复的 Kernel Agent 组装收敛为一个 `talos-worker` library。
`talos-server` 使用该 library 获得进程内 `AgentHarness`；独立 `talos-worker` binary
使用同一 library，并在外层增加 Harness gRPC listener、节点注册、租约和心跳。
Coordinator 继续只持有 `Arc<dyn AgentHarness>`，不感知传输形态。

## 4.1 Worker library

状态：Complete。

工作：

- 新增 `crates/app/talos-worker`，集中构造 LLM Provider Router、工具、插件/MCP、本地
  Workspace Memory、Kernel Provider、Harness 和 deployment 描述。
- Worker 只通过现有 typed ports 获取 Session history、附件、memory cursor、Agent control
  与 ToolImage；不调用 Coordinator、NodeStore、SessionStore，也不打开控制面数据库。
- 输出同一组 Harness、deployment 和必要的运行时句柄，供两种 Host 组合；不新增
  embedded/remote 业务枚举，不复制 Harness DTO。

成功标准：内嵌和独立组装不再各自创建 Kernel Provider/Harness；Worker 不拥有或调用
Coordinator、NodeStore、SessionStore，也不打开控制面数据库。

## 4.2 talos-server 内嵌接入

状态：Complete。依赖：4.1。

工作：

- `talos-base` 构造已有本地 Service/Store adapters，并注入 `talos-worker`。
- 将 Worker 返回的 Harness 以稳定节点 `embedded` 注册到原 Coordinator。
- 保持原配置热更新、MCP 生命周期、Dream Memory maintenance 和 Session/Turn 持久化顺序。

成功标准：原 embedded Session/Turn 行为不变；`talos-base` 不再包含另一套 Worker Harness 组装。

## 4.3 独立 binary 与回归

状态：Complete。依赖：4.1。

工作：

- 新增 `bin/talos-worker` 薄入口；远程资源使用现有 `AgentSessionDataClient` 实现 Worker ports。
- Worker Harness 外包现有 `AgentHarnessRpcAdapter`，并运行 `AgentNodeRunner` 完成注册、续租、
  心跳和注销。
- `talos-server` 只保留控制面入口；进程测试和 E2E 使用 `talos-worker`。
- 更新项目结构、Rust 模块清单、Spec 和启动示例。

成功标准：`talos-server` 无参数只启动控制面；`talos-worker` 独立启动并完成原跨进程主链路；
控制面没有新增 RPC 执行记录或传输状态持久化。

验证：

- `just test -p talos-worker -p talos-worker-bin -p talos-base -p talos-server`
- `just clippy -p talos-worker -p talos-worker-bin -p talos-base -p talos-server`
- `just fmt-check`
- `just e2e-case 16_distributed_agent`
- `pnpm --dir talos-app check:repo`

阶段出口（2026-09-21）：

- `talos-worker` library 成为唯一 Kernel Provider/Harness 生产组装位置；它接收现有 typed
  ports，不调用 Coordinator、NodeStore、SessionStore，也没有 embedded/remote 业务分支。
- `talos-server` 注入本地 Service adapters 后直接将 Harness 注册为 `embedded`；独立
  `talos-worker` 注入 AgentSessionDataClient，并只在 Host 外层增加 gRPC、注册和租约。
- `talos-server` 只保留控制面产品入口；进程测试、E2E、开发命令和 Server 镜像均使用
  `talos-worker` 执行节点。实现未增加控制面执行状态、RPC 账本或数据库字段。
- `just test` 的阶段 scoped 集合 499/499；阶段 scoped `just clippy`、`just fmt-check`、
  `pnpm --dir talos-app check:repo`、Spec
  47 个本地链接、Bash/ShellCheck 均通过。`just e2e-group 16_distributed_agent` 使用真实
  独立 Worker 通过，耗时 64.11 秒；`./build.sh server-bin` 产出可执行的
  `talos-server` 与 `talos-worker`。
