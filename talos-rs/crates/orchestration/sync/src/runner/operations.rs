use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_sync_sdk::version_includes;
use talos_sync_sdk::SyncClient;

use super::{control::ControlCommand, transport, RunnerChannels};
use crate::{
    facade::sync_error, SyncBinding, SyncDataAdapter, SyncResourceKey, SyncVersion, TalosSync,
};

pub(super) async fn execute<A: SyncDataAdapter>(
    sync: &TalosSync,
    adapter: &A,
    binding: &SyncBinding,
    client: &SyncClient,
    channels: &mut RunnerChannels,
    command: ControlCommand,
) {
    match command {
        ControlCommand::CatchUp {
            resource,
            minimum,
            reply,
        } => {
            if !reply.is_closed() {
                let result = catch_up(
                    sync,
                    adapter,
                    binding,
                    client,
                    channels,
                    &resource,
                    minimum.as_ref(),
                )
                .await;
                let _ = reply.send(result);
            }
        }
        ControlCommand::Flush {
            resource,
            cut,
            minimum,
            reply,
        } => {
            if !reply.is_closed() {
                let result = flush(
                    sync,
                    adapter,
                    binding,
                    client,
                    channels,
                    &resource,
                    (cut, minimum),
                )
                .await;
                if result.is_ok() {
                    channels.failed_uploads.remove(&resource);
                }
                let _ = reply.send(result);
            }
        }
    }
}

async fn catch_up<A: SyncDataAdapter>(
    sync: &TalosSync,
    adapter: &A,
    binding: &SyncBinding,
    client: &SyncClient,
    channels: &RunnerChannels,
    resource: &SyncResourceKey,
    minimum: Option<&SyncVersion>,
) -> TalosResult<SyncVersion> {
    let (document, hub_cut) =
        transport::download(sync, adapter, binding, client, resource, &channels.changes).await?;
    // A local unuploaded version can be ahead of H. It never proves a different
    // manager can recover a requested shared cut from the hub.
    let result = match minimum {
        Some(minimum) if !version_includes(&hub_cut, minimum).map_err(sync_error)? => {
            Err(TalosError::new(
                ErrorCode::Conflict,
                "The requested shared history is not available yet.",
            ))
        }
        _ => document.committed_version().map_err(sync_error),
    };
    document.close().await.map_err(sync_error)?;
    result
}

async fn flush<A: SyncDataAdapter>(
    sync: &TalosSync,
    adapter: &A,
    binding: &SyncBinding,
    client: &SyncClient,
    channels: &RunnerChannels,
    resource: &SyncResourceKey,
    cut: (Option<i64>, SyncVersion),
) -> TalosResult<SyncVersion> {
    let (cut, minimum) = cut;
    if let Some(cut) = cut {
        let mut acknowledged = None;
        while sync.has_pending_through(binding, resource, cut).await? {
            acknowledged = transport::upload_resource(
                sync,
                adapter,
                binding,
                client,
                channels,
                resource.clone(),
            )
            .await?;
            if acknowledged.is_none() {
                // A confirmed H tombstone can supersede this pending cut while
                // downloading. Recover its real version and verify below.
                break;
            }
        }
        if let Some(version) = acknowledged {
            if version_includes(&version, &minimum).map_err(sync_error)? {
                return Ok(version);
            }
        }
    }
    // The runner may already have acknowledged the fixed cut before servicing
    // this request. Recover H's current complete cut rather than inventing an ACK.
    let (document, hub_cut) =
        transport::download(sync, adapter, binding, client, resource, &channels.changes).await?;
    document.close().await.map_err(sync_error)?;
    if !version_includes(&hub_cut, &minimum).map_err(sync_error)? {
        return Err(TalosError::new(
            ErrorCode::Conflict,
            "The captured changes were superseded before publication.",
        ));
    }
    Ok(hub_cut)
}
