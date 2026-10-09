# 节点注册、租约与认证

[主方案](../design.md)。节点管理在控制面，执行 RPC 在节点。全部 PB 见
[node.proto](../../../talos-rs/crates/orchestration/agent-pb/proto/node.proto) 和 [auth.proto](../../../talos-rs/crates/orchestration/agent-pb/proto/auth.proto)。

## 1. 共享对象

```rust
// agent-protocol; secret-bearing objects must not implement plaintext Debug output.
pub struct NodeAuth { pub node_id: String, pub token: String }
pub struct NodeRequest<T> { pub auth: NodeAuth, pub params: T }
pub struct RegisterParams { pub deployments: Vec<Deployment> }
pub struct Deployment { pub agent_id: String }
pub struct LeaseValidity { pub expires_at: i64, pub server_at: i64 }
pub struct Registration {
    pub lease_token: String, pub validity: LeaseValidity, pub owner_user_id: String,
}
pub enum NodeMode { Embedded, Remote }
pub enum NodeStatus { Enabled, Disabled }
pub enum NodeHealth { Unknown, Healthy, Degraded }
pub struct HeartbeatParams { pub health: NodeHealth }
pub struct HeartbeatResult { pub received_at: i64, pub registration_required: bool }
```

协议版本使用 gRPC package `talos.agent.v1`。所有时间为 Unix 秒。PB 请求按具体方法命名：
`RegisterRequest` 包含 NodeAuth 和 RegisterParams，`HeartbeatRequest` 包含 NodeAuth 和
HeartbeatParams；`RenewLeaseRequest`、`UnregisterRequest` 的 params 使用 Empty。
NodeAuth 只有一个定义；缺少 auth/params 或未知 enum 拒绝。
Register 必须声明至少一个真实 Harness 的逻辑 agent_id；空列表、空 ID 或重复 ID 均拒绝，
不能将零个 Client 的发布标记为可执行。
节点整体归 owner_user_id
所有；用户可以使用自己节点的能力，不逐 Agent 或 Workspace 分配节点权限。

## 2. 控制 RPC / Rust SDK

agent-client 的 concrete `AgentNodeClient` 提供：

```rust
impl AgentNodeClient {
    pub async fn register(&self, r: NodeRequest<RegisterParams>) -> TalosResult<Registration>;
    pub async fn renew_lease(&self, r: NodeRequest<()>) -> TalosResult<LeaseValidity>;
    pub async fn heartbeat(&self, r: NodeRequest<HeartbeatParams>) -> TalosResult<HeartbeatResult>;
    pub async fn unregister(&self, r: NodeRequest<()>) -> TalosResult<()>;
}
```

控制面 RPC adapter 验证 auth 后调用现有 Service 中新增的 AgentNodeService；业务 Service
不接受裸 token。内部认证结果只有必要信息：

```rust
// Created only by the authentication boundary, never deserialized from business params.
pub enum NodePrincipal {
    Registration { node_id: String, credential_hash: [u8; 32] },
    Lease { node_id: String, token_hash: [u8; 32] },
}
```

AgentNodeService 的对应方法签名为 `async fn method(&self, principal: &NodePrincipal,
params: Params) -> TalosResult<Result>`；Params/Result 与 SDK 表达同一对象。register 仅接受
Registration principal，其他控制 RPC 仅接受 Lease principal。

| 操作 | 效果 |
| --- | --- |
| Register | 检查 enabled/凭据期限/有效部署描述；更新当前租约，返回 token/期限/所属用户；不处理 Session/Turn |
| RenewLease | 条件更新当前有效租约期限；token 不变；不改业务执行状态 |
| Heartbeat | 保存服务器接收时间和健康；返回是否需要重新 Register |
| Unregister | 清除当前租约、摘除对应句柄；节点调用前已停止接收并清理本地执行 |

控制面节点 listener 一次构造完整的注册/管理、执行句柄 publisher、Session data、
subagent 和 tool image 服务；这些依赖均必需，不支持只登记但不发布执行的服务形态。
Register 处理期间发布当前部署的 RemoteAgentClient 并标记登记已发布，再返回租约。
节点 Runner 安装租约并负责续租、心跳和到期处理；健康心跳与有效租约、已发布句柄共同
决定 available。发布句柄不等于已收到健康心跳，也不安装占位 Harness。
注册只发布 agent_id；Provider 能力判断保留在原 Harness。当前句柄和 Client token 只在内存，AgentNodeService 不额外保存 token 原文。控制面重启后内存为空，Heartbeat 返回
registration_required=true，节点重新 Register。若租约已经
过期则 Renew/Heartbeat 返回 ResourceExpired，节点也走 Register。重新登记不触发 Turn 恢复。

