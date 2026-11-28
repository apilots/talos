//! Stable host identity reuses the original durable identity row.
use talos_error::{TalosError, TalosResult};
use talos_storage::Storage;

pub(crate) async fn open(storage: &dyn Storage) -> TalosResult<String> {
    let mut tx = storage.begin_transaction().await?;
    let generated = uuid::Uuid::new_v4();
    let writer = u64::from_be_bytes(generated.as_bytes()[..8].try_into().expect("UUID prefix"));
    tx.execute(
        "INSERT OR IGNORE INTO sync_identity(singleton, peer_id, loro_writer_id) VALUES (1, ?, ?)",
        &[&generated.to_string(), &writer.to_string()],
    )
    .await?;
    let rows = tx
        .query("SELECT peer_id FROM sync_identity WHERE singleton = 1", &[])
        .await?;
    let identity = rows
        .first()
        .and_then(|row| row.get_string("peer_id"))
        .filter(|identity| !identity.is_empty())
        .ok_or_else(|| TalosError::internal("Host identity is unavailable."))?;
    tx.commit().await?;
    Ok(identity)
}

#[cfg(test)]
#[path = "host_identity_tests.rs"]
mod tests;
