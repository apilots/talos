use agent_protocol::{NodeHealth, NodeMode, NodeStatus};

/// Database record confined to the Store/Service boundary. Hashes are never
/// returned from the public node management API.
#[derive(Clone, PartialEq, Eq)]
pub struct NodeRecord {
    pub node_id: String,
    pub owner_user_id: Option<String>,
    pub display_name: String,
    pub mode: NodeMode,
    pub endpoint: Option<String>,
    pub status: NodeStatus,
    pub credential_hash: Option<[u8; 32]>,
    pub credential_expires_at: Option<i64>,
    pub lease_token_hash: Option<[u8; 32]>,
    pub lease_expires_at: Option<i64>,
    pub last_heartbeat_at: Option<i64>,
    pub health: NodeHealth,
    pub created_at: i64,
    pub updated_at: i64,
}

pub struct CreateNodeParams {
    pub node_id: String,
    pub owner_user_id: Option<String>,
    pub display_name: String,
    pub mode: NodeMode,
    pub endpoint: Option<String>,
    pub credential_hash: Option<[u8; 32]>,
    pub credential_expires_at: Option<i64>,
    pub now_at: i64,
}

pub struct RegisterNodeParams {
    pub node_id: String,
    pub expected_credential_hash: [u8; 32],
    pub lease_token_hash: [u8; 32],
    pub lease_expires_at: i64,
    pub now_at: i64,
}

pub struct LeaseCheck {
    pub node_id: String,
    pub token_hash: [u8; 32],
    pub now_at: i64,
}

pub struct RenewNodeParams {
    pub lease: LeaseCheck,
    pub expires_at: i64,
}

pub struct HeartbeatNodeParams {
    pub lease: LeaseCheck,
    pub health: NodeHealth,
}

pub struct UpdateNodeParams {
    pub node_id: String,
    pub display_name: String,
    pub endpoint: Option<String>,
    pub status: NodeStatus,
    pub now_at: i64,
}

pub struct RotateCredentialParams {
    pub node_id: String,
    pub hash: [u8; 32],
    pub expires_at: i64,
    pub now_at: i64,
}

pub struct ListNodesParams {
    pub owner_user_id: Option<String>,
    pub cursor: Option<String>,
    pub limit: u32,
}

pub struct NodePageRecord {
    pub data: Vec<NodeRecord>,
    pub next_cursor: Option<String>,
}
