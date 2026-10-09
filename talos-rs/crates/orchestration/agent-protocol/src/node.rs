/// Authentication material carried by every node control or Harness request.
/// Keep this value out of logs and diagnostic Debug output.
#[derive(Clone)]
pub struct NodeAuth {
    pub node_id: String,
    pub token: String,
}

pub struct NodeRequest<T> {
    pub auth: NodeAuth,
    pub params: T,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deployment {
    pub agent_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterParams {
    pub deployments: Vec<Deployment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaseValidity {
    pub expires_at: i64,
    pub server_at: i64,
}

/// Secret-bearing registration; its token is never logged or persisted in plaintext.
pub struct Registration {
    pub lease_token: String,
    pub validity: LeaseValidity,
    pub owner_user_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeMode {
    Embedded,
    Remote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeStatus {
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeHealth {
    Unknown,
    Healthy,
    Degraded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeartbeatParams {
    pub health: NodeHealth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeartbeatResult {
    pub received_at: i64,
    pub registration_required: bool,
}

/// Public node state; credential and lease hashes stay inside the Store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeInfo {
    pub node_id: String,
    pub display_name: String,
    pub mode: NodeMode,
    pub endpoint: Option<String>,
    pub status: NodeStatus,
    pub health: NodeHealth,
    pub lease_expires_at: Option<i64>,
    pub last_heartbeat_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub owner_user_id: Option<String>,
    pub available: bool,
    pub credential_expires_at: Option<i64>,
}

/// A one-time management response that must never be logged or re-readable.
pub struct NodeCredential {
    pub node_id: String,
    pub token: String,
    pub expires_at: i64,
}

pub struct CreatedNode {
    pub node: NodeInfo,
    pub credential: NodeCredential,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodePage {
    pub data: Vec<NodeInfo>,
    pub next_cursor: Option<String>,
}

/// Creates a node with a control-plane-generated, immutable UUID.
pub struct CreateNodeRequest {
    pub display_name: String,
    pub endpoint: String,
    pub owner_user_id: Option<String>,
    pub credential_expires_at: i64,
}

pub struct GetNodeRequest {
    pub node_id: String,
}

pub struct ListNodesRequest {
    pub cursor: Option<String>,
    pub limit: u32,
}

pub struct UpdateNodeRequest {
    pub node_id: String,
    pub display_name: String,
    pub endpoint: String,
    pub status: NodeStatus,
}

pub struct RotateCredentialRequest {
    pub node_id: String,
    pub expires_at: i64,
}

/// A future timestamp is not evidence of a live node. The boundary is exclusive.
pub fn heartbeat_is_recent(last_heartbeat_at: Option<i64>, now: i64) -> bool {
    last_heartbeat_at
        .is_some_and(|heartbeat| heartbeat <= now && now.saturating_sub(heartbeat) < 30)
}

#[cfg(test)]
mod tests {
    #[test]
    fn heartbeat_expiry_excludes_future_and_exact_timeout() {
        assert!(super::heartbeat_is_recent(Some(71), 100));
        for heartbeat in [None, Some(70), Some(101), Some(i64::MIN)] {
            assert!(!super::heartbeat_is_recent(heartbeat, 100));
        }
    }
}
