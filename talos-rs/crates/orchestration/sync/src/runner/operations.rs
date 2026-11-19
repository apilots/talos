use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_sync_sdk::version_includes;
use talos_sync_sdk::SyncClient;

use super::{
    control::{ControlCommand, FlushTarget},
    transport, RunnerChannels,
};
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
            target,
            reply,
        } => {
            if !reply.is_closed() {
                let result = flush(sync, binding, client, channels, &resource, target).await;
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

async fn flush(
    sync: &TalosSync,
    binding: &SyncBinding,
    client: &SyncClient,
    channels: &RunnerChannels,
    resource: &SyncResourceKey,
    target: FlushTarget,
) -> TalosResult<SyncVersion> {
    let cut = match &target {
        FlushTarget::Captured(sequence) => Some(*sequence),
        FlushTarget::Outbox { cut, .. } => *cut,
    };
    let mut acknowledged = None;
    if let Some(cut) = cut {
        while sync.has_pending_through(binding, resource, cut).await? {
            acknowledged =
                transport::upload_resource(sync, binding, client, channels, resource.clone(), cut)
                    .await?;
            if acknowledged.is_none() {
                // A concurrent ACK or imported tombstone can remove pending rows.
                // Verify Hub coverage even when the queue is already empty.
                break;
            }
        }
    }
    let minimum = match target {
        FlushTarget::Captured(sequence) => {
            sync.captured_version(binding, resource, sequence).await?
        }
        FlushTarget::Outbox { minimum, .. } => minimum,
    };
    if let Some(version) = acknowledged {
        if version_includes(&version, &minimum).map_err(sync_error)? {
            return Ok(version);
        }
    }
    // Joined reports the durable Hub vector independently of whether a later
    // inbound update is ready for local publication.
    let committed = sync
        .document_version(binding, resource)
        .await?
        .unwrap_or_default();
    let (document, hub_cut) = transport::join_document(client, resource, &committed).await?;
    document.close().await.map_err(sync_error)?;
    if !version_includes(&hub_cut, &minimum).map_err(sync_error)? {
        return Err(TalosError::new(
            ErrorCode::Conflict,
            "The captured changes were superseded before publication.",
        ));
    }
    Ok(hub_cut)
}
