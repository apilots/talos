# MCP And External Integration Tools Alignment

## 1. 范围

本篇覆盖：

- `ListMcpResourcesTool`
- `ReadMcpResourceTool`
- `ToolSearch`
- `Skill`
- deferred MCP tools

---

## 2. 现状判断

Talos 当前 MCP 系统已经有良好骨架：

- client
- transport
- tools/list
- resources/list
- resources/read
- resource cache

但仍只到“协议可通”的阶段，没有到“系统工具平台可用”的阶段。

---

## 3. 当前源码基线

本篇基于本轮对以下源码的对照探索：

- Claude Code:
  - `src/tools/ListMcpResourcesTool/ListMcpResourcesTool.ts`
  - `src/tools/ReadMcpResourceTool/ReadMcpResourceTool.ts`
  - `src/tools/ToolSearchTool/ToolSearchTool.ts`
  - `src/tools/SkillTool/SkillTool.ts`
- Talos:
  - `crates/action/tools/src/mcp/mod.rs`
  - `crates/action/tools/src/handlers/mcp.rs`

### 3.1 Claude Code 当前现状

Claude Code 的 MCP/外部集成工具已经形成了一套完整的平台能力：

- `ListMcpResourcesTool`
  - `shouldDefer`
  - read-only / concurrency-safe
  - reconnect 后自动确保连接
  - 单 server 失败隔离，不拖垮整体结果
  - 资源列表缓存与 invalidation
- `ReadMcpResourceTool`
  - read-only / concurrency-safe
  - blob 自动落盘
  - 不把 base64 直接塞进模型上下文
  - 结构化返回 `contents`
- `ToolSearchTool`
  - deferred tools 正式进入工具协议
  - keyword search / direct select / MCP prefix search
  - deferred tool 集合变化时主动清 cache
- `SkillTool`
  - skill 进入正式工具体系
  - 可与 agent/tool progress 协同
  - skill metadata 与权限规则对接

### 3.2 Talos 当前现状

Talos 当前 MCP 骨架已经具备这些基础：

- `McpConnectionManager`
- stdio / http transport
- MCP tool list / call
- list resources / read resource
- resource TTL cache
- `mcp__server__tool` 命名

但离 Claude Code 仍有明显差距：

- `ListMcpResourcesTool` 仍是 manager + handler 级能力，不是完整对外工具契约
- `read_resource` 仍会把 blob/base64 直接拼接进 `ToolOutputPayload::Resource`
- cache 主要依赖 TTL，没有 notification-driven invalidation
- `ToolSearch` 与 `Skill` 完全缺失
- deferred tools 还没有成为 catalog 治理能力

### 3.3 本篇方案落点

因此 MCP 域不是推翻 Talos 当前协议骨架，而是：

- 保留 `McpConnectionManager` / transport / handler
- 把 Claude Code 已经沉淀出来的 resource/blob/discovery/deferred 能力补齐
- 新增 `ToolSearch` 和 `Skill`，统一纳入 platform catalog

---

## 4. `ListMcpResourcesTool`

### 3.1 目标能力

- 支持按 server 过滤
- 支持列出所有 server 的资源
- 处理 reconnect / partial failure
- 输出结构化资源信息

### 3.2 Talos 要补齐的点

- 连接健康检查
- 断开重连
- 资源缓存失效机制不只依赖 TTL，应支持 notification 驱动
- 对单个 server 错误要隔离，不拖垮整体结果

---

## 5. `ReadMcpResourceTool`

### 4.1 当前 Talos 问题

当前 `read_resource` 会把 blob/base64 直接拼进返回内容，这不适合作为模型上下文。

### 4.2 目标行为

- text resource：直接返回文本
- blob resource：落盘并返回保存路径与说明
- 不允许把大段 base64 直接塞进 transcript

---

## 6. `ToolSearch`

### 5.1 为什么必须新增

Claude Code 的工具体系不是“所有工具永远全部展开暴露”，而是存在 deferred tools、feature-gated tools、连接器工具。

Talos 如果要完全对齐，就必须支持：

- 工具发现
- 延迟展开
- MCP server 连接中的占位能力

### 5.2 目标能力

- keyword search
- direct select
- MCP tool prefix search
- deferred tool cache invalidation

---

## 7. `Skill`

### 7.1 目标定位

Skill 视为一种特殊工具，不再是 prompt 拼接旁路能力。

当前方向已确认：

- `Skill` 作为正式工具纳入 Talos 对齐范围
- 能力语义直接对齐 Claude Code
- 当前 Talos 侧仍属于缺失/假实现状态，需要按正式工具重做

### 7.2 目标行为

- 可枚举
- 可按需调用
- 可进入 ToolSearch
- skill metadata 进入 tool catalog
- 默认按 Claude Code 风格进入 deferred/discovery 路径，而不是首轮全部展开

---

## 8. 目标接口表

| 模块 | 职责 |
|---|---|
| `mcp_connection_manager` | lifecycle / reconnect / capabilities |
| `mcp_resource_store` | resource/template cache + invalidation |
| `tool_search` | deferred tool discovery |
| `skill_registry` | skill metadata 到 tool catalog 映射 |

---

## 9. 关键决策

1. MCP tools 对外仍使用 Claude Code 兼容命名
2. `ListMcpResourcesTool` / `ReadMcpResourceTool` 也保持外部兼容
3. blob resource 必须落盘
4. ToolSearch 必须纳入正式工具集，而不是 prompt 提示技巧

---

## 10. 验收标准

- Talos MCP 体系不再只是“能 call”，而是具备 Claude Code 风格的资源与工具发现能力
- `ReadMcpResourceTool` 不再直接向模型暴露 base64 blob
- Skill 与 deferred tools 可以统一进入 catalog 和 ToolSearch
