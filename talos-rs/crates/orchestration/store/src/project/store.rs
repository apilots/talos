use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use talos_error::{TalosError, TalosResult};
use talos_storage::{Row, Storage};
use talos_types::{ProjectPatch, ProjectStatus};

use super::ProjectRecord;

#[async_trait]
/// Persist project records; callers lend paths and implementations own stored copies.
pub trait ProjectStore: Send + Sync {
    async fn list_projects(&self) -> TalosResult<Vec<ProjectRecord>>;

    async fn get_project(&self, project_id: &str) -> TalosResult<Option<ProjectRecord>>;

    /// Returns the current number of projects owned by one user.
    async fn count_projects_for_owner(&self, user_id: &str) -> TalosResult<u64>;

    async fn create_project(&self, name: &str, directory: &Path) -> TalosResult<ProjectRecord>;

    async fn create_project_for_owner(
        &self,
        name: &str,
        directory: &Path,
        user_id: &str,
    ) -> TalosResult<ProjectRecord>;

    /// Create one immutable owner/node/directory binding after Worker path validation.
    async fn create_project_on_node(
        &self,
        name: &str,
        directory: &Path,
        user_id: &str,
        node_id: &str,
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
        Ok(Self { storage })
    }

    fn parse_project_record(row: &Row) -> ProjectRecord {
        ProjectRecord {
            execution_node_id: row.get_string("execution_node_id").unwrap_or_default(),
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
        let sql = "SELECT p.project_id, p.name, p.user_id, p.execution_node_id, p.directory, p.created_at, p.updated_at, COUNT(s.session_key) AS session_count FROM projects p LEFT JOIN sessions s ON s.project_id = p.project_id GROUP BY p.project_id, p.name, p.user_id, p.execution_node_id, p.directory, p.created_at, p.updated_at ORDER BY p.created_at DESC";
        let rows = self.storage.query(sql, &[]).await?;
        Ok(rows.iter().map(Self::parse_project_record).collect())
    }

    async fn get_project(&self, project_id: &str) -> TalosResult<Option<ProjectRecord>> {
        let sql =
            "SELECT p.project_id, p.name, p.user_id, p.execution_node_id, p.directory, p.created_at, p.updated_at, COUNT(s.session_key) AS session_count FROM projects p LEFT JOIN sessions s ON s.project_id = p.project_id WHERE p.project_id = ? GROUP BY p.project_id, p.name, p.user_id, p.execution_node_id, p.directory, p.created_at, p.updated_at";
        let rows = self.storage.query(sql, &[project_id]).await?;
        Ok(rows
            .into_iter()
            .next()
            .map(|row| Self::parse_project_record(&row)))
    }

    async fn count_projects_for_owner(&self, user_id: &str) -> TalosResult<u64> {
        let rows = self
            .storage
            .query(
                "SELECT COUNT(*) AS project_count FROM projects WHERE user_id = ?",
                &[user_id],
            )
            .await?;
        let row = rows
            .first()
            .ok_or_else(|| TalosError::internal("count projects returned no aggregate row"))?;
        let count = row
            .get_i64("project_count")
            .ok_or_else(|| TalosError::internal("count projects returned an invalid total"))?;
        u64::try_from(count)
            .map_err(|_| TalosError::internal("count projects returned a negative total"))
    }

    async fn create_project(&self, name: &str, directory: &Path) -> TalosResult<ProjectRecord> {
        self.create_project_for_owner(name, directory, "").await
    }

    async fn create_project_for_owner(
        &self,
        name: &str,
        directory: &Path,
        user_id: &str,
    ) -> TalosResult<ProjectRecord> {
        self.create_project_on_node(name, directory, user_id, "embedded")
            .await
    }

    async fn create_project_on_node(
        &self,
        name: &str,
        directory: &Path,
        user_id: &str,
        node_id: &str,
    ) -> TalosResult<ProjectRecord> {
        let now = chrono::Utc::now().timestamp_millis();
        let record = ProjectRecord {
            execution_node_id: node_id.to_owned(),
            project_id: uuid::Uuid::now_v7().to_string(),
            name: name.to_string(),
            user_id: user_id.to_string(),
            directory: directory.to_path_buf(),
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
                "execution_node_id".to_string(),
                "directory".to_string(),
                "created_at".to_string(),
                "updated_at".to_string(),
            ],
            vec![
                serde_json::Value::String(record.project_id.clone()),
                serde_json::Value::String(record.name.clone()),
                serde_json::Value::String(record.user_id.clone()),
                serde_json::Value::String(record.execution_node_id.clone()),
                serde_json::Value::String(record.directory.display().to_string()),
                serde_json::Value::Number(now.into()),
                serde_json::Value::Number(now.into()),
            ],
        );
        self.storage.insert("projects", &row).await.map_err(|error| {
            let unique = matches!(std::error::Error::source(&error).and_then(|source| source.downcast_ref::<sqlx::Error>()), Some(sqlx::Error::Database(database)) if database.is_unique_violation());
            if unique { TalosError::from_source(talos_error::ErrorCode::Conflict, "Project directory already exists for this user and Worker", error) } else { error }
        })?;
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
            return Err(talos_error::TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                format!("project {} still has bound sessions", project_id),
            ));
        }
        let deleted = self
            .storage
            .execute(
                r#"
                DELETE FROM projects
                WHERE project_id = ?
                  AND NOT EXISTS (
                      SELECT 1 FROM sessions WHERE sessions.project_id = projects.project_id
                  )
                  AND NOT EXISTS (
                      SELECT 1 FROM files WHERE files.project_id = projects.project_id
                  )
                "#,
                &[project_id],
            )
            .await?;
        if deleted == 0 && self.get_project(project_id).await?.is_some() {
            return Err(TalosError::new(
                talos_error::ErrorCode::Conflict,
                format!("project {project_id} still has bound sessions or files"),
            ));
        }
        Ok(())
    }
}
