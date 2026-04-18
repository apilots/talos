# Phase B Core Tools Plan

## 1. 目标

Phase B 负责把核心公开工具迁移到 Phase A 平台能力之上。

范围：

- 文件域：`Read` / `Edit` / `Write` / `Glob` / `Grep` / `NotebookEdit`
- Shell 域：`Bash` / `TaskOutput` / `TaskStop`
- MCP 域：`ListMcpResourcesTool` / `ReadMcpResourceTool` / `ToolSearch` / `Skill`
- 网络域：`WebFetch` / `WebSearch`

非目标：

- `PowerShell`
- 高层 `Agent`
- feature-gated runtime 工具

## 2. 域顺序

1. 文件域先落地
2. Shell 与 MCP 并行推进
3. 网络域在 `ToolSearch` 稳定后公开接入

原因：

- 文件域最早验证 `Read -> Edit/Write` 与 stale-write 闭环
- Shell 和 MCP 都重度依赖平台治理，但 runtime 相对独立
- 网络域要依赖 deferred catalog / `ToolSearch` 的可见性模型

## 3. 文件域实施包

### 3.1 Package 02-A: Read + Fs Read State

交付物：

- `Read` canonical public contract
- `fs_read_state` runtime
- 编码、行尾、二进制/设备文件保护

目标模块：

- `tools/file_read`
- `runtime/fs_read_state`
- `runtime/fs_encoding`

门禁：

- 未完成前，不允许进入 `Edit/Write/NotebookEdit` 最终接线

### 3.2 Package 02-B: Edit / Write

交付物：

- `Edit`
- `Write`
- structured diff / patch
- read-before-write
- stale-write rejection

目标模块：

- `tools/file_edit`
- `tools/file_write`
- `runtime/fs_diff`

### 3.3 Package 02-C: Glob / Grep

交付物：

- `Glob`
- `Grep`
- content/files/count 模式
- context/head limit/offset 支持

目标模块：

- `tools/glob`
- `tools/grep`

### 3.4 Package 02-D: NotebookEdit

交付物：

- `NotebookEdit`
- notebook parse/edit helpers
- 与普通文本编辑分离

目标模块：

- `tools/notebook_edit`
- `runtime/notebook`

## 4. Shell 域实施包

### 4.1 Package 03-A: Shell Governance Runtime

交付物：

- `shell_ast`
- `shell_permissions`
- destructive / read-only / concurrency 分类

### 4.2 Package 03-B: Bash Foreground

交付物：

- 对外 `Bash` 契约
- foreground execution
- progress / preview / persisted output

### 4.3 Package 03-C: Background Lifecycle

交付物：

- `background_tasks`
- `TaskOutput`
- `TaskStop`

约束：

- `exec_command` 保留为 internal runtime API
- 外部模型契约统一迁移到 `Bash`

## 5. MCP 域实施包

### 5.1 Package 05-A: Connection And Cache

交付物：

- lifecycle / reconnect / capabilities
- partial failure 处理
- cache invalidation

### 5.2 Package 05-B: ReadMcpResource Text/Blob

交付物：

- text resource 直接回模型
- blob resource 落盘再引用

### 5.3 Package 05-C: ToolSearch

交付物：

- deferred tool discovery
- feature-gated tool discoverability

门禁：

- 未完成前，不允许公开 rollout `07` 类特性工具

### 5.4 Package 05-D: Skill Registry

交付物：

- skill metadata -> tool catalog 映射
- `Skill` 正式进入工具集

## 6. 网络域实施包

### 6.1 Package 04-A: Web Runtime

交付物：

- `web_permissions`
- `web_fetcher`
- markdown 化
- redirect / domain policy

### 6.2 Package 04-B: WebFetch

交付物：

- `WebFetch` canonical public contract
- preview / citation / persisted result

### 6.3 Package 04-C: WebSearch

交付物：

- provider abstraction
- `WebSearch`
- citation rendering

### 6.4 Package 04-D: Remove http_request

交付物：

- `http_request` 退出模型公开 catalog
- 清理 prompt / registry / tests 中的公开曝光

## 7. 并行波次

### Wave 1

- Worker A: `02-A` + `02-B`
- Worker B: `03-A` + `03-B` + `03-C`
- Worker C: `05-A` + `05-B` + `05-C`

### Wave 2

- Worker D: `02-C` + `02-D`
- Worker E: `04-A` + `04-B` + `04-C`
- Worker F: `05-D` 与 Phase C 前置接线

硬约束：

- `02-A` 先于 `02-B` 验收
- `05-C` 先于 `04` 公开 rollout
- `04-D` 在 `04-B/04-C` 稳定后执行清理

## 8. 测试矩阵

文件域：

- 单测：validation、stale-write、encoding、diff
- 集成：`Read -> Edit -> Write`
- e2e：patch/transcript rendering

Shell 域：

- 单测：command classification、permission
- 集成：background lifecycle
- e2e：approval/sandbox/progress

MCP 域：

- 单测：blob persistence、cache invalidation
- 集成：resource text/blob flow
- e2e：ToolSearch discoverability

网络域：

- 单测：URL/domain/redirect
- 集成：provider integration
- e2e：citation/progress

## 9. 阶段完成定义

- 四个域全部接入 Phase A 平台
- 不再存在生产态 mock 网络工具
- `Read/Edit/Write/Grep/Glob/NotebookEdit/Bash/ToolSearch/WebFetch/WebSearch` 外部契约完成切换
- `exec_command` 和 MCP runtime 退为 internal runtime layer
