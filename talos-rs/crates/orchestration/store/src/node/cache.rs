//! Public Computer replicas owned by the authenticated control-plane directory.
use super::DatabaseAgentNodeStore;
use talos_error::{TalosError, TalosResult};
use talos_storage::database_error;
use talos_types::{NodeInfo, NodeMode, NodeStatus};
impl DatabaseAgentNodeStore {
    /// A Server-confirmed mutation supersedes legacy Sync-derived cache markers.
    pub async fn confirm_remote_computer(&self, node: &NodeInfo) -> TalosResult<()> {
        let mut tx = self.storage.write_transaction().await?;
        upsert(&mut tx, node).await?;
        tx.commit().await
    }
    /// Publish one complete owner directory before SQL filtering, pagination and counts.
    pub async fn replace_remote_computers(
        &self,
        owner: &str,
        nodes: &[NodeInfo],
    ) -> TalosResult<()> {
        let mut tx = self.storage.write_transaction().await?;
        for node in nodes {
            if node.owner_user_id.as_deref() != Some(owner) {
                return Err(TalosError::authorization_denied());
            }
            upsert(&mut tx, node).await?;
        }
        let ids =
            serde_json::to_string(&nodes.iter().map(|node| &node.node_id).collect::<Vec<_>>())
                .map_err(|error| {
                    TalosError::internal_with_source(
                        "Encode the account Computer directory.",
                        error,
                    )
                })?;
        sqlx::query(
            "DELETE FROM agent_nodes WHERE owner_user_id=? AND mode='remote'
            AND credential_hash IS NULL AND lease_token_hash IS NULL
            AND NOT EXISTS (SELECT 1 FROM managed_workers w WHERE w.node_id=agent_nodes.node_id)
            AND node_id NOT IN (SELECT value FROM json_each(?))",
        )
        .bind(owner)
        .bind(ids)
        .execute(&mut *tx)
        .await
        .map_err(|error| database_error("replace the account Computer cache", error))?;
        tx.commit().await
    }
    pub async fn remove_cached_computer(&self, owner: &str, id: &str) -> TalosResult<()> {
        let mut tx = self.storage.write_transaction().await?;
        sqlx::query(
            "DELETE FROM agent_nodes WHERE node_id=? AND owner_user_id=? AND mode='remote'
            AND credential_hash IS NULL AND lease_token_hash IS NULL
            AND NOT EXISTS (SELECT 1 FROM managed_workers w WHERE w.node_id=agent_nodes.node_id)",
        )
        .bind(id)
        .bind(owner)
        .execute(&mut *tx)
        .await
        .map_err(|error| database_error("remove a confirmed Computer cache entry", error))?;
        tx.commit().await
    }
}
async fn upsert(tx: &mut sqlx::SqliteConnection, node: &NodeInfo) -> TalosResult<()> {
    if node.mode != NodeMode::Remote || node.owner_user_id.is_none() {
        return Err(TalosError::authorization_denied());
    }
    let existing: Option<(Option<String>, String)> =
        sqlx::query_as("SELECT owner_user_id,mode FROM agent_nodes WHERE node_id=?")
            .bind(&node.node_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|error| database_error("read Computer cache ownership", error))?;
    if existing.is_some_and(|(owner, mode)| owner != node.owner_user_id || mode != "remote") {
        return Err(TalosError::authorization_denied());
    }
    let deletion: Option<String> =
        sqlx::query_scalar("SELECT owner_user_id FROM computer_deletions WHERE node_id=?")
            .bind(&node.node_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|error| database_error("read Computer cache deletion", error))?;
    if let Some(owner) = deletion {
        if Some(&owner) != node.owner_user_id.as_ref() {
            return Err(TalosError::authorization_denied());
        }
        // A fresh authenticated Server result overrides legacy Sync-only markers.
        sqlx::query("DELETE FROM computer_deletions WHERE node_id=? AND owner_user_id=?")
            .bind(&node.node_id)
            .bind(&owner)
            .execute(&mut *tx)
            .await
            .map_err(|error| database_error("clear a superseded Computer cache marker", error))?;
    }
    let report = node
        .system_info
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| TalosError::internal_with_source("Encode Worker system report.", error))?;
    sqlx::query("INSERT INTO agent_nodes(node_id,owner_user_id,display_name,mode,endpoint,status,created_at,updated_at,system_info)
        VALUES (?,?,?,'remote',?,?,?,?,?) ON CONFLICT(node_id) DO UPDATE SET display_name=excluded.display_name,
        endpoint=excluded.endpoint,status=excluded.status,created_at=excluded.created_at,updated_at=excluded.updated_at,system_info=excluded.system_info
        WHERE agent_nodes.credential_hash IS NULL AND agent_nodes.lease_token_hash IS NULL
        AND NOT EXISTS (SELECT 1 FROM managed_workers w WHERE w.node_id=excluded.node_id)")
        .bind(&node.node_id).bind(&node.owner_user_id).bind(&node.display_name).bind(&node.endpoint)
        .bind(match node.status {NodeStatus::Enabled=>"enabled",NodeStatus::Disabled=>"disabled"})
        .bind(node.created_at).bind(node.updated_at).bind(report).execute(&mut *tx).await.map_err(|error|database_error("write the Server Computer cache",error))?;
    Ok(())
}
