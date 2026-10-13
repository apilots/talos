# 执行节点 Memory 维护端口

[主方案](../design.md)。控制面拥有维护调度、Session 空闲互斥、模型调用与用量、Session
cursor；绑定 Worker 拥有形成策略、日期与 Workspace 文件。复用原 Kernel 形成算法、
`talos-memory` 校验和原子写入，不新增业务模型、文件副本或执行表。

## 调用链

```text
Turn reflection → bound AgentHarness → Worker UserWorkspaceMemoryService
Dream scheduler → bound AgentHarness.form_dream_memory
                → Worker KernelAgentProvider → existing formation algorithm
                → Worker MemoryService → local files
                → existing history/cursor ports → control-plane Services
```

Coordinator 查询 Session 固定节点并验证归属/可用性；内嵌直调同一 WorkerHarness，远程由
RemoteAgentClient 内聚 gRPC。每个网络请求含 NodeAuth；节点验证租约、owner 和 agent_id，
按自身配置解析 Workspace，不接收控制面的默认根目录。不可用节点明确失败。

## Rust 与 gRPC

[AgentHarness](../../../talos-rs/crates/orchestration/agent-protocol/src/port.rs) 的维护入口：

| 方法 | 参数 | 返回（AgentResult） | gRPC |
| --- | --- | --- | --- |
| read_workspace_memory | AgentSessionContext | UserWorkspaceMemorySnapshot | ReadWorkspaceMemory(SessionRequest) |
| update_workspace_memory | AgentSessionContext, UserWorkspaceMemoryUpdate | UserWorkspaceMemorySnapshot | UpdateWorkspaceMemory(UpdateWorkspaceMemoryRequest) |
| form_dream_memory | AgentSessionContext, DreamMemoryModelCall | Option<u64>（已写入的 covers_until_seq） | FormDreamMemory 双向流 |

```rust
pub type DreamMemoryModelCall = Arc<dyn Fn(Vec<ChatMessage>)
    -> AgentFuture<'static, AgentResult<ChatResponse>> + Send + Sync>;
```

这是本次维护调用的依赖，由控制面注入；本地直调，远端 Client 转换流内回调。它不缓存执行结果。

WorkerHarness 组合原执行 Harness、原 Kernel Provider 与文件 owner；七个执行方法原样转发。
无此维护能力的 Host 明确拒绝。控制面不按 Agent ID 查找内嵌 Provider 来维护远程 Session。

唯一 PB 定义：[memory.proto](../../../talos-rs/crates/orchestration/agent-pb/proto/memory.proto)和
[harness.proto](../../../talos-rs/crates/orchestration/agent-pb/proto/harness.proto)。投影位于
[memory_pb.rs](../../../talos-rs/crates/orchestration/agent-protocol/src/memory_pb.rs)和
[dream_pb.rs](../../../talos-rs/crates/orchestration/agent-protocol/src/dream_pb.rs)。

WorkspaceMemorySnapshot 保留 Missing 模板、Valid 文档/版本、Invalid 原文/诊断三态；
WorkspaceMemoryUpdate 保留六种操作、optional 的 unchanged/set/clear、MustNotExist/Match
版本条件。继续使用原 Markdown parser；文件 unary deadline 10 秒，不重试写入。

```proto
message DreamMemoryModelPrompt { string system = 1; string user = 2; }
message DreamMemoryModelReply { string content = 1; TokenUsage usage = 2; }
message FormDreamMemoryRequest {
  NodeAuth auth = 1;
  oneof body { SessionContext context = 2; DreamMemoryModelReply model_reply = 3; Failure model_failure = 4; }
}
message DreamMemoryFormation { optional uint64 covers_until_seq = 1; }
message FormDreamMemoryResponse {
  oneof result { DreamMemoryModelPrompt model_prompt = 1; DreamMemoryFormation formed = 2; Failure failure = 3; }
}
```

首帧为 context。Worker 原算法需要模型时发送 model_prompt；Client 调用本次调用附带的
DreamMemoryModelCall（原 ChatMessage/ChatResponse），回传 model_reply 或 model_failure。
形成算法只消费 System/User 文本、结果 content 和 token usage，PB 仅投影这些字段；不作为
通用 LLM 协议，其他消息形态明确拒绝。模型选择、reasoning 与用量处理仍在原 InnerModelClient。
控制面不接触 Worker 文件或判断日期。形成操作默认整体 300 秒预算，首帧等待 30 秒；
断流/租约撤销取消本次形成，不自动重放，也不伪造 cursor 成功。

## 业务时序

自动反思仍在成功主 Turn 发布前执行，复用 InnerModelClient、Memory 用量类型及原
best-effort 策略；快照和更新通过同一 Harness 访问绑定 Worker。

Dream 仍预约空闲 Session；Worker 使用自身 Kernel 配置和 timezone，从 canonical history
构造原维护输入，不读取 summary。原算法在文件写入成功后，通过原 cursor port 推进控制面
Session cursor。模型/用量失败不写文件、不推进 cursor；文件成功而 cursor 提交失败时保留
原重复覆盖校验行为，重跑不制造副本。

本期没有无 Session 的多节点文件浏览接口；文件管理需要显式的节点选择。
