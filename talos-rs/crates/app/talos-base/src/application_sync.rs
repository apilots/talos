//! Optional replication adapter over the existing host, protocol and Store capture.
use super::application_host::{ApplicationHost, ApplicationStores};
use std::sync::Arc;
use talos_auth::RequestIdentity;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_store::{session::DatabaseSessionStore, sync::StoreSyncAdapter};
use talos_sync::{SyncBindParams, SyncHandle, SyncStartParams};

impl crate::TalosApplicationRuntime {
    pub async fn start_account_sync(
        &self,
        actor: &RequestIdentity,
        binding: SyncBindParams,
        credentials: talos_sync_sdk::CredentialCallback,
    ) -> TalosResult<Arc<SyncHandle>> {
        self.host
            .start_account_sync(actor, binding, credentials)
            .await
    }
}

impl ApplicationHost {
    pub(crate) fn install_capture(
        &self,
        binding: talos_sync::SyncBinding,
    ) -> TalosResult<Arc<StoreSyncAdapter>> {
        let configured_default_mode = talos_types::UserPermissionMode::from(
            &self
                .config
                .agent
                .kernel
                .tools
                .sandbox_policy
                .user_permission_mode,
        );
        let adapter = Arc::new(StoreSyncAdapter::new(
            self.stores.storage.clone(),
            self.sync.clone(),
            binding,
            Arc::new(move |record| {
                Ok(talos_types::SessionInfo {
                    session_kind: record.session_kind,
                    node_id: record.execution_node_id.clone(),
                    id: record.id.clone(),
                    title: record.title.clone(),
                    user_id: record.user_id.clone(),
                    source_type: record.source_type.clone(),
                    source_provider: record.source_provider.clone(),
                    model_selection: record.conf.model_selection.clone().ok_or_else(|| {
                        TalosError::internal("Session model selection is unavailable.")
                    })?,
                    configured_default_mode,
                    effective_mode: record
                        .conf
                        .user_permission_mode
                        .unwrap_or(configured_default_mode),
                    created_at: record.created_at,
                    last_activity: record.last_activity,
                    message_count: record.message_count,
                    usage: record.usage.clone(),
                    project_id: record.project_id.clone(),
                    current_directory: record.current_directory.clone(),
                    pinned: record.pinned,
                })
            }),
        ));
        self.stores.sessions.install_sync(adapter.clone())?;
        self.stores.projects.install_sync(adapter.clone())?;
        self.stores.files.install_sync(adapter.clone())?;
        adapter.start_capture();
        Ok(adapter)
    }
    /// Compose one account after original Auth verification. Anonymous history
    /// claiming and UI defaults belong to the product login flow, not this API.
    pub async fn start_account_sync(
        &self,
        actor: &RequestIdentity,
        binding: SyncBindParams,
        credentials: talos_sync_sdk::CredentialCallback,
    ) -> TalosResult<Arc<SyncHandle>> {
        let token = actor.access_token.as_deref().ok_or_else(|| {
            TalosError::new(
                ErrorCode::Unauthenticated,
                "Synchronization requires a signed-in account.",
            )
        })?;
        let verified = self.auth_service.verify_access_token(token).await?;
        if verified.auth_disabled
            || actor.user_id != verified.user_id
            || binding.source_user_id != verified.user_id
            || binding.scope.account_id != verified.user_id
        {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "Synchronization association is not owned by this account.",
            ));
        }
        let config = self.config.reload()?;
        if config.sync.enabled == Some(false) || !self.sync.enabled(&verified.user_id).await? {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "Synchronization is disabled for this management host or account.",
            ));
        }
        for alias in &binding.computers {
            if alias.source_node_id != "embedded"
                || config
                    .agent_node
                    .runner
                    .as_ref()
                    .is_none_or(|runner| runner.node_id != alias.computer_id)
            {
                return Err(TalosError::new(
                    ErrorCode::PermissionDenied,
                    "Computer alias does not identify this host's registered Worker.",
                ));
            }
        }
        self.verify_computer_aliases(&verified, &binding.computers)
            .await?;
        let mut installed = self.stores.handles.lock().await;
        if self.refreshes_closed() {
            return Err(TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Synchronization host has stopped.",
            ));
        }
        if let Some(handle) = installed.get(&verified.user_id) {
            if handle.scope() == &binding.scope && !handle.is_stopped() {
                return Ok(handle.clone());
            }
            // Retain the retired scope until the replacement is fully installed.
            // A transient bind/install failure must remain discoverable for retry.
            let previous = handle.clone();

            let observer = self
                .stores
                .observers
                .lock()
                .expect("Sync observers lock")
                .remove(&verified.user_id);
            if let Some(observer) = observer {
                super::application_sync_observer::join_observer(observer).await;
            }
            previous.shutdown().await?;
        }
        let binding = self.sync.bind_account(&verified, binding).await?;
        self.install_account_access(&verified, credentials.clone())
            .await?;
        let adapter = self.install_capture(binding.clone())?;
        let trusted_ca_pem = self
            .config
            .sync
            .trusted_ca_file
            .as_ref()
            .map(std::fs::read)
            .transpose()
            .map_err(|error| {
                TalosError::internal_with_source(
                    "Read sync trust certificate; technical details were withheld.",
                    error,
                )
            })?;
        let handle = Arc::new(self.sync.clone().start(
            adapter.clone(),
            SyncStartParams {
                binding,
                credentials,
                trusted_ca_pem,
                reconnect_delay: std::time::Duration::from_secs(1),
            },
        )?);
        adapter.attach_handle(Arc::downgrade(&handle));

        self.observe_sync(&verified.user_id, &handle);
        installed.insert(verified.user_id, handle.clone());
        Ok(handle)
    }
}