## 3. 凭据和租约

- 控制面创建 256-bit 随机注册凭据；Register 签发独立随机租约 token。数据库只存 SHA-256
  hash；校验使用恒定时间比较。Get/List/日志/错误不返回 token 或 hash。
- 使用 TLS 并验证对端证书。endpoint 由节点所有者或管理员设置，节点 Register 不能随意改变控制面
  的访问目标。节点验证本地已安装的 NodeAuth，CP 验证数据库当前 hash/有效期。
- 默认 90 秒租约、30 秒续租、10 秒心跳；新期限为 min(server_now + 90s, credential_expires_at)。
  本地截止时刻由请求发送的单调时间加 expires_at − server_at 得到；失败不延长期限。
- 节点 Runner 在本地截止时刻撤销租约，拒绝新 RPC 并取消旧流，然后尝试重新注册；显式鉴权拒绝停止
  使用该租约。Runner 所有退出路径均通知 Host 清除租约，再等待 gRPC graceful shutdown。控制面撤下到期/失鲜节点的新调用入口，保留原业务事实。
- Register 替换租约；旧 token 不能续期、发起新调用或新流帧。新 token 安装时取消旧 token
  的未完成 RPC，执行停止仍由原 Harness 负责。正常续租不替换 token，不影响长流。
- 停用、改 endpoint、轮换凭据使当前租约失效并撤下 Client。节点通过控制 RPC
  拒绝或最迟本地租约到期发现撤销；失联时撤销有至多剩余租约的传播窗口，不承诺瞬时停机。
- 注册凭据放在工具不可写的配置目录，文件权限 0600。持有节点凭据属于受信任执行边界；
  Workspace 内容不应读取这些凭据。临时网络不可达与执行已经停止是两个不同结果。

## 4. 内嵌登记与 Registry

内嵌节点由可信 composition 使用稳定 node_id `embedded` 和现有 Agent definition：
构造 Harness 后通过 Coordinator 安装句柄，再由 AgentNodeService 持久化节点目录记录，
不经过网络鉴权。Store 中 mode=embedded、网络字段为空，
owner_user_id 为空；用户/Workspace 沿用原 Service 权限，原有用户与 anonymous 本地模式不
需要预先逐项登记。管理员创建远程节点不能占用 `embedded` 身份。

```rust
impl AgentNodeService {
    // Trusted composition entry only; never exposed as an RPC.
    pub async fn register_embedded(&self, has_harnesses: bool) -> TalosResult<NodeInfo>;
}
// Public registration entry points stay on the existing Coordinator owner.
impl CoordinatorImpl {
    pub async fn register_agent(&self, node_id: String, agent_id: String,
        harness: Arc<dyn AgentHarness>) -> TalosResult<()>;
    pub async fn replace_remote_agent(&self, node_id: String, agent_id: String,
        harness: Arc<dyn AgentHarness>) -> TalosResult<()>;
    pub async fn unregister_remote_node(&self, node_id: &str);
}
// Remains private to Coordinator; Service never manipulates this index directly.
impl AgentRegistry {
    pub async fn register(&self, node_id: String, agent_id: String,
        harness: Arc<dyn AgentHarness>) -> AgentResult<()>;
    pub async fn get(&self, node_id: &str, agent_id: &str)
        -> AgentResult<Arc<dyn AgentHarness>>;
    pub async fn contains_agent(&self, agent_id: &str) -> bool;
    pub async fn replace(&self, node_id: String, agent_id: String,
        harness: Arc<dyn AgentHarness>) -> AgentResult<()>;
    pub async fn unregister_node(&self, node_id: &str);
}
```

Registry 仍是现有内存索引。composition 分别调用 Service 持久化节点目录和 Coordinator 发布句柄；
远程 Register/Unregister/Update/Rotate 在现有 RPC publisher 的同一个短期异步互斥内
完成目录变更与 Registry 发布/撤下；没有后台延迟删除任务，旧撤销不会越过新发布。
该锁不覆盖管理用户认证、执行 RPC、心跳或续租，也不增加状态表。只改显示名保留租约和句柄，即使
节点尚未收到首个心跳或暂时失鲜；改 endpoint/停用/轮换才撤销租约，node_id 保持不变。内嵌可用性来自进程内生命周期，远程来自有效租约、
健康心跳和已发布 Client；均不另存 available。Registry 不知道 Session/Turn 如何持久化。

## 5. 管理接口

