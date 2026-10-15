//! Advertise only this management host's verified Computer aliases, without durable state.
use crate::{facade::sync_error, TalosSync};
use std::{
    collections::BTreeSet,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use talos_error::{TalosError, TalosResult};
use talos_sync_sdk::{SyncClient, SyncPresence};

/// Polled alongside the document session, cancelled with its owning connection.
/// At most one publication is in flight; SDK deadlines bound its room lifetime.
pub(super) async fn run(
    sync: &TalosSync,
    computers: tokio::sync::watch::Receiver<Vec<String>>,
    client: &SyncClient,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(10));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        let current = computers.borrow().clone();
        if let Err(error) = publish(sync, &current, client).await {
            talos_logging::report_error(&error);
        }
    }
}

async fn publish(sync: &TalosSync, computers: &[String], client: &SyncClient) -> TalosResult<()> {
    let observed_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| {
            TalosError::internal_with_source("Read synchronization presence time.", error)
        })?
        .as_millis() as i64;
    let computers: BTreeSet<_> = computers.iter().collect();
    for computer_id in computers {
        // Release the ephemeral room after each publication; never retain it between ticks.
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
