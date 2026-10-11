//! Resolve execution inputs from the pinned Worker, including embedded execution.
use super::*;
use agent_protocol::{AgentSessionContext, WorkerEnvironment, WorkerEnvironmentContext};
use talos_config::EffectiveWorkspace;
use talos_types::{ModelInfo, ModelSelection};

impl SessionService {
    pub(super) fn environment_context(
        user_id: &str,
        workspace_id: Option<String>,
    ) -> WorkerEnvironmentContext {
        WorkerEnvironmentContext {
            user_id: user_id.to_owned(),
            username: None,
            workspace_id,
        }
    }
    pub(super) async fn worker_environment(
        &self,
        node_id: &str,
        user_id: &str,
    ) -> TalosResult<Arc<dyn WorkerEnvironment>> {
        self.coordinator.worker_environment(node_id, user_id).await
    }
    pub(super) async fn model_info_for_selection(
        &self,
        node_id: &str,
        user_id: &str,
        selection: &ModelSelection,
    ) -> TalosResult<ModelInfo> {
        let catalog = self
            .worker_environment(node_id, user_id)
            .await?
            .list_models(Self::environment_context(user_id, None))
            .await?;
        let model = catalog
            .models
            .into_iter()
            .find(|model| {
                model.provider == selection.model_ref.provider
                    && model.model.id == selection.model_ref.id
            })
            .ok_or_else(|| {
                TalosError::localized(talos_error::ErrorLocalization::ModelNotConfigured {
                    provider: selection.model_ref.provider.clone(),
                    model: selection.model_ref.id.clone(),
                })
            })?;
        selection.resolve_reasoning(&model)?;
        Ok(model)
    }
    /// Resolve only the saved Workspace ID. Never rebind to a changed Worker default.
    pub(crate) async fn build_agent_session_context(
        &self,
        record: &SessionRecord,
    ) -> TalosResult<AgentSessionContext> {
        if record.agent_id.trim().is_empty() {
            return Err(TalosError::internal("Session is missing its Agent binding"));
        }
        let workspace = self
            .worker_environment(&record.execution_node_id, &record.user_id)
            .await?
            .workspace_info(Self::environment_context(
                &record.user_id,
                Some(record.workspace_id.clone()),
            ))
            .await?;
        Ok(AgentSessionContext {
            agent_id: record.agent_id.clone(),
            session_id: record.id.clone(),
            workspace: EffectiveWorkspace {
                workspace_id: workspace.workspace_id,
                user_id: record.user_id.clone(),
                root: workspace.user_workspace_dir.into(),
            },
        })
    }
    pub(super) async fn normalize_current_directory(
        &self,
        node_id: &str,
        user_id: &str,
        mode: TalosConnectionMode,
        directory: Option<PathBuf>,
    ) -> TalosResult<Option<PathBuf>> {
        match (mode, directory) {
            (TalosConnectionMode::Remote, _) | (_, None) => Ok(None),
            (TalosConnectionMode::Embedded, Some(directory)) => self
                .worker_environment(node_id, user_id)
                .await?
                .validate_project_directory(Self::environment_context(user_id, None), directory)
                .await
                .map(Some),
        }
    }
}
