//! Authorized synchronization capture and explicit local history claiming.
use super::DatabaseSessionStore;
use std::sync::Arc;
use talos_error::{TalosError, TalosResult};

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
        if !tx
            .query(
                "SELECT 1 FROM local_computer_retirements WHERE node_id = ? AND owner_user_id = ?",
                &[computer_id, user_id],
            )
            .await?
            .is_empty()
        {
            tx.commit().await?;
            return Ok(());
        }
        if !tx.query("SELECT 1 FROM sessions s JOIN agent_active_turns a ON a.session_id = s.session_key WHERE s.execution_node_id = 'embedded' AND s.user_id = ? LIMIT 1", &[user_id]).await?.is_empty() {
            return Err(TalosError::localized(talos_error::ErrorLocalization::ComputerDeletionActive));
        }
        let mut resources = Vec::new();
        for row in tx.query("SELECT session_key AS id FROM sessions WHERE execution_node_id = 'embedded' AND user_id = ?", &[user_id]).await? {
            resources.push(talos_sync::SyncResourceKey::Session { session_id: row.get_string("id").expect("Session key") });
        }
        for row in tx.query("SELECT project_id AS id FROM projects WHERE execution_node_id = 'embedded' AND user_id = ?", &[user_id]).await? {
            resources.push(talos_sync::SyncResourceKey::Project { project_id: row.get_string("id").expect("Project key") });
        }
        let adapters = self.sync.all();
        let mut cuts = Vec::new();
        for adapter in &adapters {
            if adapter.binding().scope.account_id == user_id {
                cuts.push((
                    adapter.clone(),
                    crate::sync::capture::prepare(adapter, &mut *tx, resources.clone()).await?,
                ));
            }
        }
        tx.execute("UPDATE sessions SET execution_node_id = ? WHERE execution_node_id = 'embedded' AND user_id = ?", &[computer_id, user_id]).await?;
        tx.execute("UPDATE projects SET execution_node_id = ? WHERE execution_node_id = 'embedded' AND user_id = ?", &[computer_id, user_id]).await?;
        for (adapter, cut) in &cuts {
            crate::sync::capture::finish(adapter, &mut *tx, cut).await?;
        }
        for adapter in &adapters {
            if adapter.binding().scope.account_id == user_id
                && adapter.binding().computers.iter().any(|alias| {
                    alias.source_node_id == "embedded" && alias.computer_id == computer_id
                })
            {
                adapter
                    .sync
                    .retire_computer_alias(&mut *tx, adapter.binding(), computer_id)
                    .await?;
            }
        }
        tx.execute(
            "INSERT INTO local_computer_retirements(node_id,owner_user_id) VALUES (?, ?)",
            &[computer_id, user_id],
        )
        .await?;
        tx.commit().await?;
        for adapter in adapters {
            adapter.committed();
        }
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
