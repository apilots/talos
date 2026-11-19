use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_sync_sdk::version_includes;
use talos_sync_sdk::{DocumentEvent, DocumentUpdate, SyncClient, SyncDocument};
use talos_sync_types::{ResourceKind, SyncListParams, SyncResourceKey};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::{
    facade::sync_error, store, SyncBinding, SyncCaptureParams, SyncConnectionState,
    SyncDataAdapter, SyncExportParams, SyncImportBatch, SyncScanParams, SyncStartParams, TalosSync,
};

use super::{operations, FailedUpload, RunnerChannels};

const UPLOAD_RETRY_DELAY: Duration = Duration::from_secs(15 * 60);

#[derive(Debug)]
pub(super) enum ConnectionPhase {
    Bootstrap,
    Connect,
    CatchUp,
    Subscribe,
    ReceiveFeed,
    Download,
    Upload,
    LocalStatus,
    Shutdown,
}

pub(super) async fn connected<A: SyncDataAdapter>(
    sync: &Arc<TalosSync>,
    adapter: &A,
    params: &SyncStartParams,
    stop: &CancellationToken,
    channels: &mut RunnerChannels,
    phase: &mut ConnectionPhase,
) -> TalosResult<()> {
    tokio::select! { _ = stop.cancelled() => return Ok(()), result = bootstrap(sync, adapter, &params.binding) => result? }
    let mut options = talos_sync_sdk::SyncOptions::new(
        sync.hub_endpoint().into(),
        params.binding.scope.account_id.clone(),
        sync.peer_id().into(),
    );
    options.trusted_ca_pem = params.trusted_ca_pem.clone();
    // The durable runner owns reconnect scheduling, including credential retries.
    options.connect_attempts = 1;
    *phase = ConnectionPhase::Connect;
    let client = tokio::select! {
        _ = stop.cancelled() => return Ok(()),
        client = SyncClient::connect(options, params.credentials.clone()) => client.map_err(sync_error)?,
    };
    let result = if client.scope().await != params.binding.scope {
        Err(TalosError::new(
            ErrorCode::Conflict,
            "Synchronization hub identity does not match the installed binding.",
        ))
    } else {
        // One connection owns both futures. Dropping the select cancels presence
        // guards before shutdown; optional I/O never blocks document/session work.
        let presence = super::presence::run(sync, channels.presence_computers.clone(), &client);
        tokio::select! {
            _ = presence => Ok(()),
            _ = stop.cancelled() => Ok(()),
            result = session(sync, adapter, &params.binding, &client, channels, phase) => result,
        }
    };
    if result.is_ok() {
        *phase = ConnectionPhase::Shutdown;
    }
    let shutdown = client.shutdown().await.map_err(sync_error);
    result.and(shutdown)
}

async fn bootstrap<A: SyncDataAdapter>(
    sync: &TalosSync,
    adapter: &A,
    binding: &SyncBinding,
) -> TalosResult<()> {
    let mut cursor = None;
    loop {
        let page = adapter
            .scan(SyncScanParams {
                binding: binding.clone(),
                cursor: cursor.clone(),
                limit: 128,
            })
            .await?;
        for source_resource in page.resources {
            let resource = crate::projection::resource(binding, &source_resource);
            let mut read = sync.storage.begin_read_transaction().await?;
            if store::has_capture_baseline(&mut *read, &binding.scope, &resource).await? {
                read.commit().await?;
                continue;
            }
            let facts = adapter
                .export(
                    &mut *read,
                    SyncExportParams {
                        binding: binding.clone(),
                        resource: source_resource,
                    },
                )
                .await?;
            read.commit().await?;
            if facts.changes.is_empty() && facts.event_mappings.is_empty() {
                continue;
            }
            let capture = TalosSync::prepare_capture(SyncCaptureParams {
                binding: binding.clone(),
                resource: resource.clone(),
                changes: facts.changes,
                created_at: now(),
            })?;
            let (capture, mappings) = sync.stage_capture(capture, facts.event_mappings).await?;
            // A local commit can create the baseline between the two transactions.
            let mut tx = sync.storage.begin_background_transaction().await?;
            if !store::has_capture_baseline(&mut *tx, &binding.scope, &resource).await? {
                sync.append_capture(&mut *tx, capture).await?;
                for mapping in mappings {
                    sync.map_event(&mut *tx, mapping).await?;
                }
            }
            tx.commit().await?;
        }
        if page.next_cursor.is_none() {
            break;
        }
        if page.next_cursor == cursor {
            return Err(TalosError::new(
                ErrorCode::InvalidCursor,
                "Synchronization scan did not advance.",
            ));
        }
        cursor = page.next_cursor;
    }
    Ok(())
}

