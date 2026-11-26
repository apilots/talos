//! In-process boundary lifecycle for an existing Worker runtime.

use agent_client::AgentHarnessRpcAdapter;
use agent_protocol::NodeAuth;
use std::sync::Arc;
use talos_error::TalosResult;
use tokio_util::sync::CancellationToken;

/// Embedded execution boundary sharing the adapter's admission and session gates.
/// Remote registration reuses the adapter through Worker-initiated Connect.
pub struct WorkerRpcHost {
    adapter: AgentHarnessRpcAdapter,
    finished: CancellationToken,
}

impl WorkerRpcHost {
    pub fn session_owner(&self) -> talos_agent::SessionOwner {
        self.adapter.session_owner()
    }

    pub fn adapter(&self) -> AgentHarnessRpcAdapter {
        self.adapter.clone()
    }

    /// Observe local shutdown; embedded execution has no network listener task.
    pub async fn stopped(&self) {
        self.finished.cancelled().await;
    }

    pub fn is_finished(&self) -> bool {
        self.finished.is_cancelled()
    }

    pub async fn shutdown(&self) -> TalosResult<()> {
        self.adapter.clear_local_auth();
        self.adapter.clear_registration();
        self.finished.cancel();
        Ok(())
    }
}

impl Drop for WorkerRpcHost {
    fn drop(&mut self) {
        self.adapter.clear_local_auth();
        self.adapter.clear_registration();
        self.finished.cancel();
    }
}

impl crate::TalosWorker {
    /// Builds a listener-free library entry to these exact Harness instances.
    /// The private process credential is not a cloud lease or a public grant.
    pub async fn start_local(
        &self,
        session_kind: talos_error::SessionOwnerKind,
        session_hold: talos_agent::SessionHold,
    ) -> TalosResult<(Arc<WorkerRpcHost>, NodeAuth)> {
        let auth = NodeAuth {
            node_id: "embedded".into(),
            token: format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            ),
        };
        let adapter = AgentHarnessRpcAdapter::new(
            auth.node_id.clone(),
            self.config(),
            self.harnesses().to_vec(),
        )
        .with_environment(self.environment())
        .with_resource_binder(self.resource_binder())
        .with_session_policy(session_kind, session_hold)
        .with_registration_revocation_hook({
            let plugins = self.plugin_manager();
            Arc::new(move || plugins.cancel_remote_mcp_authorizations())
        });
        adapter.install_local_auth(&auth)?;
        Ok((
            Arc::new(WorkerRpcHost {
                adapter,
                finished: CancellationToken::new(),
            }),
            auth,
        ))
    }

    pub fn local_harnesses(
        &self,
        host: &WorkerRpcHost,
        auth: &NodeAuth,
        coordinator_id: &str,
    ) -> TalosResult<Vec<(String, Arc<dyn agent_protocol::AgentHarness>)>> {
        self.deployments()
            .into_iter()
            .map(|deployment| {
                let harness = host.adapter.local_harness(
                    deployment.agent_id.clone(),
                    auth.clone(),
                    coordinator_id.to_owned(),
                )?;
                Ok((deployment.agent_id, harness))
            })
            .collect()
    }

    pub fn local_environment(
        &self,
        host: &WorkerRpcHost,
        auth: NodeAuth,
    ) -> TalosResult<Arc<dyn agent_protocol::WorkerEnvironment>> {
        host.adapter.local_environment(auth)
    }
}
