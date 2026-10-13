pub use talos_types::{
    CreateNodeRequest, CreatedNode, GetNodeRequest, ListNodesRequest, NodeCredential, NodeHealth,
    NodeInfo, NodeMode, NodePage, NodeStatus, NodeSystemInfo, RotateCredentialRequest,
    UpdateNodeRequest,
};

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
    pub system_info: Option<NodeSystemInfo>,
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
pub struct HeartbeatParams {
    pub health: NodeHealth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeartbeatResult {
    pub received_at: i64,
    pub registration_required: bool,
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