async fn session<A: SyncDataAdapter>(
    sync: &TalosSync,
    adapter: &A,
    binding: &SyncBinding,
    client: &SyncClient,
    channels: &mut RunnerChannels,
    phase: &mut ConnectionPhase,
) -> TalosResult<()> {
    *phase = ConnectionPhase::CatchUp;
    let cursor = match sync.download_cursor(binding).await? {
        Some(cursor) => cursor,
        None => catch_up(sync, adapter, binding, client, &channels.changes).await?,
    };
    *phase = ConnectionPhase::Subscribe;
    let mut subscription = match client.events(Some(cursor)).await {
        Ok(subscription) => subscription,
        Err(error) if error.code == talos_sync_types::SyncErrorCode::EventCursorExpired => {
            *phase = ConnectionPhase::CatchUp;
            let cursor = catch_up(sync, adapter, binding, client, &channels.changes).await?;
            *phase = ConnectionPhase::Subscribe;
            client.events(Some(cursor)).await.map_err(sync_error)?
        }
        Err(error) => return Err(sync_error(error)),
    };
    channels.status.send_modify(|value| {
        value.connection = SyncConnectionState::Connected;
        value.last_error = channels
            .failed_uploads
            .values()
            .next()
            .map(|failure| failure.error.clone());
    });
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    let mut feed_gap = false;
    loop {
        *phase = ConnectionPhase::LocalStatus;
        if !sync.enabled(&binding.scope.account_id).await? {
            return Ok(());
        }
        *phase = ConnectionPhase::ReceiveFeed;
        tokio::select! {
            batch = subscription.receive() => {
                let Some(batch) = batch.map_err(sync_error)? else { return Err(TalosError::new(ErrorCode::ServiceUnavailable, "Synchronization feed disconnected.")) };
                *phase = ConnectionPhase::Download;
                for entry in batch.entries {
                    match download_if_newer(sync, adapter, binding, client, (&entry.resource, &entry.version), &channels.changes).await {
                        Ok(()) => {}
                        Err(error) if error.error_code() == ErrorCode::Conflict => {
                            // Preserve the last contiguous cursor and keep
                            // publishing independent resources on this connection.
                            feed_gap = true;
                            continue;
                        }
                        Err(error) => return Err(error),
                    }
                    if !sync.has_pending_through(binding, &entry.resource, i64::MAX).await? {
                        channels.failed_uploads.remove(&entry.resource);
                    }
                }
                if !feed_gap {
                    commit_cursor(sync, binding, &batch.next_cursor).await?;
                }
            }
            command = channels.commands.recv() => {
                let Some(command) = command else { return Ok(()) };
                operations::execute(sync, adapter, binding, client, channels, command).await;
            }
            _ = interval.tick() => {
                *phase = ConnectionPhase::Upload;
                upload_pending(sync, binding, client, channels).await?;
                if feed_gap {
                    *phase = ConnectionPhase::Subscribe;
                    subscription.close().await.map_err(sync_error)?;
                    drop(subscription);
                    subscription = client.events(sync.download_cursor(binding).await?).await.map_err(sync_error)?;
                    feed_gap = false;
                }
            },
            wakeup = channels.wake.recv() => {
                if wakeup.is_none() { return Ok(()) }
                *phase = ConnectionPhase::Upload;
                upload_pending(sync, binding, client, channels).await?;
            }
        }
        *phase = ConnectionPhase::LocalStatus;
        let pending = sync.pending_changes(binding).await?;
        channels.status.send_modify(|value| {
            value.pending_changes = pending;
            value.last_error = channels
                .failed_uploads
                .values()
                .next()
                .map(|failure| failure.error.clone());
        });
    }
}

