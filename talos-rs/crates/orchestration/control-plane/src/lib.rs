//! Authoritative node directory, publication and managed Worker lifecycle.
//! Product services and transport adapters compose this library through narrow ports.

pub use agent_protocol::NodeAdmissionGates;
pub use talos_auth::Actor;

mod service;
mod types;

pub use agent_protocol::{
    CreateNodeRequest, GetNodeRequest, ListNodesRequest, RotateCredentialRequest, UpdateNodeRequest,
};
pub use service::ManagedDispatch;
pub use service::{AgentNodeRevocationHook, AgentNodeService};
/// The existing transport owner atomically verifies Worker idleness and revokes remote admission.
pub type AgentNodeDeletionHook = std::sync::Arc<
    dyn Fn(
            agent_protocol::RegistrationAdmission,
        ) -> agent_protocol::AgentFuture<'static, talos_error::TalosResult<()>>
        + Send
        + Sync,
>;
pub use types::NodePrincipal;

#[cfg(test)]
#[path = "agent_node_tests.rs"]
mod tests;

#[cfg(test)]
mod computers_tests;

#[cfg(test)]
mod deletion_tests;

/// Trusted product binding used to authorize one node lease without a product record.
pub struct SessionLeaseBinding<'a> {
    pub execution_node_id: &'a str,
    pub user_id: &'a str,
}

/// Product-owned conversation fields needed to resolve a managed node.
pub struct ManagedConversationBinding<'a> {
    pub session_id: &'a str,
    pub project_id: Option<&'a str>,
    pub node_id: Option<&'a str>,
}

/// Observes verified directory access without importing a replication implementation.
pub type NodeDirectoryObserver = std::sync::Arc<dyn Fn(&Actor) + Send + Sync>;

pub use service::ensure_managed_worker_access;

#[cfg(test)]
mod test_support {
    pub async fn run_test_migrations(pool: &sqlx::SqlitePool) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../conf/migrations/sqlite");
        talos_store::migration::run_migrations(pool, &path)
            .await
            .unwrap();
    }
}
