use talos_error::TalosResult;
use talos_service::Actor;
use talos_types::{
    ProjectCreateRequest, ProjectCreateResponse, ProjectDeleteRequest, ProjectDeleteResponse,
    ProjectInfo, ProjectListRequest, ProjectListResponse, ProjectPatchRequest,
    ProjectPatchResponse, ProjectReadRequest, ProjectReadResponse,
};

use crate::gateway_service::{require_non_empty, ServiceGateway};

impl ServiceGateway {
    pub(crate) async fn list_projects(
        &self,
        actor: &Actor,
        _request: ProjectListRequest,
    ) -> TalosResult<ProjectListResponse> {
        let projects = self.service.list_projects(actor).await?;
        Ok(ProjectListResponse {
            projects: projects.into_iter().map(ProjectInfo::from).collect(),
        })
    }

    pub(crate) async fn get_project(
        &self,
        actor: &Actor,
        request: ProjectReadRequest,
    ) -> TalosResult<ProjectReadResponse> {
        require_non_empty(&request.project_id, "project_id")?;
        let project = self.service.get_project(actor, &request.project_id).await?;
        Ok(ProjectReadResponse {
            project: ProjectInfo::from(project),
        })
    }

    pub(crate) async fn create_project(
        &self,
        actor: &Actor,
        request: ProjectCreateRequest,
    ) -> TalosResult<ProjectCreateResponse> {
        let project = self
            .service
            .create_project(actor, request.name, request.directory)
            .await?;
        Ok(ProjectCreateResponse {
            project: ProjectInfo::from(project),
        })
    }

    pub(crate) async fn patch_project(
        &self,
        actor: &Actor,
        request: ProjectPatchRequest,
    ) -> TalosResult<ProjectPatchResponse> {
        require_non_empty(&request.project_id, "project_id")?;
        let project_id = request.project_id.clone();
        let project = self
            .service
            .patch_project(actor, &project_id, request.into_patch())
            .await?;
        Ok(ProjectPatchResponse {
            project: ProjectInfo::from(project),
        })
    }

    pub(crate) async fn delete_project(
        &self,
        actor: &Actor,
        request: ProjectDeleteRequest,
    ) -> TalosResult<ProjectDeleteResponse> {
        require_non_empty(&request.project_id, "project_id")?;
        self.service
            .delete_project(actor, &request.project_id)
            .await?;
        Ok(ProjectDeleteResponse { success: true })
    }
}