async fn catch_up<A: SyncDataAdapter>(
    sync: &TalosSync,
    adapter: &A,
    binding: &SyncBinding,
    client: &SyncClient,
    changes: &broadcast::Sender<talos_sync_types::SyncResourceSummary>,
) -> TalosResult<String> {
    let initial = client
        .catalog(SyncListParams {
            limit: Some(1),
            ..Default::default()
        })
        .await
        .map_err(sync_error)?;
    let mut blocked = None;
    // Dependencies precede Session facts. The initial cut remains the feed start,
    // even if a later kind scan observes a newer committed resource version.
    for kind in [
        ResourceKind::Computer,
        ResourceKind::Project,
        ResourceKind::File,
        ResourceKind::Session,
    ] {
        let mut cursor = None;
        loop {
            let page = client
                .catalog(SyncListParams {
                    kind: Some(kind),
                    cursor: cursor.clone(),
                    limit: Some(128),
                    computer_id: None,
                })
                .await
                .map_err(sync_error)?;
            for resource in page.resources {
                match download_if_newer(
                    sync,
                    adapter,
                    binding,
                    client,
                    (&resource.resource, &resource.version),
                    changes,
                )
                .await
                {
                    Ok(()) => {}
                    Err(error) if error.error_code() == ErrorCode::Conflict => {
                        blocked.get_or_insert(error);
                    }
                    Err(error) => return Err(error),
                }
            }
            if page.next_cursor.is_none() {
                break;
            }
            if page.next_cursor == cursor {
                return Err(TalosError::new(
                    ErrorCode::InvalidCursor,
                    "Synchronization catalog did not advance.",
                ));
            }
            cursor = page.next_cursor;
        }
    }
    if let Some(error) = blocked {
        return Err(error);
    }
    commit_cursor(sync, binding, &initial.feed_cursor).await?;
    Ok(initial.feed_cursor)
}

async fn download_if_newer<A: SyncDataAdapter>(
    sync: &TalosSync,
    adapter: &A,
    binding: &SyncBinding,
    client: &SyncClient,
    target: (&SyncResourceKey, &crate::SyncVersion),
    changes: &broadcast::Sender<talos_sync_types::SyncResourceSummary>,
) -> TalosResult<()> {
    let (resource, expected) = target;
    if let Some(version) = sync.document_version(binding, resource).await? {
        if version_includes(&version, expected).map_err(sync_error)? {
            return Ok(());
        }
    }
    let (document, hub_cut) = download(sync, adapter, binding, client, resource, changes).await?;
    document.close().await.map_err(sync_error)?;
    if !version_includes(&hub_cut, expected).map_err(sync_error)? {
        return Err(TalosError::new(
            ErrorCode::Conflict,
            "The synchronization feed cut is not yet available.",
        ));
    }
    Ok(())
}

pub(super) async fn download<A: SyncDataAdapter>(
    sync: &TalosSync,
    adapter: &A,
    binding: &SyncBinding,
    client: &SyncClient,
    resource: &SyncResourceKey,
    changes: &broadcast::Sender<talos_sync_types::SyncResourceSummary>,
) -> TalosResult<(SyncDocument, crate::SyncVersion)> {
    download_with_depth(sync, adapter, binding, client, resource, changes, 0).await
}

pub(super) async fn join_document(
    client: &SyncClient,
    resource: &SyncResourceKey,
    committed: &crate::SyncVersion,
) -> TalosResult<(SyncDocument, crate::SyncVersion)> {
    let document = client
        .open_document(resource.clone(), committed.clone())
        .await
        .map_err(sync_error)?;
    let joined = tokio::time::timeout(Duration::from_secs(15), document.receive())
        .await
        .map_err(|_| {
            TalosError::new(
                ErrorCode::Timeout,
                "Synchronization document join timed out.",
            )
        })?
        .map_err(sync_error)?;
    let Some(DocumentEvent::Joined {
        version: hub_version,
        ..
    }) = joined
    else {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Invalid synchronization document join.",
        ));
    };
    Ok((document, hub_version))
}