/// Recover a bounded notification gap from the original durable projections.
/// The runner has already committed these facts; this does not replay execution.
pub(super) async fn publish_account_changes(
    sessions: &DatabaseSessionStore,
    events: &talos_service::TimelineEventService,
    account_id: &str,
) -> TalosResult<()> {
    use talos_store::session::{SessionQuery, SessionStore};
    let mut offset = 0;
    loop {
        let records = sessions
            .list_session_records(&SessionQuery {
                worker_execution_only: true,
                user_id: Some(account_id.into()),
                limit: Some(64),
                offset: Some(offset),
                ..Default::default()
            })
            .await?;
        let length = records.len();
        for record in records {
            events.publish_imported(&record.id).await?;
        }
        if length < 64 {
            return Ok(());
        }
        offset += length;
    }
}

impl ApplicationStores {
    pub(crate) async fn remove_handle(&self, account_id: &str) -> Option<Arc<SyncHandle>> {
        let (handle, observer) = {
            let mut handles = self.handles.lock().await;
            let handle = handles.remove(account_id);
            let observer = self
                .observers
                .lock()
                .expect("Sync observers lock")
                .remove(account_id);
            (handle, observer)
        };
        if let Some(task) = observer {
            super::application_sync_observer::join_observer(task).await;
        }
        handle
    }

    pub(crate) async fn shutdown_handles(&self) -> TalosResult<()> {
        let (handles, observers) = {
            let mut installed = self.handles.lock().await;
            let handles = std::mem::take(&mut *installed);
            let observers = self.observers.lock().expect("Sync observers lock").drain();
            (handles, observers)
        };
        for observer in observers {
            super::application_sync_observer::join_observer(observer).await;
        }
        let mut failure = None;
        for handle in handles.into_values() {
            if let Err(error) = handle.shutdown().await {
                if failure.is_some() {
                    talos_logging::report_error(&error);
                } else {
                    failure = Some(error);
                }
            }
        }
        failure.map_or(Ok(()), Err)
    }

    pub(crate) async fn shutdown(&self) -> TalosResult<()> {
        self.credentials
            .write()
            .expect("Sync credentials lock")
            .clear();
        self.shutdown_handles().await
    }
}
