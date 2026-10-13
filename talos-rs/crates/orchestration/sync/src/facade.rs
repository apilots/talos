use std::sync::Arc;

use talos_auth::RequestIdentity;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::{Storage, StorageTransaction};
use talos_sync_sdk::{version_equal, SyncReplica};
use talos_sync_types::SyncError;

use crate::{
    projection, store, SyncBindParams, SyncBinding, SyncCaptureParams, SyncEventMapping,
    SyncExportBatch, SyncOptions, SyncResourceKey, SyncScope, SyncVersion,
};

mod state;

/// Persistent management-side integration. It owns no Agent execution or token.
///
/// The host must run existing migrations before opening this facade. All business
/// capture and imported facts use the host's same transaction; network transport
/// lives in talos-sync-sdk and cannot turn a local commit into a network wait.
pub struct TalosSync {
    pub(crate) storage: Arc<dyn Storage>,
    options: SyncOptions,
    peer_id: String,
    writer_id: u64,
}

impl TalosSync {
    pub async fn open(storage: Arc<dyn Storage>, options: SyncOptions) -> TalosResult<Self> {
        let endpoint = url::Url::parse(&options.hub_endpoint).map_err(|error| {
            TalosError::from_source(
                ErrorCode::InvalidRequest,
                "Invalid synchronization endpoint.",
                error,
            )
        })?;
        if endpoint.scheme() != "wss"
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Synchronization requires an authenticated WSS endpoint.",
            ));
        }
        let mut tx = storage.begin_transaction().await?;
        let (peer_id, writer_id) = store::identity(&mut *tx).await?;
        tx.commit().await?;
        Ok(Self {
            storage,
            options,
            peer_id,
            writer_id,
        })
    }

    pub fn peer_id(&self) -> &str {
        &self.peer_id
    }
    pub fn hub_endpoint(&self) -> &str {
        &self.options.hub_endpoint
    }

    /// Restore an installed association after the host verifies the account again.
    pub async fn binding(
        &self,
        scope: &SyncScope,
        source_user_id: &str,
    ) -> TalosResult<Option<SyncBinding>> {
        let mut tx = self.storage.begin_transaction().await?;
        let binding = store::binding(&mut *tx, scope, source_user_id, &self.peer_id).await?;
        tx.commit().await?;
        Ok(binding)
    }

    /// Durable resource discovery for uploads; this is not a product list index.
    pub async fn pending_resources(
        &self,
        binding: &SyncBinding,
        limit: usize,
    ) -> TalosResult<Vec<SyncResourceKey>> {
        Ok(self
            .pending_resources_after(binding, 0, i64::MAX, limit)
            .await?
            .into_iter()
            .map(|(_, resource)| resource)
            .collect())
    }

    /// Page the original outbox so failed resources cannot starve later facts.
    pub(crate) async fn pending_resources_after(
        &self,
        binding: &SyncBinding,
        after: i64,
        through: i64,
        limit: usize,
    ) -> TalosResult<Vec<(i64, SyncResourceKey)>> {
        if !(1..=1024).contains(&limit) {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Invalid synchronization batch limit.",
            ));
        }
        let mut tx = self.storage.begin_transaction().await?;
        self.require_binding(&mut *tx, binding).await?;
        let rows = tx.query("SELECT kind, resource_id, MIN(seq) AS first_seq FROM sync_outbox WHERE hub_id = ? AND account_id = ? GROUP BY kind, resource_id HAVING MIN(seq) > CAST(? AS INTEGER) AND MIN(seq) <= CAST(? AS INTEGER) ORDER BY MIN(seq) LIMIT ?", &[&binding.scope.hub_id, &binding.scope.account_id, &after.to_string(), &through.to_string(), &limit.to_string()]).await?;
        let resources = rows
            .iter()
            .map(|row| {
                let id = store::text(row, "resource_id")?;
                let sequence = row
                    .get_i64("first_seq")
                    .ok_or_else(|| TalosError::internal("Invalid outbox resource sequence."))?;
                Ok((
                    sequence,
                    match store::text(row, "kind")?.as_str() {
                        "computer" => SyncResourceKey::Computer { computer_id: id },
                        "project" => SyncResourceKey::Project { project_id: id },
                        "session" => SyncResourceKey::Session { session_id: id },
                        "file" => SyncResourceKey::File { file_id: id },
                        _ => {
                            return Err(TalosError::internal(
                                "Invalid persisted synchronization resource kind.",
                            ))
                        }
                    },
                ))
            })
            .collect::<TalosResult<Vec<_>>>()?;
        tx.commit().await?;
        Ok(resources)
    }

    pub async fn download_cursor(&self, binding: &SyncBinding) -> TalosResult<Option<String>> {
        let mut tx = self.storage.begin_transaction().await?;
        self.require_binding(&mut *tx, binding).await?;
        let rows = tx.query("SELECT download_cursor FROM sync_bindings WHERE hub_id = ? AND account_id = ? AND source_user_id = ?", &[&binding.scope.hub_id, &binding.scope.account_id, &binding.source_user_id]).await?;
        let cursor = rows
            .first()
            .and_then(|row| row.get_string("download_cursor"));
        tx.commit().await?;
        Ok(cursor)
    }

    pub async fn document_version(
        &self,
        binding: &SyncBinding,
        resource: &SyncResourceKey,
    ) -> TalosResult<Option<SyncVersion>> {
        let resource = projection::resource(binding, resource);
        let mut tx = self.storage.begin_transaction().await?;
        self.require_binding(&mut *tx, binding).await?;
        let version = store::document(&mut *tx, &binding.scope, &resource)
            .await?
            .map(|record| record.version);
        tx.commit().await?;
        Ok(version)
    }

    /// Bind only after the existing Auth owner verifies the account and the host
    /// explicitly authorizes claiming this source user and these Computer aliases.
    /// This infrastructure does not authenticate public requests or grant execution.
    pub async fn bind_account(
        &self,
        actor: &RequestIdentity,
        params: SyncBindParams,
    ) -> TalosResult<SyncBinding> {
        if actor.auth_disabled || actor.user_id != params.scope.account_id {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "Synchronization requires the authenticated account owner.",
            ));
        }
        if [
            &params.scope.hub_id,
            &params.scope.account_id,
            &params.source_user_id,
        ]
        .iter()
        .any(|value| value.is_empty() || value.len() > 128)
        {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Invalid synchronization binding.",
            ));
        }
        let mut sources = std::collections::HashSet::new();
        for alias in &params.computers {
            if alias.source_node_id.is_empty()
                || !sources.insert(&alias.source_node_id)
                || uuid::Uuid::parse_str(&alias.computer_id).is_err()
            {
                return Err(TalosError::new(
                    ErrorCode::InvalidRequest,
                    "Invalid Computer association.",
                ));
            }
        }
        let binding = SyncBinding {
            scope: params.scope,
            peer_id: self.peer_id.clone(),
            source_user_id: params.source_user_id,
            computers: params.computers,
        };
        let mut tx = self.storage.begin_transaction().await?;
        store::save_binding(&mut *tx, &binding).await?;
        tx.commit().await?;
        Ok(binding)
    }

    /// Restore capture associations from this host's durable DB without opening a network connection.
    /// The host must not expose these bindings or use them as account authorization.
    pub async fn saved_bindings(&self) -> TalosResult<Vec<SyncBinding>> {
        let rows = self
            .storage
            .query(
                "SELECT hub_id, account_id, source_user_id, computers_json FROM sync_bindings",
                &[],
            )
            .await?;
        rows.iter()
            .map(|row| {
                Ok(SyncBinding {
                    scope: crate::SyncScope {
                        hub_id: store::text(row, "hub_id")?,
                        account_id: store::text(row, "account_id")?,
                    },
                    peer_id: self.peer_id().into(),
                    source_user_id: store::text(row, "source_user_id")?,
                    computers: store::json(row, "computers_json")?,
                })
            })
            .collect()
    }

    /// Read only the authenticated owner's durable association after a host restart.
    pub async fn account_binding(
        &self,
        actor: &RequestIdentity,
    ) -> TalosResult<Option<SyncBinding>> {
        if actor.auth_disabled {
            return Err(TalosError::new(
                ErrorCode::Unauthenticated,
                "Synchronization requires a signed-in account.",
            ));
        }
        let mut tx = self.storage.begin_transaction().await?;
        let rows = tx
            .query(
                "SELECT hub_id FROM sync_bindings WHERE account_id = ? AND source_user_id = ?",
                &[&actor.user_id, &actor.user_id],
            )
            .await?;
        if rows.len() > 1 {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "The account has more than one synchronization hub association.",
            ));
        }
        let binding = match rows.first() {
            Some(row) => {
                store::binding(
                    &mut *tx,
                    &crate::SyncScope {
                        hub_id: store::text(row, "hub_id")?,
                        account_id: actor.user_id.clone(),
                    },
                    &actor.user_id,
                    self.peer_id(),
                )
                .await?
            }
            None => None,
        };
        tx.commit().await?;
        Ok(binding)
    }

    /// Read durable pending work even while the transport is explicitly disabled.
    pub async fn pending_changes(&self, binding: &SyncBinding) -> TalosResult<u64> {
        let rows = self
            .storage
            .query(
                "SELECT COUNT(*) AS count FROM sync_outbox WHERE hub_id = ? AND account_id = ?",
                &[&binding.scope.hub_id, &binding.scope.account_id],
            )
            .await?;
        Ok(rows
            .first()
            .and_then(|row| row.get_i64("count"))
            .unwrap_or(0) as u64)
    }

    /// Persist this host's verified account choice before or after hub discovery.
    /// The caller owns authentication. Disabling retains bindings and captured facts.
    pub async fn set_enabled(&self, account_id: &str, enabled: bool) -> TalosResult<()> {
        if account_id.is_empty() || account_id.len() > 128 {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Invalid synchronization account.",
            ));
        }
        let mut tx = self.storage.begin_transaction().await?;
        tx.execute("INSERT INTO sync_account_preferences(account_id, enabled) VALUES (?, ?) ON CONFLICT(account_id) DO UPDATE SET enabled = excluded.enabled", &[account_id, if enabled { "1" } else { "0" }]).await?;
        tx.commit().await
    }

    /// Capture canonical facts inside the caller's transaction, with no I/O to H.
    pub async fn capture(
        &self,
        tx: &mut dyn StorageTransaction,
        params: SyncCaptureParams,
    ) -> TalosResult<()> {
        self.require_binding(tx, &params.binding).await?;
        let resource = projection::resource(&params.binding, &params.resource);
        resource.validate().map_err(sync_error)?;
        let changes = params
            .changes
            .into_iter()
            .map(|change| projection::change(&params.binding, change))
            .collect::<TalosResult<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let mut batch = Vec::new();
        let mut bytes = 0;
        for change in changes {
            let size = store::encode(&change)?.len();
            if size > 8 * 1024 * 1024 {
                return Err(TalosError::new(
                    ErrorCode::UploadTooLarge,
                    "A synchronization fact exceeds the limit.",
                ));
            }
            if bytes + size > 8 * 1024 * 1024 && !batch.is_empty() {
                store::append(tx, &params.binding, &resource, &batch, params.created_at).await?;
                batch.clear();
                bytes = 0;
            }
            bytes += size;
            batch.push(change);
        }
        if !batch.is_empty() {
            store::append(tx, &params.binding, &resource, &batch, params.created_at).await?;
        }
        Ok(())
    }

    /// Form and persist one fixed upload cut. A lost ACK retains the same facts.
    pub async fn export_pending(
        &self,
        binding: &SyncBinding,
        resource: SyncResourceKey,
        limit: usize,
    ) -> TalosResult<Option<SyncExportBatch>> {
        let resource = projection::resource(binding, &resource);
        if !(1..=1024).contains(&limit) {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Invalid synchronization batch limit.",
            ));
        }
        let mut tx = self.storage.begin_transaction().await?;
        self.require_binding(&mut *tx, binding).await?;
        let rows = store::outbox(&mut *tx, &binding.scope, &resource, limit).await?;
        let Some(last) = rows.last() else {
            tx.commit().await?;
            return Ok(None);
        };
        let through_sequence = last.seq;
        let (mut document, acknowledged) = self
            .load_document(&mut *tx, &binding.scope, &resource)
            .await?;
        for row in &rows {
            if row.exported_version.is_none() {
                document.apply(&row.changes).map_err(sync_error)?;
            }
        }
        let version = document.version().map_err(sync_error)?;
        let update = document.export_since(&acknowledged).map_err(sync_error)?;
        let version_json = store::encode(&version)?;
        store::save_document(
            &mut *tx,
            &binding.scope,
            &document.summary().map_err(sync_error)?,
            &document.snapshot().map_err(sync_error)?,
            &acknowledged,
        )
        .await?;
        for row in &rows {
            if row.exported_version.is_none() {
                tx.execute(
                    "UPDATE sync_outbox SET exported_version_json = ? WHERE seq = ?",
                    &[&version_json, &row.seq.to_string()],
                )
                .await?;
            }
        }
        tx.commit().await?;
        Ok(Some(SyncExportBatch {
            scope: binding.scope.clone(),
            resource,
            through_sequence,
            update,
            version,
        }))
    }

    /// Called only for the hub's durable upload ACK of this exact exported cut.
    pub async fn acknowledge(&self, batch: &SyncExportBatch) -> TalosResult<()> {
        let mut tx = self.storage.begin_transaction().await?;
        let rows = tx.query("SELECT exported_version_json FROM sync_outbox WHERE seq = ? AND hub_id = ? AND account_id = ? AND kind = ? AND resource_id = ?", &[&batch.through_sequence.to_string(), &batch.scope.hub_id, &batch.scope.account_id, batch.resource.kind().as_str(), batch.resource.id()]).await?;
        let Some(row) = rows.first() else {
            tx.commit().await?;
            return Ok(());
        };
        if !talos_sync_sdk::version_includes(
            &batch.version,
            &store::json::<SyncVersion>(row, "exported_version_json")?,
        )
        .map_err(sync_error)?
        {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Synchronization acknowledgement does not match the exported cut.",
            ));
        }
        let (document, acknowledged) = self
            .load_document(&mut *tx, &batch.scope, &batch.resource)
            .await?;
        let acknowledged =
            talos_sync_sdk::version_union(&acknowledged, &batch.version).map_err(sync_error)?;
        store::save_document(
            &mut *tx,
            &batch.scope,
            &document.summary().map_err(sync_error)?,
            &document.snapshot().map_err(sync_error)?,
            &acknowledged,
        )
        .await?;
        for row in store::outbox(&mut *tx, &batch.scope, &batch.resource, 1024).await? {
            if row.seq <= batch.through_sequence {
                if let Some(version) = &row.exported_version {
                    if talos_sync_sdk::version_includes(&batch.version, version)
                        .map_err(sync_error)?
                    {
                        tx.execute(
                            "DELETE FROM sync_outbox WHERE seq = ?",
                            &[&row.seq.to_string()],
                        )
                        .await?;
                    }
                }
            }
        }
        tx.commit().await
    }

    /// Merge a downloaded candidate in the same transaction as original Store
    /// materialization/event mapping. The host must apply the returned facts before
    /// committing, then call the SDK's mark_committed; no download ACK is emitted.
    pub async fn import_document(
        &self,
        tx: &mut dyn StorageTransaction,
        binding: &SyncBinding,
        resource: &SyncResourceKey,
        updates: &[Vec<u8>],
        downloaded_version: &SyncVersion,
        download_cursor: Option<&str>,
    ) -> TalosResult<(SyncVersion, Vec<talos_sync_types::SyncChange>)> {
        self.require_binding(tx, binding).await?;
        let (mut document, acknowledged) = self.load_document(tx, &binding.scope, resource).await?;
        self.apply_pending(tx, binding, resource, &mut document)
            .await?;
        document.import(updates).map_err(sync_error)?;
        document.set_writer_id(self.writer_id).map_err(sync_error)?;
        let version = document.version().map_err(sync_error)?;
        if !talos_sync_sdk::version_includes(&version, downloaded_version).map_err(sync_error)? {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Downloaded synchronization dependencies are incomplete.",
            ));
        }
        let acknowledged =
            talos_sync_sdk::version_union(&acknowledged, downloaded_version).map_err(sync_error)?;
        if document
            .deleted_at(downloaded_version)
            .map_err(sync_error)?
        {
            // H's committed terminal state supersedes offline patches. This is
            // not an upload ACK and must not discard a merely local pending Delete.
            tx.execute("DELETE FROM sync_outbox WHERE hub_id = ? AND account_id = ? AND kind = ? AND resource_id = ?", &[&binding.scope.hub_id, &binding.scope.account_id, resource.kind().as_str(), resource.id()]).await?;
        }
        store::save_document(
            tx,
            &binding.scope,
            &document.summary().map_err(sync_error)?,
            &document.snapshot().map_err(sync_error)?,
            &acknowledged,
        )
        .await?;
        if let Some(cursor) = download_cursor {
            tx.execute(
                "UPDATE sync_bindings SET download_cursor = ? WHERE hub_id = ? AND account_id = ?",
                &[cursor, &binding.scope.hub_id, &binding.scope.account_id],
            )
            .await?;
        }
        Ok((version, document.changes().map_err(sync_error)?))
    }

    /// Persist a stable source key alongside the newly allocated local event cursor.
    pub async fn map_event(
        &self,
        tx: &mut dyn StorageTransaction,
        mapping: SyncEventMapping,
    ) -> TalosResult<()> {
        let key = store::encode(&mapping.key)?;
        let generation = i64::try_from(mapping.generation)
            .map_err(|_| TalosError::new(ErrorCode::InvalidRequest, "Invalid history generation."))?
            .to_string();
        let rows = tx.query("SELECT local_event_id, local_event_seq FROM sync_event_map WHERE hub_id = ? AND account_id = ? AND session_id = ? AND generation = ? AND stable_event_key = ?", &[&mapping.scope.hub_id, &mapping.scope.account_id, &mapping.session_id, &generation, &key]).await?;
        if let Some(row) = rows.first() {
            if store::text(row, "local_event_id")? != mapping.local_event_id
                || row.get_i64("local_event_seq") != Some(mapping.local_event_seq)
            {
                return Err(TalosError::new(
                    ErrorCode::Conflict,
                    "Stable event already maps to another local cursor.",
                ));
            }
            return Ok(());
        }
        tx.execute("INSERT INTO sync_event_map(hub_id, account_id, session_id, generation, stable_event_key, local_event_id, local_event_seq) VALUES (?, ?, ?, ?, ?, ?, ?)", &[&mapping.scope.hub_id, &mapping.scope.account_id, &mapping.session_id, &generation, &key, &mapping.local_event_id, &mapping.local_event_seq.to_string()]).await?;
        Ok(())
    }

    async fn require_binding(
        &self,
        tx: &mut dyn StorageTransaction,
        binding: &SyncBinding,
    ) -> TalosResult<()> {
        if binding.peer_id != self.peer_id
            || store::binding(tx, &binding.scope, &binding.source_user_id, &self.peer_id)
                .await?
                .as_ref()
                != Some(binding)
        {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "Synchronization binding is not installed in this management database.",
            ));
        }
        Ok(())
    }

    async fn load_document(
        &self,
        tx: &mut dyn StorageTransaction,
        scope: &SyncScope,
        resource: &SyncResourceKey,
    ) -> TalosResult<(SyncReplica, SyncVersion)> {
        match store::document(tx, scope, resource).await? {
            Some(record) => {
                let document = SyncReplica::from_snapshot(resource.clone(), &record.snapshot)
                    .map_err(sync_error)?;
                document.set_writer_id(self.writer_id).map_err(sync_error)?;
                if !version_equal(&document.version().map_err(sync_error)?, &record.version)
                    .map_err(sync_error)?
                {
                    return Err(TalosError::internal(
                        "Persisted synchronization document version does not match.",
                    ));
                }
                Ok((document, record.acknowledged))
            }
            None => Ok((
                SyncReplica::new(resource.clone(), self.writer_id).map_err(sync_error)?,
                SyncVersion::default(),
            )),
        }
    }
}

pub(crate) fn sync_error(error: SyncError) -> TalosError {
    match projection::transcode::<_, talos_error::ApiError>(&error) {
        Ok(error) => error.into(),
        Err(error) => error,
    }
}
