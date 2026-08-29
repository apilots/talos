use std::path::PathBuf;

use async_trait::async_trait;
use talos_error::TalosResult;
use talos_types::{ProjectPatch, ProjectStatus};

use super::{ProjectRecord, ProjectStore};

#[derive(Default)]
pub struct MockProjectStore {
    projects: Vec<ProjectRecord>,
}

impl MockProjectStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_project(mut self, project: ProjectRecord) -> Self {
        self.projects.push(project);
        self
    }
}

#[async_trait]
impl ProjectStore for MockProjectStore {
    async fn list_projects(&self) -> TalosResult<Vec<ProjectRecord>> {
        Ok(self.projects.clone())
    }

    async fn get_project(&self, project_id: &str) -> TalosResult<Option<ProjectRecord>> {
        Ok(self
            .projects
            .iter()
            .find(|project| project.project_id == project_id)
            .cloned())
    }

    async fn count_projects_for_owner(&self, user_id: &str) -> TalosResult<u64> {
        Ok(self
            .projects
            .iter()
            .filter(|project| project.user_id == user_id)
            .count() as u64)
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
