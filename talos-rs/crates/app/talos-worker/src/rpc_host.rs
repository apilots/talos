//! In-process boundary lifecycle for an existing Worker runtime.

use agent_client::AgentHarnessRpcAdapter;
use agent_protocol::NodeAuth;
use std::sync::Arc;
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio_util::sync::CancellationToken;

pub(crate) struct WorkerBoundary {
    host: Arc<WorkerRpcHost>,
    auth: NodeAuth,
    node_id: String,
    kind: talos_error::SessionOwnerKind,
    hold: talos_agent::SessionHold,
}

/// Embedded execution boundary sharing the adapter's admission and session gates.
/// Remote registration reuses the same adapter through its outbound connection.
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
        self.execution_host("embedded", session_kind, session_hold, || {
            AgentHarnessRpcAdapter::new("embedded".into(), self.config(), self.harnesses().to_vec())
                .with_environment(self.environment())
                .with_resource_binder(self.resource_binder())
                .with_session_policy(session_kind, session_hold)
                .with_registration_revocation_hook({
                    let plugins = self.plugin_manager();
                    Arc::new(move || plugins.cancel_remote_mcp_authorizations())
                })
        })
    }

    pub(crate) fn execution_host(
        &self,
        node_id: &str,
        kind: talos_error::SessionOwnerKind,
        hold: talos_agent::SessionHold,
        build: impl FnOnce() -> AgentHarnessRpcAdapter,
    ) -> TalosResult<(Arc<WorkerRpcHost>, NodeAuth)> {
        let mut boundary = self
            .execution_boundary
            .lock()
            .expect("Worker boundary lock");
        if let Some(existing) = boundary.as_ref() {
            if existing.node_id != node_id || existing.kind != kind || existing.hold != hold {
                return Err(TalosError::new(
                    ErrorCode::Conflict,
                    "Worker execution entry policy is already fixed.",
                ));
            }
            if existing.host.is_finished() {
                return Err(TalosError::new(
                    ErrorCode::ServiceUnavailable,
                    "Worker execution entry has stopped.",
                ));
            }
            return Ok((existing.host.clone(), existing.auth.clone()));
        }
        let adapter = build();
        let auth = NodeAuth {
            node_id: node_id.into(),
            token: format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            ),
        };
        adapter.install_local_auth(&auth)?;
        let host = Arc::new(WorkerRpcHost {
            adapter,
            finished: CancellationToken::new(),
        });
        *boundary = Some(WorkerBoundary {
            host: host.clone(),
            auth: auth.clone(),
            node_id: node_id.into(),
            kind,
            hold,
        });
        Ok((host, auth))
    }

    pub(crate) async fn stop_execution_host(&self) -> TalosResult<()> {
        let host = {
            self.execution_boundary
                .lock()
                .expect("Worker boundary lock")
                .as_ref()
                .map(|boundary| boundary.host.clone())
        };
        if let Some(host) = host {
            host.shutdown().await?;
        }
        Ok(())
    }

    pub(crate) fn ensure_execution_host(&self, host: &WorkerRpcHost) -> TalosResult<()> {
        let boundary = self
            .execution_boundary
            .lock()
            .expect("Worker boundary lock");
        if !boundary
            .as_ref()
            .is_some_and(|entry| std::ptr::eq(entry.host.as_ref(), host))
        {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Execution entry belongs to a different Worker instance.",
            ));
        }
        Ok(())
    }

    pub fn local_harnesses(
        &self,
        host: &WorkerRpcHost,
        auth: &NodeAuth,
        coordinator_id: &str,
    ) -> TalosResult<Vec<(String, Arc<dyn agent_protocol::AgentHarness>)>> {
        self.ensure_execution_host(host)?;
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
        self.ensure_execution_host(host)?;
        host.adapter.local_environment(auth)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use talos_fs::FileSystem;
    struct UnusedFiles;
    impl talos_agent::AttachmentReader for UnusedFiles {
        fn read<'a>(
            &'a self,
            _: &'a talos_agent::TurnFileScope,
            _: &'a [String],
        ) -> talos_agent::AgentFuture<'a, TalosResult<Vec<talos_agent::ResolvedAttachment>>>
        {
            Box::pin(async { Ok(vec![]) })
        }
    }
    impl talos_tools::ToolImageStore for UnusedFiles {
        fn save<'a>(
            &'a self,
            _: &'a str,
            _: Vec<talos_llm_provider::Attachment>,
        ) -> talos_agent::AgentFuture<'a, TalosResult<Vec<talos_types::FileInfo>>> {
            Box::pin(async { Ok(vec![]) })
        }
        fn read<'a>(
            &'a self,
            _: &'a str,
            _: &'a [String],
        ) -> talos_agent::AgentFuture<'a, TalosResult<Vec<talos_llm_provider::Attachment>>>
        {
            Box::pin(async { Ok(vec![]) })
        }
    }

    #[tokio::test]
    async fn repeated_entries_share_one_boundary_and_adapter_retains_namespace_ownership() {
        let home = tempfile::tempdir().unwrap();
        let config = talos_config::Config::default()
            .with_runtime_paths(talos_config::TalosPaths::from_home(home.path().to_owned()));
        let lock = config.talos_paths().data_dir.join("worker/execution.lock");
        let worker = crate::TalosWorker::build(
            config,
            crate::TalosWorkerResources {
                attachment_reader: Arc::new(UnusedFiles),
                tool_image_store: Arc::new(UnusedFiles),
                agent_control: None,
                automation_control: None,
            },
        )
        .await
        .unwrap();
        let (first, auth) = worker
            .start_local(
                talos_error::SessionOwnerKind::Server,
                talos_agent::SessionHold::Turn,
            )
            .await
            .unwrap();
        let (second, second_auth) = worker
            .clone()
            .start_local(
                talos_error::SessionOwnerKind::Server,
                talos_agent::SessionHold::Turn,
            )
            .await
            .unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert!(auth.node_id == second_auth.node_id && auth.token == second_auth.token);
        assert!(worker
            .start_local(
                talos_error::SessionOwnerKind::Tui,
                talos_agent::SessionHold::Attached
            )
            .await
            .err()
            .unwrap()
            .is_code(ErrorCode::Conflict));
        let adapter = first.adapter();
        drop(first);
        drop(second);
        drop(worker);
        assert!(talos_fs::std_fs().try_file_lock(&lock).unwrap().is_none());
        drop(adapter);
        assert!(talos_fs::std_fs().try_file_lock(&lock).unwrap().is_some());
    }
}
