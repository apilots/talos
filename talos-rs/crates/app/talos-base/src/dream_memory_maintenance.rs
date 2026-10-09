use std::sync::Arc;

use talos_config::{DreamMemoryMaintenanceConfig, TalosPaths, WorkspaceConfig};
use talos_error::TalosResult;
use talos_service::{InnerModelClient, TalosService};
use talos_store::session::{SessionPageCursor, SessionQuery, SessionStore};
use talos_types::ModelUsageKind;

/// Result of one bounded global Dream Memory maintenance run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct DreamMemoryMaintenanceReport {
    pub processed_sessions: u64,
    pub failed_sessions: u64,
}

/// Selects bounded Sessions and delegates formation to their concrete Provider.
#[derive(Clone)]
pub(super) struct DreamMemoryMaintenance {
    session_store: Arc<dyn SessionStore>,
    service: Arc<TalosService>,
    coordinator: Arc<dyn talos_coordinator::AgentCoordinator>,
    inner_model: Arc<InnerModelClient>,
    workspaces: WorkspaceConfig,
    workspace_paths: TalosPaths,
    config: DreamMemoryMaintenanceConfig,
}

impl DreamMemoryMaintenance {
    pub(super) fn new(
        session_store: Arc<dyn SessionStore>,
        service: Arc<TalosService>,
        coordinator: Arc<dyn talos_coordinator::AgentCoordinator>,
        inner_model: Arc<InnerModelClient>,
        runtime_config: &talos_config::Config,
        config: DreamMemoryMaintenanceConfig,
    ) -> Self {
        Self {
            session_store,
            service,
            coordinator,
            inner_model,
            workspaces: runtime_config.workspaces.clone(),
            workspace_paths: runtime_config.talos_paths(),
            config,
        }
    }

    pub(super) async fn run(&self) -> TalosResult<DreamMemoryMaintenanceReport> {
        let mut report = DreamMemoryMaintenanceReport::default();
        let mut after = None;
        for _ in 0..self.config.max_batches {
            let records = self
                .session_store
                .list_session_records(&SessionQuery {
                    limit: Some(self.config.batch_size),
                    after: after.clone(),
                    ..SessionQuery::default()
                })
                .await?;
            let record_count = records.len();
            after = records.last().map(|record| SessionPageCursor {
                last_activity: record.last_activity,
                session_id: record.id.clone(),
            });
            for record in records {
                let session_id = record.id.clone();
                match self.process_session(record).await {
                    Ok(true) => report.processed_sessions += 1,
                    Ok(false) => {}
                    Err(error) => {
                        report.failed_sessions += 1;
                        tracing::warn!(session_id, error = %error, "Dream Memory session processing failed");
                    }
                }
            }
            if record_count < self.config.batch_size {
                break;
            }
        }
        Ok(report)
    }

    async fn process_session(
        &self,
        record: talos_store::session::SessionRecord,
    ) -> TalosResult<bool> {
        let workspace = self.workspaces.resolve(
            &self.workspace_paths,
            &record.workspace_id,
            &record.user_id,
        )?;
        let context = talos_agent::AgentSessionContext {
            agent_id: record.agent_id.clone(),
            session_id: record.id.clone(),
            workspace: workspace.clone(),
        };
        let harness = self.coordinator.session_harness(&context).await?;
        let inner_model = self.inner_model.clone();
        let user_id = record.user_id.clone();
        let session_id = record.id.clone();
        let result = self
            .service
            .run_idle_session_maintenance(&record.id, move || async move {
                harness
                    .form_dream_memory(
                        context,
                        Arc::new(move |messages| {
                            let inner_model = inner_model.clone();
                            let user_id = user_id.clone();
                            let session_id = session_id.clone();
                            Box::pin(async move {
                                inner_model
                                    .chat(talos_service::InnerModelChatParams {
                                        user_id,
                                        session_id: Some(session_id),
                                        usage_kind: ModelUsageKind::Dream,
                                        messages,
                                    })
                                    .await
                            })
                        }),
                    )
                    .await
            })
            .await?;
        Ok(result.flatten().is_some())
    }
}

#[cfg(test)]
#[path = "dream_memory_maintenance_tests.rs"]
mod tests;
