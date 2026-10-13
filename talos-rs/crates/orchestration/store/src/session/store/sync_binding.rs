//! Authorized synchronization capture and explicit local history claiming.
use super::DatabaseSessionStore;
use std::sync::Arc;
use talos_error::{TalosError, TalosResult};

impl DatabaseSessionStore {
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
        tx.execute("UPDATE files SET owner_user_id = ? WHERE owner_user_id = 'anonymous' AND (project_id IS NULL OR project_id IN (SELECT project_id FROM projects WHERE user_id = 'anonymous' AND execution_node_id = 'embedded'))", &[user_id]).await?;
        tx.execute("UPDATE projects SET user_id = ? WHERE user_id = 'anonymous' AND execution_node_id = 'embedded'", &[user_id]).await?;
        tx.execute("UPDATE sessions SET workspace_user_id = COALESCE(workspace_user_id, user_id), user_id = ? WHERE user_id = 'anonymous' AND execution_node_id = 'embedded'", &[user_id]).await?;
        tx.commit().await
    }

    /// Install the authorized capture boundary; imports bypass these workflows.
    pub fn with_sync(self, sync: Arc<crate::sync::StoreSyncAdapter>) -> Self {
        self.sync.install(sync).expect("initial Store sync binding");
        self
    }

    /// Install a host-verified binding after login; existing transactions keep
    /// their original association until they commit.
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
        crate::sync::capture::transaction(
            self.storage.begin_transaction().await?,
            self.sync.clone(),
            vec![talos_sync::SyncResourceKey::Session {
                session_id: session_id.into(),
            }],
        )
        .await
    }
}