async fn download_with_depth<A: SyncDataAdapter>(
    sync: &TalosSync,
    adapter: &A,
    binding: &SyncBinding,
    client: &SyncClient,
    resource: &SyncResourceKey,
    changes: &broadcast::Sender<talos_sync_types::SyncResourceSummary>,
    depth: usize,
) -> TalosResult<(SyncDocument, crate::SyncVersion)> {
    if depth > 32 {
        return Err(TalosError::new(
            ErrorCode::Conflict,
            "Synchronization resource dependency chain is cyclic or too deep.",
        ));
    }
    let mut committed = sync
        .document_version(binding, resource)
        .await?
        .unwrap_or_default();
    let (document, hub_version) = join_document(client, resource, &committed).await?;
    tokio::time::timeout(Duration::from_secs(15), async {
        while !version_includes(&committed, &hub_version).map_err(sync_error)? {
            let event = document.receive().await.map_err(sync_error)?;
            let Some(DocumentEvent::Update { updates, .. }) = event else {
                return Err(TalosError::new(
                    ErrorCode::ServiceUnavailable,
                    "Synchronization document disconnected.",
                ));
            };
            let (mut tx, mut version, facts) =
                prepare_download(sync, adapter, binding, resource, &updates, &hub_version).await?;
            let batch = SyncImportBatch {
                scope: binding.scope.clone(),
                resource: resource.clone(),
                version: version.clone(),
                changes: facts,
            };
            let dependencies = adapter.missing_dependencies(&mut *tx, &batch).await?;
            if !dependencies.is_empty() {
                tx.rollback().await?;
                for dependency in dependencies {
                    if &dependency == resource {
                        return Err(TalosError::new(
                            ErrorCode::Conflict,
                            "A synchronized resource cannot depend on itself.",
                        ));
                    }
                    let (dependency_document, _) = Box::pin(download_with_depth(
                        sync,
                        adapter,
                        binding,
                        client,
                        &dependency,
                        changes,
                        depth + 1,
                    ))
                    .await?;
                    dependency_document.close().await.map_err(sync_error)?;
                }
                // The received bytes remain owned by this call. Rebuild the cut
                // after dependencies commit; do not await the runner's own queue.
                let (recovered_tx, recovered_version, facts) =
                    prepare_download(sync, adapter, binding, resource, &updates, &hub_version)
                        .await?;
                tx = recovered_tx;
                version = recovered_version;
                adapter
                    .apply_prepared(
                        &mut *tx,
                        &SyncImportBatch {
                            scope: binding.scope.clone(),
                            resource: resource.clone(),
                            version: version.clone(),
                            changes: facts,
                        },
                    )
                    .await?;
            } else {
                adapter.apply_prepared(&mut *tx, &batch).await?;
            }
            tx.commit().await?;
            document
                .mark_committed(version.clone())
                .map_err(sync_error)?;
            committed = version;
            if let Some(summary) = sync.resource_summary(binding, resource).await? {
                let _ = changes.send(summary);
            }
        }
        Ok::<_, TalosError>(())
    })
    .await
    .map_err(|_| {
        TalosError::new(
            ErrorCode::Timeout,
            "Synchronization document recovery timed out.",
        )
    })??;
    Ok((document, hub_version))
}

async fn prepare_download<A: SyncDataAdapter>(
    sync: &TalosSync,
    adapter: &A,
    binding: &SyncBinding,
    resource: &SyncResourceKey,
    updates: &[Vec<u8>],
    version: &crate::SyncVersion,
) -> TalosResult<(
    Box<dyn talos_storage::StorageTransaction>,
    crate::SyncVersion,
    Vec<talos_sync_types::SyncChange>,
)> {
    sync.import_document_with(
        binding,
        resource,
        updates,
        version,
        None,
        |version, changes| async move {
            let batch = SyncImportBatch {
                scope: binding.scope.clone(),
                resource: resource.clone(),
                version,
                changes,
            };
            adapter.prepare_import(&batch).await?;
            Ok(batch.changes)
        },
    )
    .await
}

