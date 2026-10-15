//! Advertise only this management host's verified Computer aliases, without durable state.
use crate::{facade::sync_error, SyncBinding, TalosSync};
use std::{
    collections::BTreeSet,
    time::{SystemTime, UNIX_EPOCH},
};
use talos_error::{TalosError, TalosResult};
use talos_sync_sdk::{SyncClient, SyncPresence};

pub(super) async fn publish(
    sync: &TalosSync,
    binding: &SyncBinding,
    client: &SyncClient,
) -> TalosResult<()> {
    let observed_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| {
            TalosError::internal_with_source("Read synchronization presence time.", error)
        })?
        .as_millis() as i64;
    let computers: BTreeSet<_> = binding
        .computers
        .iter()
        .map(|alias| &alias.computer_id)
        .collect();
    for computer_id in computers {
        // Do not reserve an extra room for telemetry while document work runs.
        let channel = client
            .open_presence(computer_id.clone())
            .await
            .map_err(sync_error)?;
        let sent = channel
            .set(SyncPresence {
                peer_id: sync.peer_id().into(),
                computer_id: computer_id.clone(),
                observed_at,
                valid_until: observed_at + 30_000,
            })
            .await
            .map_err(sync_error);
        let closed = channel.close().await.map_err(sync_error);
        match (sent, closed) {
            (Err(error), Err(cleanup)) => {
                talos_logging::report_error(&cleanup);
                return Err(error);
            }
            (Err(error), _) | (_, Err(error)) => return Err(error),
            (Ok(()), Ok(())) => {}
        }
    }
    Ok(())
}
