use super::super::mapping::project_info_from_record;
use super::{ServiceResult, TalosService};
use crate::Actor;
use talos_types::{ProjectInfo, ProjectPatch};

impl TalosService {
    pub async fn list_projects(&self, actor: &Actor) -> ServiceResult<Vec<ProjectInfo>> {
        let projects = self.access.list_projects(actor).await?;
        Ok(projects
            .into_iter()
            .map(|project| self.project_info_with_status(project))
            .collect())
    }

    pub async fn get_project(&self, actor: &Actor, project_id: &str) -> ServiceResult<ProjectInfo> {
        let project = self.access.get_project(actor, project_id).await?;
        Ok(self.project_info_with_status(project))
    }

    pub async fn create_project(
        &self,
        actor: &Actor,
        name: String,
        directory: std::path::PathBuf,
    ) -> ServiceResult<ProjectInfo> {
        let project = self.access.create_project(actor, name, directory).await?;
        Ok(self.project_info_with_status(project))
    }

    pub async fn patch_project(
        &self,
        actor: &Actor,
        project_id: &str,
        patch: ProjectPatch,
    ) -> ServiceResult<ProjectInfo> {
        let project = self.access.patch_project(actor, project_id, patch).await?;
        Ok(self.project_info_with_status(project))
    }

    pub async fn delete_project(&self, actor: &Actor, project_id: &str) -> ServiceResult<()> {
        let _project = self.access.get_project(actor, project_id).await?;
        self.files.ensure_project_has_no_files(project_id).await?;
        self.access.delete_project(actor, project_id).await
    }

    fn project_info_with_status(
        &self,
        mut project: talos_store::project::ProjectRecord,
    ) -> ProjectInfo {
        project.status = self.fs.project_status_for_directory(&project.directory);
        project_info_from_record(project)
    }

    pub async fn read_fs_directory(
        &self,
        actor: &Actor,
        path: Option<std::path::PathBuf>,
    ) -> ServiceResult<talos_types::FsDirectorySnapshot> {
        self.fs.read_directory(actor, path).await
    }

    pub async fn create_fs_directory(
        &self,
        actor: &Actor,
        parent: std::path::PathBuf,
        name: String,
    ) -> ServiceResult<talos_types::FsDirectoryCreateResponse> {
        self.fs.create_directory(actor, parent, name).await
    }
}
