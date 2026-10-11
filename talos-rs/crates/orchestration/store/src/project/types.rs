use std::path::PathBuf;

use talos_types::ProjectStatus;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ProjectRecord {
    pub execution_node_id: String,
    pub project_id: String,
    pub name: String,
    pub user_id: String,
    pub directory: PathBuf,
    pub status: ProjectStatus,
    pub created_at: i64,
    pub updated_at: i64,
    pub session_count: i64,
}
