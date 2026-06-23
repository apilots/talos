use std::path::PathBuf;
use std::sync::Arc;

use talos_error::TalosResult;
use talos_store::project::{ProjectRecord, ProjectStore};
use talos_types::{validate_project_directory, ProjectPatch};

#[derive(Clone)]
pub struct ProjectService {
    project_store: Arc<dyn ProjectStore>,
}

impl ProjectService {
    pub fn new(project_store: Arc<dyn ProjectStore>) -> Self {
        Self { project_store }
    }

    pub async fn list_projects(&self) -> TalosResult<Vec<ProjectRecord>> {
        self.project_store.list_projects().await
    }

    pub async fn get_project(&self, project_id: &str) -> TalosResult<Option<ProjectRecord>> {
        self.project_store.get_project(project_id).await
    }

    pub async fn create_project(
        &self,
        name: String,
        directory: PathBuf,
        user_id: String,
    ) -> TalosResult<ProjectRecord> {
        validate_project_directory(&directory)?;
        self.project_store
            .create_project_for_owner(&name, &directory, &user_id)
            .await
    }

    pub async fn patch_project(
        &self,
        project_id: &str,
        patch: ProjectPatch,
    ) -> TalosResult<Option<ProjectRecord>> {
        self.project_store.patch_project(project_id, &patch).await
    }

    pub async fn delete_project(&self, project_id: &str) -> TalosResult<()> {
        self.project_store.delete_project(project_id).await
    }
}
