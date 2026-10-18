use super::super::mapping::project_info_from_record;
use super::{ServiceResult, TalosService};
use crate::Actor;
use agent_protocol::WorkerEnvironment;
use std::sync::Arc;
use talos_types::*;

impl TalosService {
    pub(crate) async fn worker_environment(
        &self,
        actor: &Actor,
        node_id: Option<&str>,
    ) -> ServiceResult<Arc<dyn WorkerEnvironment>> {
        let node = self
            .connect_selected_computer(actor, node_id.unwrap_or("embedded"))
            .await?;
        self.access.worker_environment(actor, &node).await
    }
    pub async fn workspace_info(
        &self,
        actor: &Actor,
        request: WorkspaceInfoRequest,
    ) -> ServiceResult<WorkspaceInfoResponse> {
        self.worker_environment(actor, request.node_id.as_deref())
            .await?
            .workspace_info(actor.worker_environment_context())
            .await
    }
    pub async fn list_projects(
        &self,
        actor: &Actor,
        mut request: ProjectListRequest,
    ) -> ServiceResult<Vec<ProjectInfo>> {
        if request
            .node_id
            .as_ref()
            .is_some_and(|id| id.trim().is_empty())
        {
            return Err(talos_error::TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Node ID cannot be empty",
            ));
        }
        if let Some(node) = request.node_id.take() {
            request.node_id = Some(self.resolve_computer_metadata_node(actor, &node).await?);
        }
        let projects = self.access.list_projects(actor).await?;
        let mut result = Vec::new();
        for project in projects {
            if request
                .node_id
                .as_ref()
                .is_none_or(|node_id| node_id == &project.execution_node_id)
            {
                result.push(self.project_info_with_status(actor, project).await?);
            }
        }
        Ok(result)
    }
    pub async fn get_project(&self, actor: &Actor, project_id: &str) -> ServiceResult<ProjectInfo> {
        let project = self.access.get_project(actor, project_id).await?;
        self.project_info_with_status(actor, project).await
    }
    pub async fn create_project(
        &self,
        actor: &Actor,
        mut request: ProjectCreateRequest,
    ) -> ServiceResult<ProjectInfo> {
        request.node_id = Some(
            self.connect_selected_computer(actor, request.node_id.as_deref().unwrap_or("embedded"))
                .await?,
        );
        let project = self.access.create_project(actor, request).await?;
        self.project_info_with_status(actor, project).await
    }
    pub async fn patch_project(
        &self,
        actor: &Actor,
        project_id: &str,
        patch: ProjectPatch,
    ) -> ServiceResult<ProjectInfo> {
        let project = self.access.patch_project(actor, project_id, patch).await?;
        self.project_info_with_status(actor, project).await
    }
    pub async fn delete_project(&self, actor: &Actor, project_id: &str) -> ServiceResult<()> {
        self.access.get_project(actor, project_id).await?;
        self.files.ensure_project_has_no_files(project_id).await?;
        self.access.delete_project(actor, project_id).await
    }
    async fn project_info_with_status(
        &self,
        actor: &Actor,
        mut project: talos_store::project::ProjectRecord,
    ) -> ServiceResult<ProjectInfo> {
        let status = async {
            self.worker_environment(actor, Some(&project.execution_node_id))
                .await?
                .project_status(
                    actor.worker_environment_context(),
                    project.directory.clone(),
                )
                .await
        }
        .await;
        project.status = match status {
            Ok(status) => status,
            Err(error)
                if matches!(
                    error.error_code(),
                    talos_error::ErrorCode::ServiceUnavailable
                        | talos_error::ErrorCode::Timeout
                        | talos_error::ErrorCode::ResourceExpired
                        | talos_error::ErrorCode::ResourceNotFound
                ) =>
            {
                ProjectStatus {
                    state: ProjectStatusState::NodeUnavailable,
                    reason: Some(ProjectUnavailableReason::NodeUnavailable),
                }
            }
            Err(error) => return Err(error),
        };
        self.project_for_client(actor, project_info_from_record(project))
            .await
    }
    pub async fn read_fs_directory(
        &self,
        actor: &Actor,
        request: FsDirectoryReadRequest,
    ) -> ServiceResult<FsDirectorySnapshot> {
        self.worker_environment(actor, request.node_id.as_deref())
            .await?
            .read_directory(actor.worker_environment_context(), request)
            .await
    }
    pub async fn create_fs_directory(
        &self,
        actor: &Actor,
        request: FsDirectoryCreateRequest,
    ) -> ServiceResult<FsDirectoryCreateResponse> {
        self.worker_environment(actor, request.node_id.as_deref())
            .await?
            .create_directory(actor.worker_environment_context(), request)
            .await
    }
}
