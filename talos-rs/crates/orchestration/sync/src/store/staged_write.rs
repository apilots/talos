use std::sync::Arc;

use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::{Storage, StorageTransaction};

#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Capture,
    Snapshot,
}

impl Kind {
    fn table(self) -> &'static str {
        match self {
            Self::Capture => "sync_capture_batches",
            Self::Snapshot => "sync_snapshots",
        }
    }
}

pub(crate) struct StagedWrite {
    pub(crate) id: String,
    pub(crate) storage: Arc<dyn Storage>,
    pub(crate) kind: Kind,
}

impl StagedWrite {
    pub(crate) async fn require_staged(&self, tx: &mut dyn StorageTransaction) -> TalosResult<()> {
        let table = self.kind.table();
        let rows = tx
            .query(
                &format!("SELECT 1 FROM {table} WHERE id=? AND state='staged'"),
                &[&self.id],
            )
            .await?;
        if rows.is_empty() {
            Err(unavailable())
        } else {
            Ok(())
        }
    }

    pub(crate) async fn publish(&self, tx: &mut dyn StorageTransaction) -> TalosResult<()> {
        let table = self.kind.table();
        let changed = tx
            .execute(
                &format!("UPDATE {table} SET state='published' WHERE id=? AND state='staged'"),
                &[&self.id],
            )
            .await?;
        if changed == 1 {
            Ok(())
        } else {
            Err(unavailable())
        }
    }
}

fn unavailable() -> TalosError {
    TalosError::new(
        ErrorCode::Conflict,
        "The staged synchronization write became unavailable.",
    )
}

impl Drop for StagedWrite {
    fn drop(&mut self) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let id = self.id.clone();
        let storage = self.storage.clone();
        let table = self.kind.table();
        runtime.spawn(async move {
            if let Err(error) = retire(&*storage, table, &id).await {
                talos_logging::report_error(&error);
            }
        });
    }
}

async fn retire(storage: &dyn Storage, table: &str, id: &str) -> TalosResult<()> {
    let mut tx = storage.begin_background_transaction().await?;
    tx.execute(
        &format!("UPDATE {table} SET state='retired' WHERE id=? AND state='staged'"),
        &[id],
    )
    .await?;
    tx.commit().await
}
