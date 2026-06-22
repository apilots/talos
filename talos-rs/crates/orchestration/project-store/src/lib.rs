use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use talos_error::TalosResult;
use talos_storage::{Row, Storage};
use talos_types::{ProjectPatch, ProjectStatus};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ProjectRecord {
    pub project_id: String,
    pub name: String,
    pub user_id: String,
    pub directory: PathBuf,
    pub status: ProjectStatus,
    pub created_at: i64,
    pub updated_at: i64,
    pub session_count: i64,
}

#[async_trait]
pub trait ProjectStore: Send + Sync {
    async fn list_projects(&self) -> TalosResult<Vec<ProjectRecord>>;

    async fn get_project(&self, project_id: &str) -> TalosResult<Option<ProjectRecord>>;

    async fn create_project(&self, name: &str, directory: &PathBuf) -> TalosResult<ProjectRecord>;

    async fn create_project_for_owner(
        &self,
        name: &str,
        directory: &PathBuf,
        user_id: &str,
    ) -> TalosResult<ProjectRecord>;

    async fn patch_project(
        &self,
        project_id: &str,
        patch: &ProjectPatch,
    ) -> TalosResult<Option<ProjectRecord>>;

    async fn delete_project(&self, project_id: &str) -> TalosResult<()>;
}

pub struct StorageProjectStore {
    storage: Arc<dyn Storage>,
}

impl StorageProjectStore {
    pub async fn new(storage: Arc<dyn Storage>) -> TalosResult<Self> {
        storage.init_schema().await?;
        Ok(Self { storage })
    }

    fn parse_project_record(row: &Row) -> ProjectRecord {
        ProjectRecord {
            project_id: row.get_string("project_id").unwrap_or_default(),
            name: row.get_string("name").unwrap_or_default(),
            user_id: row.get_string("user_id").unwrap_or_default(),
            directory: row
                .get_string("directory")
                .map(PathBuf::from)
                .unwrap_or_default(),
            status: ProjectStatus::default(),
            created_at: row.get_i64("created_at").unwrap_or_default(),
            updated_at: row.get_i64("updated_at").unwrap_or_default(),
            session_count: row.get_i64("session_count").unwrap_or_default(),
        }
    }

    async fn count_sessions_bound_to_project(&self, project_id: &str) -> TalosResult<i64> {
        let sql = "SELECT COUNT(*) AS count FROM sessions WHERE project_id = ?";
        let rows = self.storage.query(sql, &[project_id]).await?;
        Ok(rows
            .into_iter()
            .next()
            .and_then(|row| row.get_i64("count"))
            .unwrap_or_default())
    }
}

#[async_trait]
impl ProjectStore for StorageProjectStore {
    async fn list_projects(&self) -> TalosResult<Vec<ProjectRecord>> {
        let sql = "SELECT p.project_id, p.name, p.user_id, p.directory, p.created_at, p.updated_at, COUNT(s.session_key) AS session_count FROM projects p LEFT JOIN sessions s ON s.project_id = p.project_id GROUP BY p.project_id, p.name, p.user_id, p.directory, p.created_at, p.updated_at ORDER BY p.created_at DESC";
        let rows = self.storage.query(sql, &[]).await?;
        Ok(rows.iter().map(Self::parse_project_record).collect())
    }

    async fn get_project(&self, project_id: &str) -> TalosResult<Option<ProjectRecord>> {
        let sql =
            "SELECT p.project_id, p.name, p.user_id, p.directory, p.created_at, p.updated_at, COUNT(s.session_key) AS session_count FROM projects p LEFT JOIN sessions s ON s.project_id = p.project_id WHERE p.project_id = ? GROUP BY p.project_id, p.name, p.user_id, p.directory, p.created_at, p.updated_at";
        let rows = self.storage.query(sql, &[project_id]).await?;
        Ok(rows
            .into_iter()
            .next()
            .map(|row| Self::parse_project_record(&row)))
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
        let now = chrono::Utc::now().timestamp_millis();
        let record = ProjectRecord {
            project_id: uuid::Uuid::now_v7().to_string(),
            name: name.to_string(),
            user_id: user_id.to_string(),
            directory: directory.clone(),
            status: ProjectStatus::default(),
            created_at: now,
            updated_at: now,
            session_count: 0,
        };
        let row = Row::new(
            vec![
                "project_id".to_string(),
                "name".to_string(),
                "user_id".to_string(),
                "directory".to_string(),
                "created_at".to_string(),
                "updated_at".to_string(),
            ],
            vec![
                serde_json::Value::String(record.project_id.clone()),
                serde_json::Value::String(record.name.clone()),
                serde_json::Value::String(record.user_id.clone()),
                serde_json::Value::String(record.directory.display().to_string()),
                serde_json::Value::Number(now.into()),
                serde_json::Value::Number(now.into()),
            ],
        );
        self.storage.insert("projects", &row).await?;
        Ok(record)
    }

    async fn patch_project(
        &self,
        project_id: &str,
        patch: &ProjectPatch,
    ) -> TalosResult<Option<ProjectRecord>> {
        let Some(mut record) = self.get_project(project_id).await? else {
            return Ok(None);
        };
        if let Some(name) = patch.name.as_ref() {
            record.name = name.clone();
        }
        record.updated_at = chrono::Utc::now().timestamp_millis();
        let row = Row::new(
            vec!["name".to_string(), "updated_at".to_string()],
            vec![
                serde_json::Value::String(record.name.clone()),
                serde_json::Value::Number(record.updated_at.into()),
            ],
        );
        self.storage
            .update("projects", &row, "project_id = ?", &[project_id])
            .await?;
        Ok(Some(record))
    }

    async fn delete_project(&self, project_id: &str) -> TalosResult<()> {
        let bound_count = self.count_sessions_bound_to_project(project_id).await?;
        if bound_count > 0 {
            return Err(talos_error::TalosError::ValidationError(format!(
                "project {} still has bound sessions",
                project_id
            )));
        }
        self.storage
            .execute("DELETE FROM projects WHERE project_id = ?", &[project_id])
            .await?;
        Ok(())
    }
}

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
