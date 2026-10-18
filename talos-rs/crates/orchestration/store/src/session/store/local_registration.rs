//! Snapshot the pre-removal local resources in the transaction observing deletion.
use talos_error::TalosResult;
use talos_storage::StorageTransaction;

pub(crate) async fn capture_removal_boundary(
    tx: &mut dyn StorageTransaction,
    owner: &str,
    computer: &str,
) -> TalosResult<()> {
    if !tx
        .query(
            "SELECT 1 FROM local_computer_retirements WHERE node_id = ? AND owner_user_id = ?",
            &[computer, owner],
        )
        .await?
        .is_empty()
    {
        return Ok(());
    }
    tx.execute(
        "INSERT INTO local_computer_retirements(node_id, owner_user_id) VALUES (?, ?)",
        &[computer, owner],
    )
    .await?;
    tx.execute("INSERT INTO local_computer_retirement_resources(node_id,kind,resource_id) SELECT ?, 'session', session_key FROM sessions WHERE execution_node_id = 'embedded' AND user_id = ?", &[computer, owner]).await?;
    tx.execute("INSERT INTO local_computer_retirement_resources(node_id,kind,resource_id) SELECT ?, 'project', project_id FROM projects WHERE execution_node_id = 'embedded' AND user_id = ?", &[computer, owner]).await?;
    Ok(())
}

impl super::DatabaseSessionStore {
    pub async fn local_registration_retired(
        &self,
        owner: &str,
        computer: &str,
    ) -> TalosResult<bool> {
        Ok(!self.storage.query("SELECT 1 FROM local_computer_retirements WHERE node_id = ? AND owner_user_id = ? AND finalized = 1", &[computer, owner]).await?.is_empty())
    }
    /// An authenticated deletion response and a Sync import establish the same
    /// durable boundary. Retrying cannot include subsequently created local work.
    pub async fn record_local_registration_removal(
        &self,
        owner: &str,
        computer: &str,
    ) -> TalosResult<()> {
        let mut tx = self.storage.begin_transaction().await?;
        capture_removal_boundary(&mut *tx, owner, computer).await?;
        tx.commit().await
    }
}
