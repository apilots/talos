use std::path::PathBuf;

use async_trait::async_trait;
use talos_error::TalosResult;
use talos_types::{ProjectPatch, ProjectStatus};

use super::{ProjectRecord, ProjectStore};

#[derive(Default)]
pub struct MockProjectStore;

impl MockProjectStore {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ProjectStore for MockProjectStore {
    async fn list_projects(&self) -> TalosResult<Vec<ProjectRecord>> {
        Ok(Vec::new())
    }

    async fn get_project(&self, _project_id: &str) -> TalosResult<Option<ProjectRecord>> {
        Ok(None)
    }

    async fn create_project(&self, name: &str, directory: &PathBuf) -> TalosResult<ProjectRecord> {
        self.create_project_for_owner(name, directory, "").await
    }

    async fn create_project_for_owner(
        &self,
        name: &str,
        directory: &PathBuf,
        user_id: &str,
    ) -> TalosResult<ProjectRecord> {
        Ok(ProjectRecord {
            project_id: "project-mock".to_string(),
            name: name.to_string(),
            user_id: user_id.to_string(),
            directory: directory.clone(),
            status: ProjectStatus::default(),
            created_at: 0,
            updated_at: 0,
            session_count: 0,
        })
    }

    async fn patch_project(
        &self,
        _project_id: &str,
        _patch: &ProjectPatch,
    ) -> TalosResult<Option<ProjectRecord>> {
        Ok(None)
    }

    async fn delete_project(&self, _project_id: &str) -> TalosResult<()> {
        Ok(())
    }
}
