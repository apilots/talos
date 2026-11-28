//! Authorized synchronization capture and explicit local history claiming.
use super::DatabaseSessionStore;
#[cfg(feature = "sync")]
use std::sync::Arc;
use talos_error::{TalosError, TalosResult};

use crate::capture;
impl DatabaseSessionStore {
    /// Preserve the canonical identity of historical local records before an
    /// explicitly deleted enrollment is replaced. New unregistered local records
    /// are excluded from Sync until an intentional enrollment supplies a new alias.
    pub async fn retire_local_registration(
        &self,
        user_id: &str,
        computer_id: &str,
    ) -> TalosResult<()> {
        let mut tx = self.storage.begin_transaction().await?;
        crate::session::capture_removal_boundary(&mut *tx, user_id, computer_id).await?;
        if !tx.query("SELECT 1 FROM local_computer_retirements WHERE node_id = ? AND owner_user_id = ? AND finalized = 1", &[computer_id, user_id]).await?.is_empty() {
            tx.commit().await?;
            return Ok(());
        }
        if !tx.query("SELECT 1 FROM sessions s JOIN agent_active_turns a ON a.session_id = s.session_key JOIN local_computer_retirement_resources r ON r.resource_id = s.session_key AND r.kind = 'session' WHERE r.node_id = ? AND s.execution_node_id = 'embedded' AND s.user_id = ? LIMIT 1", &[computer_id, user_id]).await?.is_empty() {
            return Err(TalosError::localized(talos_error::ErrorLocalization::ComputerDeletionActive));
        }
        let mut resources = Vec::new();
        for row in tx.query("SELECT resource_id AS id FROM local_computer_retirement_resources WHERE node_id = ? AND kind = 'project'", &[computer_id]).await? {
            resources.push(crate::capture::ResourceKey::Project { project_id: row.get_string("id").expect("Project key") });
        }
        let cuts = capture::prepare(&mut *tx, resources).await?;
        tx.execute("UPDATE sessions SET execution_node_id = ? WHERE execution_node_id = 'embedded' AND user_id = ? AND session_key IN (SELECT resource_id FROM local_computer_retirement_resources WHERE node_id = ? AND kind = 'session')", &[computer_id, user_id, computer_id]).await?;
        tx.execute(
            "UPDATE resource_revisions SET execution_node_id=?1,
               changes_json=CASE WHEN json_type(changes_json, '$.before.fields')='object'
                   THEN json_set(changes_json, '$.before.fields.execution_node_id', ?1) ELSE changes_json END
             WHERE kind='session' AND execution_node_id='embedded' AND owner_user_id=?2
               AND resource_id IN (SELECT resource_id FROM local_computer_retirement_resources WHERE node_id=?1 AND kind='session')",
            &[computer_id, user_id],
        ).await?;
        tx.execute(
            "UPDATE resource_revisions SET changes_json=json_set(changes_json, '$.after.fields.execution_node_id', ?1)
             WHERE kind='session' AND execution_node_id=?1 AND owner_user_id=?2 AND json_type(changes_json, '$.after.fields')='object'
               AND json_extract(changes_json, '$.after.fields.execution_node_id')='embedded'",
            &[computer_id, user_id],
        ).await?;
        tx.execute("UPDATE projects SET execution_node_id = ? WHERE execution_node_id = 'embedded' AND user_id = ? AND project_id IN (SELECT resource_id FROM local_computer_retirement_resources WHERE node_id = ? AND kind = 'project')", &[computer_id, user_id, computer_id]).await?;
        capture::finish(&mut *tx, &cuts).await?;
        tx.execute(
            "UPDATE resource_revisions SET execution_node_id=?1,
                changes_json=CASE WHEN json_type(changes_json,'$.before')='object'
                  THEN json_set(changes_json,'$.before.fields.execution_node_id',?1) ELSE changes_json END
             WHERE kind='project' AND owner_user_id=?2 AND resource_id IN
               (SELECT resource_id FROM local_computer_retirement_resources WHERE node_id=?1 AND kind='project')",
            &[computer_id,user_id],
        ).await?;
        tx.execute(
            "UPDATE resource_revisions SET changes_json=json_set(changes_json,'$.after.fields.execution_node_id',?1)
             WHERE kind='project' AND owner_user_id=?2 AND json_type(changes_json,'$.after')='object' AND resource_id IN
               (SELECT resource_id FROM local_computer_retirement_resources WHERE node_id=?1 AND kind='project')",
            &[computer_id,user_id],
        ).await?;
        tx.execute(
            "UPDATE sync_bindings SET computers_json=COALESCE(
                (SELECT json_group_array(json(value)) FROM json_each(computers_json)
                 WHERE json_extract(value,'$.computerId')<>?1), '[]')
             WHERE account_id=?2",
            &[computer_id, user_id],
        )
        .await?;
        tx.execute("UPDATE local_computer_retirements SET finalized = 1 WHERE node_id = ? AND owner_user_id = ?", &[computer_id, user_id]).await?;
        tx.commit().await?;
        self.sync.wake_capture();
        Ok(())
    }

    /// Explicit trusted-host ownership claim. Filesystem namespaces and Worker bindings never change.
    pub async fn claim_local_history(&self, user_id: &str) -> TalosResult<()> {
        let mut tx = self.storage.begin_transaction().await?;
        let active = tx.query("SELECT 1 FROM sessions s JOIN agent_active_turns a ON a.session_id = s.session_key WHERE s.user_id = 'anonymous' AND s.execution_node_id = 'embedded' LIMIT 1", &[]).await?;
        if !active.is_empty() {
            return Err(TalosError::new(
                talos_error::ErrorCode::ActiveTurnRunning,
                "Finish local Turns before claiming their history.",
            ));
        }
        // Only an explicit claim transfers captured facts for still-owned resources.
        // Deleted anonymous resources cannot be silently claimed by a later login.
        tx.execute(
            "UPDATE resource_revisions SET owner_user_id=?1 WHERE owner_user_id='anonymous' AND (
                (kind='session' AND resource_id IN (SELECT session_key FROM sessions WHERE user_id='anonymous' AND execution_node_id='embedded')) OR
                (kind='project' AND resource_id IN (SELECT project_id FROM projects WHERE user_id='anonymous' AND execution_node_id='embedded')) OR
                (kind='file' AND resource_id IN (SELECT file_id FROM files WHERE owner_user_id='anonymous' AND (project_id IS NULL OR project_id IN (SELECT project_id FROM projects WHERE user_id='anonymous' AND execution_node_id='embedded'))))
            )",
            &[user_id],
        ).await?;
        tx.execute("UPDATE files SET owner_user_id = ? WHERE owner_user_id = 'anonymous' AND (project_id IS NULL OR project_id IN (SELECT project_id FROM projects WHERE user_id = 'anonymous' AND execution_node_id = 'embedded'))", &[user_id]).await?;
        tx.execute("UPDATE projects SET user_id = ? WHERE user_id = 'anonymous' AND execution_node_id = 'embedded'", &[user_id]).await?;
        tx.execute("UPDATE sessions SET workspace_user_id = COALESCE(workspace_user_id, user_id), user_id = ? WHERE user_id = 'anonymous' AND execution_node_id = 'embedded'", &[user_id]).await?;
        tx.commit().await
    }

    /// Attach an authorized replication consumer; transaction capture is always active.
    #[cfg(feature = "sync")]
    pub fn with_sync(self, sync: Arc<crate::sync::StoreSyncAdapter>) -> Self {
        self.sync.install(sync).expect("initial Store sync binding");
        self
    }

    /// Install a host-verified binding after login; existing transactions keep
    /// their original association until they commit.
    #[cfg(feature = "sync")]
    pub fn install_sync(
        &self,
        adapter: std::sync::Arc<crate::sync::StoreSyncAdapter>,
    ) -> TalosResult<()> {
        self.sync.install(adapter)
    }

    pub(crate) async fn begin_session_transaction(
        &self,
        session_id: &str,
    ) -> TalosResult<Box<dyn talos_storage::StorageTransaction>> {
        crate::capture::source::transaction(
            self.storage.begin_transaction().await?,
            self.sync.clone(),
            vec![session_id.into()],
        )
        .await
    }
}
