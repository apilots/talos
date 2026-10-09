# 独立 Kernel 节点的控制面依赖

[主方案](../design.md)的 Harness RPC 只传执行调用。独立节点构造原
`KernelAgentProviderParams` 时，还需要原 Service/Store 持有的资料和工具能力。
控制面持有的资源由节点侧 Client 实现原 trait，在原 Service/Store owner 处理；
Workspace 文件使用节点本地原实现。节点不打开控制面的 Session/Turn 数据库，也不用
空实现替代。跨节点协议在 `agent-pb` 定义专用 gRPC 方法，转换在 Client/adapter；
业务类型的唯一来源仍是各原 crate。

## 依赖与必要的远程入口

| 原端口及 owner | 跨节点时的方法 | 请求 / 返回的原业务语义 |
| --- | --- | --- |
| `SessionHistoryReader` / `SessionHistoryService` | `ReadSessionHistory` | `session_id, window, limit` → 按完成序的 `SessionHistoryTurn[]` |
| `AttachmentReader` / `FileService` | `ResolveAttachments`（流） | `project_id?, file_ids[]` → 同顺序的文件元数据及原始字节；用户由租约确定 |
| `KernelMemoryProgressStore` / `SessionStore` | `ReadMemoryProgress`, `AdvanceMemoryProgress` | `session_id` → cursor；`session_id, expected, next` → 原 CAS 结果 |
| `DreamMemoryFormationStore` / `MemoryService` | 节点内直调 | Markdown 与 bundle 留节点 Workspace；原 revision + batch CAS 不变 |
| `MemoryReader` / `MemoryService` | 节点内直调 | 原 typed 查询与结果；SQLite 仅为可从 Markdown 重建的节点本地索引，不是 Session/Turn 业务持久化 |
| `UserWorkspaceMemoryReader/Writer` / `UserWorkspaceMemoryService` | 节点内直调 | 节点本地 `MEMORY.md`；保留原 Missing/Valid/Invalid、版本 CAS 与 entry 操作 |
| `ToolImageStore` / `FileService` | `SaveToolImage`（流）；读取复用 `ResolveAttachments` | 每张图一次原文件生命周期，Client 按原顺序循环，保留部分成功行为 |
| `AgentControl` / `TalosService` | `SpawnSubagent`, `SendSubagentMessage`, `WaitSubagent`, `InterruptSubagent`, `FollowUpSubagent`, `ListSubagents` | 原请求/结果；父 Session 节点亲和继续由 Service 校验 |

每个跨节点请求均是 `NodeAuth auth` 加具体 `*Params params`；每个普通响应使用具体
`oneof {业务结果, Failure}`，流使用 typed 元数据帧、有限大小的字节块和失败帧。
不要用通用 JSON/Any 包装业务对象或把文件字节放入事件。现有 `AgentRunResult`、
History、Memory、Subagent 对象要逐字段投影；无法无损投影时先补该方法的 typed PB，
不能丢字段。只有原 Tool arguments 等既有 JSON 扩展点仍用原有编码。

## 授权及数据位置

- 控制面每次从 NodeStore 校验当前租约 hash、过期时间和 enabled 状态，读取不可变的
  `owner_user_id`，不得信任节点自报 user_id。涉及 Session 的调用还要查原 SessionStore，
  要求 Session 所属用户与节点 owner 相同，且 `execution_node_id` 等于该节点。
- 附件 owner 从有效节点租约派生，project 沿用原 `TurnFileScope`，交原 FileService
  做所有者、项目和文件状态校验；不接收节点自报 owner。这样可直接实现现有
  `AttachmentReader`，不为 RPC 强加它原本没有的 Session 上下文。Workspace 内
  `MEMORY.md`、Dream Memory 日文件、bundle 与可重建
  搜索索引均由节点本地原实现管理；节点只接收逻辑 workspace_id，并在本机解析根路径。
  控制面保留 Session/Turn 数据库。本功能不新增用户 Memory 管理代理；若产品需要从
  控制面跨节点浏览这些文件，应单独定义明确的节点选择语义，不能在多节点时猜测位置。
- 子 Agent 调用保留原 `AgentCaller`、`AgentTarget` 与原 Service 校验；节点 token 仅
  确认运行节点身份，不授予管理员权限或跨用户权限。
- Kernel transcript、summary、continuation、shell/文件工具工作目录属于节点本地
  Workspace，不经过此 RPC。模型、skills、MCP、网络工具在节点按生产配置构造，
  其所需密钥和插件目录必须在节点配置，不从控制面隐式读取。
- 控制面必须配置可信节点 TLS CA；节点 Harness listener 使用证书和私钥。
  Register 处理期间发布对应 RemoteAgentClient；节点收到响应后安装 token，健康心跳后允许路由。停用、租约
  失效、endpoint 变更或重新 Register 时撤下旧实例。控制面重启后等待重登记。

本清单对应的协议定义在 [resources.proto](../../../talos-rs/crates/orchestration/agent-pb/proto/resources.proto)。
生产组合和验收证据见[节点接口](node-api.md)及[阶段 9](../plans/09-review-ablation.md)。

控制面资源 adapter 位于 [agent_node_resources_rpc.rs](../../../talos-rs/crates/app/talos-base/src/agent_node_resources_rpc.rs)；注册/管理与 listener 组合位于 [agent_node_rpc.rs](../../../talos-rs/crates/app/talos-base/src/agent_node_rpc.rs)。