沿用[现有 Actor](../../../talos-rs/crates/orchestration/service/src/access/types.rs) 的用户身份，
按节点归属管理，不使用 NodeAuth。管理 gRPC 与 NodeControl 挂到控制面 TLS listener：管理请求从
authorization metadata 读取现有用户 Bearer token，复用 auth_service.verify_access_token
生成 RequestIdentity，再映射 Actor。用户只能创建/查看/管理自己的节点；管理员可以代管。
Create 不接受 node_id，由 Service 自动生成 UUID v4，同时返回在 NodeInfo 与 NodeCredential 中。
部署流程把返回值写入 Worker `TALOS_HOME/conf/talos.yaml` 的 `agent_node.runner.node_id`；
Worker 只读取已有配置，重启不生成新 ID，也不自动注册创建或回写配置。
Create 省略 owner_user_id 时使用 Actor.user_id，指定其他用户须管理员身份；普通用户的
List 强制过滤 owner_user_id。Get/Update/Rotate 先检查归属，不信任客户端自报 role。
网络管理入口必须启用用户认证；本地 embedded 初始化使用上述可信 composition 入口。
节点所有者或管理员通过生成的 AgentNodeManagementClient 调用五个管理 RPC，不另建管理框架或 UI。
AgentNodeService 管理方法为 `async fn method(&self, actor: &Actor,
r: Request) -> TalosResult<Result>`：

| 方法 | Request | Result |
| --- | --- | --- |
| create_node | CreateNodeRequest | CreatedNode { node: NodeInfo, credential: NodeCredential } |
| get_node | GetNodeRequest | NodeInfo |
| list_nodes | ListNodesRequest | NodePage |
| update_node | UpdateNodeRequest | NodeInfo |
| rotate_credential | RotateCredentialRequest | NodeCredential |

请求与查询对象的字段逐项以 node.proto 为准；对应领域结构使用同名字段和闭合 enum。
NodeInfo 不包含秘密，available 由 Service 推导。list 默认 50、最大 200，node_id 游标分页。
Create 创建 remote 节点；embedded 由 composition 配置。owner_user_id 创建后保持不变，
Register 不接受自报用户覆盖归属，Update 只修改显示名/地址/启停。Create/Rotate 凭据只返回
一次，丢失响应则轮换，不保存可回读明文。

控制面在已有配置中启用独立 TLS 入口（`gateway.auth` 必须为 `true`）：

```yaml
agent_node:
  control_plane:
    bind_address: "0.0.0.0:18800"
    tls_certificate_file: "/etc/talos/agent-control.crt"
    tls_private_key_file: "/etc/talos/agent-control.key"
    trusted_node_ca_file: "/etc/talos/agent-node-ca.crt"
```

管理调用使用 `agent-pb` 生成的 `AgentNodeManagementClient`。例如：

```rust
let tls = tonic::transport::ClientTlsConfig::new()
    .ca_certificate(tonic::transport::Certificate::from_pem(ca_pem));
let channel = tonic::transport::Endpoint::from_shared(
    "https://control.example:18800".to_string(),
)?
.tls_config(tls)?
.connect()
.await?;
let mut client = agent_pb::agent_node_management_client::AgentNodeManagementClient::new(channel);
let mut request = tonic::Request::new(agent_pb::CreateNodeRequest {
    display_name: "My laptop".into(),
    endpoint: "https://laptop.example:18801".into(),
    owner_user_id: None,
    credential_expires_at: expires_at_unix_seconds,
});
request.metadata_mut().insert(
    "authorization",
    format!("Bearer {user_access_token}").parse()?,
);
let response = client.create_node(request).await?.into_inner();
// Save CreatedNode.node.node_id in runner.node_id and the one-time credential outside the Workspace.
```

其余四个管理方法使用相同 metadata；节点 `NodeAuth` 不用于管理调用。

独立节点将一次性注册凭据写在节点的 `<talos_home>/conf/` 下，文件权限为 `0600`，
并在自身已有配置里加入：

```yaml
agent_node:
  runner:
    node_id: "550e8400-e29b-41d4-a716-446655440000" # Use the UUID returned by CreateNode.
    control_plane_endpoint: "https://control.example:18800"
    credential_file: "/home/user/.talos/conf/node.credential"
    bind_address: "0.0.0.0:18801"
    tls_certificate_file: "/home/user/.talos/conf/agent-node.crt"
    tls_private_key_file: "/home/user/.talos/conf/agent-node.key"
    trusted_ca_file: "/home/user/.talos/conf/control-ca.crt"
```

`talos-worker` 通过统一 Worker library 构造真实 Kernel Harness，启动 TLS RPC listener 后注册实际
deployments；上述监听地址、证书和私钥均为必填。注册成功后节点安装租约，资源 Client
和 Harness RPC 的每个请求都使用当前租约；租约失效、注销或进程退出时停止接纳新调用。
