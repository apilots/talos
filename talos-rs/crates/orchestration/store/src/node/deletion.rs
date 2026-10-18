use super::DatabaseAgentNodeStore;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::database_error;

impl DatabaseAgentNodeStore {
    /// Materialize an owner-authenticated deletion confirmed by the authoritative
    /// directory, even when optional account synchronization is disabled.
    pub async fn reconcile_deleted_registration(
        &self,
        node_id: &str,
        owner: &str,
    ) -> TalosResult<()> {
        let mut tx = self.begin_write(node_id).await?;
        let existing: Option<String> = sqlx::query_scalar("SELECT owner_user_id FROM agent_nodes WHERE node_id = ? UNION ALL SELECT owner_user_id FROM computer_deletions WHERE node_id = ?")
            .bind(node_id).bind(node_id).fetch_optional(&mut *tx).await.map_err(|error| database_error("read deleted Computer owner", error))?;
        if existing
            .as_deref()
            .is_some_and(|existing| existing != owner)
        {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "Computer belongs to another account.",
            ));
        }
        sqlx::query("INSERT INTO computer_deletions(node_id, owner_user_id, deleted_at) VALUES (?, ?, ?) ON CONFLICT(node_id) DO NOTHING")
            .bind(node_id).bind(owner).bind(chrono::Utc::now().timestamp()).execute(&mut *tx).await.map_err(|error| database_error("retain authoritative Computer deletion", error))?;
        sqlx::query(
            "DELETE FROM agent_nodes WHERE node_id = ? AND owner_user_id = ? AND mode = 'remote'",
        )
        .bind(node_id)
        .bind(owner)
        .execute(&mut *tx)
        .await
        .map_err(|error| database_error("remove deleted Computer replica", error))?;
        tx.commit().await
    }

    pub(super) async fn read_deletion_owner(&self, node_id: &str) -> TalosResult<Option<String>> {
        sqlx::query_scalar("SELECT owner_user_id FROM computer_deletions WHERE node_id = ?")
            .bind(node_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| database_error("read Computer deletion", error))
    }

    /// Immediate write transaction excludes concurrent durable admission. Sync capture
    /// records the same deletion; no Project, Session, attachment or disk data is removed.
    pub(super) async fn delete_registration(
        &self,
        node_id: &str,
        owner: &str,
        now_at: i64,
    ) -> TalosResult<()> {
        let mut tx = self.begin_write(node_id).await?;
        let active: i64 = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sessions s WHERE s.execution_node_id = ? AND (
                EXISTS(SELECT 1 FROM agent_active_turns a WHERE a.session_id = s.session_key)
                OR (SELECT json_extract(e.payload_json, '$.type') FROM agent_events e
                    WHERE e.session_id = s.session_key AND e.sequence IS NOT NULL
                    ORDER BY e.event_seq DESC LIMIT 1) NOT IN ('turnCompleted', 'turnFailed')))",
        )
        .bind(node_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| database_error("check Computer active Turns", error))?;
        if active != 0 {
            return Err(TalosError::localized(
                talos_error::ErrorLocalization::ComputerDeletionActive,
            ));
        }
        let affected = sqlx::query(
            "DELETE FROM agent_nodes WHERE node_id = ? AND owner_user_id = ? AND mode = 'remote'",
        )
        .bind(node_id)
        .bind(owner)
        .execute(&mut *tx)
        .await
        .map_err(|error| database_error("delete Computer registration", error))?
        .rows_affected();
        if affected != 1 {
            return Err(TalosError::new(
                ErrorCode::ResourceNotFound,
                "Computer registration was not found.",
            ));
        }
        sqlx::query(
            "INSERT INTO computer_deletions(node_id, owner_user_id, deleted_at) VALUES (?, ?, ?)",
        )
        .bind(node_id)
        .bind(owner)
        .bind(now_at)
        .execute(&mut *tx)
        .await
        .map_err(|error| database_error("record Computer deletion", error))?;
        tx.commit().await
    }
}
