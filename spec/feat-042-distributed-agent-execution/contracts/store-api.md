# 节点 Store 与既有业务持久化

[主方案](../design.md)，完整字段见 [agent_nodes.sql](agent_nodes.sql)。新增职责是节点目录；
Session、Turn 和结果持久化仍由原 Service/Store 负责。

## 1. 新增 AgentNodeStore

在现有 orchestration/store 增加 node 模块，复用同一个 Storage。NodeRecord 只在 Store
边界内使用，字段对应 agent_nodes 的全部列，owner_user_id 为 Option<String>。hash 使用
Option<[u8;32]>，可空时间使用 Option<i64>，mode/status/health 使用共享闭合 enum。
Service 创建远程节点时生成 UUID v4；Store 原 node_id 主键持续保存该身份，更新名称/地址不改变主键。
Service 将它转换为不含秘密的 NodeInfo，available 从运行时登记及租约/心跳推导。

```rust
pub struct CreateNodeParams {
    pub node_id: String, pub owner_user_id: Option<String>, pub display_name: String, pub mode: NodeMode,
    pub endpoint: Option<String>, pub credential_hash: Option<[u8; 32]>,
    pub credential_expires_at: Option<i64>, pub now_at: i64,
}
pub struct RegisterNodeParams {
    pub node_id: String, pub expected_credential_hash: [u8; 32],
    pub lease_token_hash: [u8; 32], pub lease_expires_at: i64, pub now_at: i64,
}
pub struct LeaseCheck { pub node_id: String, pub token_hash: [u8; 32], pub now_at: i64 }
pub struct RenewNodeParams { pub lease: LeaseCheck, pub expires_at: i64 }
pub struct HeartbeatNodeParams { pub lease: LeaseCheck, pub health: NodeHealth }
pub struct UpdateNodeParams {
    pub node_id: String, pub display_name: String, pub endpoint: Option<String>,
    pub status: NodeStatus, pub now_at: i64,
}
pub struct RotateCredentialParams {
    pub node_id: String, pub hash: [u8; 32], pub expires_at: i64, pub now_at: i64,
}
pub struct ListNodesParams {
    pub owner_user_id: Option<String>, pub cursor: Option<String>, pub limit: u32,
}
pub struct NodePageRecord { pub data: Vec<NodeRecord>, pub next_cursor: Option<String> }

#[async_trait]
pub trait AgentNodeStore: Send + Sync {
    async fn upsert_embedded(&self, status: NodeStatus, now_at: i64) -> TalosResult<NodeRecord>;
    async fn create(&self, p: CreateNodeParams) -> TalosResult<NodeRecord>;
    async fn get(&self, node_id: &str) -> TalosResult<Option<NodeRecord>>;
    async fn list(&self, p: ListNodesParams) -> TalosResult<NodePageRecord>;
    async fn register(&self, p: RegisterNodeParams) -> TalosResult<NodeRecord>;
    async fn renew(&self, p: RenewNodeParams) -> TalosResult<NodeRecord>;
    async fn heartbeat(&self, p: HeartbeatNodeParams) -> TalosResult<NodeRecord>;
    async fn unregister(&self, p: LeaseCheck) -> TalosResult<()>;
    async fn update(&self, p: UpdateNodeParams) -> TalosResult<NodeRecord>;
    async fn rotate_credential(&self, p: RotateCredentialParams) -> TalosResult<NodeRecord>;
}
```

| 方法 | 事务条件 / 写入 |
| --- | --- |
| upsert_embedded | 可信进程内初始化稳定 embedded 行，更新启停状态，不颁发凭据或租约 |
| create | 管理 Service 只创建 remote；node_id 唯一，必须有所属用户和合法 endpoint/hash/期限 |
| register | remote、enabled、当前凭据 hash 匹配且未过期；替换当前租约，health=unknown，清空旧心跳 |
| renew | enabled、当前 token hash 匹配且未过期；期限不减且不超过凭据期限，同一秒重复续租可幂等返回 |
| heartbeat | enabled、当前 token hash 匹配且未过期；只更新最新 health 和服务器接收时间 |
| unregister | 当前 token hash 匹配且未过期，清空租约/心跳、health=unknown；不触及后来签发的租约 |
| update | 只更新显示名/启停/endpoint，不能变更所属用户；改变地址或停用时同事务使租约失效 |
| rotate_credential | 仅 remote；更新凭据 hash/期限，同时使租约失效 |

owner_user_id 由可信 Service 根据 Actor 决定，非节点自报字段；与原 Session.user_id 使用
同一身份。一个用户可有多个节点，不对 owner_user_id 加唯一约束。普通用户的 list 必须
注入自己的 owner_user_id，只有管理员可不带归属过滤。

RPC publisher 串行编排目录变更和 Registry 发布/撤下；AgentNodeService 用当前登记互斥协调租约写入，Store 事务仍按当前 hash 比较，
防止旧续租/心跳/注销覆盖新登记。失效租约不需要历史表。时间参数由服务端产生；远程
请求不能传入任意期限，Service 按注册凭据期限和固定 90 秒租约计算期限。

内嵌 composition 先通过 Coordinator.register_agent 安装 Harness，再由
AgentNodeService.register_embedded 调用 Store.upsert_embedded 保存稳定目录记录。
管理 create/update 只处理 remote 节点；内嵌状态由进程内初始化决定，运行时健康不写远程心跳字段。
Store 无 Harness、Client 或业务执行句柄。

## 2. 复用 SessionStore

[已有 SessionStore](../../../talos-rs/crates/orchestration/store/src/session/store.rs) 的
SessionRecord 增加非空 `execution_node_id: String`。迁移将既有 Session 固定为 `embedded`；
新 Session 由 Service 在创建前选择节点，并与 Session 其他字段一起插入，不需要二次绑定
接口或跨表执行事务。子 Session 继承父节点；创建前检查目标 Agent 在该节点可用。
节点所有者由控制面写入且不可修改。选择和每次执行都检查远程节点归属、启停、租约、
健康心跳及运行时 Harness；30 秒未收到心跳暂停新调用。原有 Service 数据访问权限保持，
不同节点的重绑定本期不开放，避免把私有 Workspace/continuation 当作已迁移。

原 [AgentExecutionStore](../../../talos-rs/crates/orchestration/store/src/session/agent_execution.rs)
的 admission、progress、waiting、terminal、continuation、command 幂等接口和对象保持原状。
无需新增 execution owner 参数或 active-turn 列。Service 继续先提交 admission，再消费
Harness permit。新增列用于选择 Harness，不表示 RPC 成功、停止确认或自动恢复依据。

## 3. 业务与节点状态分开

节点 Register/Renew/Heartbeat/Unregister 不修改 Turn。调用结果通过原 receiver 交给
Service，经过校验后写原事件和结算表。超时、租约到期及 Registry 缺席不触发节点 Store
写业务终态。未知执行仍保留已有 active turn，按既有业务管理职责处理。

需要调整的现有恢复入口见[执行流程](execution-flow.md#3-失败处理与现有代码衔接)。
该调整是修正失败判断，不增加第二套执行 Store 或远程恢复服务。