async fn upload_pending(
    sync: &TalosSync,
    binding: &SyncBinding,
    client: &SyncClient,
    channels: &mut RunnerChannels,
) -> TalosResult<()> {
    if channels.upload_cut == 0 {
        let rows = sync
            .storage
            .query(
                "SELECT MAX(seq) AS cut FROM visible_sync_outbox WHERE hub_id = ? AND account_id = ?",
                &[&binding.scope.hub_id, &binding.scope.account_id],
            )
            .await?;
        channels.upload_cut = rows.first().and_then(|row| row.get_i64("cut")).unwrap_or(0);
    }
    let resources = sync
        .pending_resources_after(binding, channels.upload_cursor, channels.upload_cut, 8)
        .await?;
    if resources.is_empty() {
        channels.upload_cursor = 0;
        channels.upload_cut = 0;
        return Ok(());
    }
    for (sequence, resource) in resources {
        channels.upload_cursor = sequence;
        // Read the durable cut before applying facts: malformed resource facts
        // must be isolated without re-running the same failing materialization.
        let rows = sync.storage.query(
            "SELECT MAX(seq) AS cut FROM visible_sync_outbox WHERE hub_id = ? AND account_id = ? AND kind = ? AND resource_id = ?",
            &[&binding.scope.hub_id, &binding.scope.account_id, resource.kind().as_str(), resource.id()],
        ).await?;
        let cut = rows.first().and_then(|row| row.get_i64("cut"));
        if channels
            .failed_uploads
            .get(&resource)
            .is_some_and(|failure| {
                failure.cut == cut
                    && failure
                        .retry_at
                        .is_none_or(|deadline| tokio::time::Instant::now() < deadline)
            })
        {
            continue;
        }
        let result =
            upload_resource(sync, binding, client, channels, resource.clone(), i64::MAX).await;
        match result {
            Ok(_) => {
                channels.failed_uploads.remove(&resource);
            }
            Err(error)
                if matches!(
                    error.error_code(),
                    ErrorCode::UploadTooLarge | ErrorCode::Conflict | ErrorCode::InvalidRequest
                ) =>
            {
                talos_logging::report_error(&error);
                // Remote limits/conflicts may recover without new local facts.
                // InvalidRequest stays parked until the local cut changes.
                let retry_at = matches!(
                    error.error_code(),
                    ErrorCode::UploadTooLarge | ErrorCode::Conflict
                )
                .then(|| tokio::time::Instant::now() + UPLOAD_RETRY_DELAY);
                channels.failed_uploads.insert(
                    resource,
                    FailedUpload {
                        cut,
                        error: error.to_api_error(),
                        retry_at,
                    },
                );
            }
            Err(error) => return Err(error),
        }
    }
    channels.status.send_modify(|value| {
        value.last_error = channels
            .failed_uploads
            .values()
            .next()
            .map(|failure| failure.error.clone());
    });
    Ok(())
}

pub(super) async fn upload_resource(
    sync: &TalosSync,
    binding: &SyncBinding,
    client: &SyncClient,
    channels: &RunnerChannels,
    resource: SyncResourceKey,
    through: i64,
) -> TalosResult<Option<crate::SyncVersion>> {
    let Some(batch) = sync
        .export_pending_through(binding, resource.clone(), 128, through)
        .await?
    else {
        return Ok(None);
    };
    // Join with the persisted local replica checkpoint. Incoming changes are
    // recovered by the feed. The Hub checks the upload against its complete
    // current document, including an authoritative tombstone.
    let committed = sync
        .document_version(binding, &resource)
        .await?
        .unwrap_or_default();
    let (document, hub_version) = join_document(client, &resource, &committed).await?;
    let result = if version_includes(&hub_version, &batch.version).map_err(sync_error)? {
        Ok(hub_version)
    } else {
        document
            .send(DocumentUpdate {
                updates: vec![batch.update.clone()],
                version: batch.version.clone(),
            })
            .await
            .map_err(sync_error)
    };
    let closed = document.close().await.map_err(sync_error);
    let acknowledged = result?;
    if !version_includes(&acknowledged, &batch.version).map_err(sync_error)? {
        return Err(TalosError::new(
            ErrorCode::Conflict,
            "Synchronization hub did not acknowledge this cut.",
        ));
    }
    sync.acknowledge(&batch).await?;
    channels
        .status
        .send_modify(|value| value.last_acknowledged_at = Some(now()));
    closed?;
    Ok(Some(acknowledged))
}

async fn commit_cursor(sync: &TalosSync, binding: &SyncBinding, cursor: &str) -> TalosResult<()> {
    let mut tx = sync.storage.begin_background_transaction().await?;
    tx.execute("UPDATE sync_bindings SET download_cursor = ? WHERE hub_id = ? AND account_id = ? AND source_user_id = ?", &[cursor, &binding.scope.hub_id, &binding.scope.account_id, &binding.source_user_id]).await?;
    tx.commit().await
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}
