//! Optional document-replication policy consumes the existing authenticated host.
use super::application_management_host::RefreshPhase;
use super::{application_host::ApplicationHost, application_sync_background::refresh_retryable};
use agent_protocol::AgentFuture;
use std::sync::Arc;
use talos_auth::RequestIdentity;
use talos_config::YamlConfigStore;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_service::Actor;
use talos_sync::{ComputerAlias, SyncBindParams};
use talos_types::{SyncConnectionState, SyncEnableRequest, SyncStatusResponse};

impl ApplicationHost {
    pub(super) fn credentials(
        &self,
        actor: &RequestIdentity,
    ) -> TalosResult<talos_sync_sdk::CredentialCallback> {
        let token = self.account_credential(actor)?;
        let account_id = actor.user_id.clone();
        let auth = self.auth_service.clone();
        Ok(Arc::new(move || {
            let account_id = account_id.clone();
            let token = token.clone();
            let auth = auth.clone();
            Box::pin(async move {
                let credential = token.read().expect("management token lock");
                let verified = auth.local_identity(&credential.token).map_err(|error| {
                    talos_logging::report_error(&error);
                    let code = match error.error_code() {
                        ErrorCode::Unauthenticated => {
                            talos_sync_types::SyncErrorCode::Unauthenticated
                        }
                        ErrorCode::PermissionDenied => {
                            talos_sync_types::SyncErrorCode::PermissionDenied
                        }
                        ErrorCode::Timeout => talos_sync_types::SyncErrorCode::Timeout,
                        ErrorCode::ServiceUnavailable => {
                            talos_sync_types::SyncErrorCode::ServiceUnavailable
                        }
                        _ => talos_sync_types::SyncErrorCode::InternalError,
                    };
                    talos_sync_types::SyncError::new(code)
                })?;
                if verified.auth_disabled || verified.user_id != account_id {
                    return Err(talos_sync_types::SyncError::new(
                        talos_sync_types::SyncErrorCode::Unauthenticated,
                    ));
                }
                Ok(talos_sync_sdk::VerifiedCredential {
                    account_id,
                    access_token: credential.token.clone(),
                })
            })
        }))
    }

    pub(crate) async fn bind_signed_in(
        &self,
        actor: &RequestIdentity,
    ) -> TalosResult<Option<TalosError>> {
        let enrollment_error = self.attach_signed_in(actor).await?;
        let credentials = self.credentials(actor)?;
        if let Err(error) = self.bind_document_sync(actor, credentials).await {
            if let Some(enrollment_error) = enrollment_error {
                talos_logging::report_error(&enrollment_error);
            }
            return Err(error);
        }
        Ok(enrollment_error)
    }

    pub(crate) async fn bind_document_sync(
        &self,
        actor: &RequestIdentity,
        credentials: talos_sync_sdk::CredentialCallback,
    ) -> TalosResult<()> {
        if !self.sync.enabled(&actor.user_id).await? {
            return Ok(());
        }
        let handle = match self
            .publish_document_binding(actor, credentials.clone(), None)
            .await?
        {
            Some(handle) => handle,
            None => {
                let scope = self
                    .discover_document_scope(actor, credentials.clone())
                    .await?;
                self.publish_document_binding(actor, credentials, Some(scope))
                    .await?
                    .expect("a discovered scope can create a document binding")
            }
        };
        self.refresh_presence_computers(actor, &handle).await
    }

    pub(super) async fn discover_document_scope(
        &self,
        actor: &RequestIdentity,
        credentials: talos_sync_sdk::CredentialCallback,
    ) -> TalosResult<talos_sync::SyncScope> {
        let mut options = talos_sync_sdk::SyncOptions::new(
            self.config.sync.hub_endpoint.clone(),
            actor.user_id.clone(),
            self.sync.peer_id().into(),
        );
        options.trusted_ca_pem = self
            .config
            .sync
            .trusted_ca_file
            .as_ref()
            .map(std::fs::read)
            .transpose()
            .map_err(|error| {
                TalosError::internal_with_source(
                    "Read synchronization CA; technical details were withheld.",
                    error,
                )
            })?;
        let client = talos_sync_sdk::SyncClient::connect(options, credentials)
            .await
            .map_err(|error| {
                TalosError::from_source(
                    ErrorCode::ServiceUnavailable,
                    "Synchronization service is unavailable.",
                    error,
                )
            })?;
        let scope = client.scope().await;
        client.shutdown().await.map_err(|error| {
            TalosError::from_source(
                ErrorCode::ServiceUnavailable,
                "Close synchronization discovery connection.",
                error,
            )
        })?;
        Ok(scope)
    }

    pub(crate) async fn publish_document_binding(
        &self,
        actor: &RequestIdentity,
        credentials: talos_sync_sdk::CredentialCallback,
        discovered_scope: Option<talos_sync::SyncScope>,
    ) -> TalosResult<Option<Arc<talos_sync::SyncHandle>>> {
        // Retirement can rewrite the saved aliases. Re-read the binding under
        // its publication guard after any discovery handshake.
        let _retirement = self.registration_retirement.lock().await;
        if let Some(handle) = self
            .stores
            .handles
            .lock()
            .await
            .get(&actor.user_id)
            .cloned()
        {
            return Ok(Some(handle));
        }
        let params = match self.sync.account_binding(actor).await? {
            Some(binding) => SyncBindParams {
                scope: binding.scope,
                source_user_id: binding.source_user_id,
                computers: binding.computers,
            },
            None => {
                let Some(scope) = discovered_scope else {
                    return Ok(None);
                };
                let computers = self
                    .local_computer(
                        &Actor::regular(&actor.user_id)
                            .with_access_token(actor.access_token.clone().expect("verified token")),
                    )
                    .await?
                    .filter(|local| local != "embedded")
                    .map_or_else(Vec::new, |local| {
                        vec![ComputerAlias {
                            source_node_id: "embedded".into(),
                            computer_id: local,
                        }]
                    });
                SyncBindParams {
                    scope,
                    source_user_id: actor.user_id.clone(),
                    computers,
                }
            }
        };
        self.start_account_sync(actor, params, credentials)
            .await
            .map(Some)
    }

    pub(super) async fn stop_account_sync(&self, account_id: &str) -> TalosResult<()> {
        let handle = self.stores.remove_handle(account_id).await;
        if let Some(handle) = handle {
            handle.shutdown().await?;
        }
        Ok(())
    }

    pub(crate) async fn refresh_account(
        &self,
        actor: &Actor,
        phase: &mut RefreshPhase,
    ) -> TalosResult<()> {
        *phase = RefreshPhase::VerifyAccount;
        let verified = self.verified_actor(actor).await?;
        *phase = RefreshPhase::LocalSettings;
        if YamlConfigStore::new(self.config.source_path())
            .load_sync()?
            .enabled
            == Some(false)
        {
            // Operator opt-out stops H but retains the local Worker's registration.
            self.stores.shutdown_handles().await?;
            *phase = RefreshPhase::ReconcileWorkerAndBinding;
            let enrollment_error = self.attach_signed_in(&verified).await?;
            self.install_enrolled_capture(actor, &verified).await?;
            return enrollment_error.map_or(Ok(()), Err);
        }
        // Both operations can start from a verified account. Join their
        // outcomes so a failure on either side cannot cancel the other.
        let (documents, worker) = tokio::join!(
            self.bind_document_sync(&verified, self.credentials(&verified)?),
            self.attach_signed_in(&verified),
        );
        let restore_failed = documents.is_err();
        let (documents, worker_error) = match (documents, worker) {
            (Ok(()), Ok(enrollment_error)) => {
                let credentials = self.credentials(&verified)?;
                let documents = async {
                    // Hub discovery/restoration can precede Worker enrollment. Fill
                    // only a missing local alias and bootstrap its previously local
                    // facts before publishing presence through the current handle.
                    self.install_enrolled_capture(actor, &verified).await?;
                    self.bind_document_sync(&verified, credentials).await
                }
                .await;
                (documents, enrollment_error)
            }
            (Err(error), Ok(enrollment_error)) => (Err(error), enrollment_error),
            (documents, Err(error)) => (documents, Some(error)),
        };
        *phase = match (restore_failed, worker_error.is_some()) {
            (true, true) => RefreshPhase::RestoreSyncAndWorker,
            (true, false) => RefreshPhase::RestoreSync,
            (false, _) => RefreshPhase::ReconcileWorkerAndBinding,
        };
        match documents {
            Ok(()) => worker_error.map_or(Ok(()), Err),
            Err(error) => Err(combine_refresh_errors(error, worker_error)),
        }
    }

    pub(crate) async fn read_status(&self, actor: &Actor) -> TalosResult<SyncStatusResponse> {
        let config = YamlConfigStore::new(self.config.source_path()).load_sync()?;
        // Snapshot identity without triggering retirement, registration or RPC.
        let local_computer_id = if actor.auth_disabled {
            Some("embedded".into())
        } else {
            if self.owns_local_computer(actor).await? {
                Some(self.local_computer_id().await?)
            } else {
                None
            }
        };
        let handle = self
            .stores
            .handles
            .lock()
            .await
            .get(&actor.user_id)
            .cloned();
        let status = handle.map(|handle| handle.status());
        let connection = status
            .as_ref()
            .map_or(SyncConnectionState::Disabled, |status| status.connection);
        let binding = if actor.auth_disabled {
            None
        } else {
            self.sync
                .account_binding(&RequestIdentity {
                    user_id: actor.user_id.clone(),
                    username: actor.username.clone(),
                    role: actor.role,
                    user_type: actor.user_type,
                    auth_disabled: false,
                    access_token: actor.access_token.clone(),
                })
                .await?
        };
        let pending_changes = match &binding {
            Some(binding) => self.sync.pending_changes(binding).await?,
            None => 0,
        };
        let binding_disabled = !actor.auth_disabled && !self.sync.enabled(&actor.user_id).await?;
        let configured_enabled = if binding_disabled {
            Some(false)
        } else {
            config.enabled
        };
        let failed = self
            .failures
            .read()
            .expect("sync failures lock")
            .contains_key(&actor.user_id);
        let connection = if configured_enabled == Some(false) {
            SyncConnectionState::Disabled
        } else if connection == SyncConnectionState::Disabled
            && !actor.auth_disabled
            && actor.access_token.is_some()
        {
            if failed {
                SyncConnectionState::Disconnected
            } else {
                SyncConnectionState::Connecting
            }
        } else {
            connection
        };
        Ok(SyncStatusResponse {
            configured_enabled,
            connection,
            peer_id: (!actor.auth_disabled).then(|| self.sync.peer_id().to_owned()),
            pending_changes,
            last_acknowledged_at: status
                .as_ref()
                .and_then(|status| status.last_acknowledged_at),
            last_error: status.and_then(|status| status.last_error).or_else(|| {
                self.failures
                    .read()
                    .expect("sync failures lock")
                    .get(&actor.user_id)
                    .cloned()
            }),
            local_computer_id,
            can_claim_local_history: self.trusted_local_host,
        })
    }

    pub(crate) fn enable_replication<'a>(
        &'a self,
        actor: &'a Actor,
        request: SyncEnableRequest,
    ) -> AgentFuture<'a, TalosResult<SyncStatusResponse>> {
        Box::pin(async move {
            let verified = self.verified_actor(actor).await?;
            if YamlConfigStore::new(self.config.source_path())
                .load_sync()?
                .enabled
                == Some(false)
            {
                return Err(TalosError::new(
                    ErrorCode::PermissionDenied,
                    "Synchronization is disabled by this server's operator.",
                ));
            }
            // Sync settings must not abort an in-flight control-plane enrollment.
            let _guard = self.lifecycle.lock().await;
            let result = async {
                if request.claim_local_history {
                    if !self.trusted_local_host {
                        return Err(TalosError::new(
                            ErrorCode::PermissionDenied,
                            "Anonymous history can only be claimed on its trusted local host.",
                        ));
                    }
                    self.service.claim_local_history(&verified.user_id).await?;
                }
                self.sync.set_enabled(&verified.user_id, true).await?;
                if let Some(error) = self.bind_signed_in(&verified).await? {
                    // H enable succeeded. G's independently retained failure is
                    // handled here and retried by the resumed account owner.
                    talos_logging::report_error(&error);
                }
                self.failures
                    .write()
                    .expect("sync failures lock")
                    .remove(&actor.user_id);
                self.read_status(actor).await
            }
            .await;
            self.schedule_refresh(actor);
            result
        })
    }

    pub(crate) fn disable_replication<'a>(
        &'a self,
        actor: &'a Actor,
    ) -> AgentFuture<'a, TalosResult<SyncStatusResponse>> {
        Box::pin(async move {
            let verified = if actor.auth_disabled {
                None
            } else {
                Some(self.verified_actor(actor).await?)
            };
            // Sync settings must not abort an in-flight control-plane enrollment.
            let _guard = self.lifecycle.lock().await;
            let result = async {
                if let Some(verified) = verified {
                    self.sync.set_enabled(&verified.user_id, false).await?;
                    // Only H stops. G remains available for RPC and source FileService callbacks.
                    let handle = self.stores.remove_handle(&actor.user_id).await;
                    if let Some(handle) = handle {
                        handle.shutdown().await?;
                    }
                }
                self.failures
                    .write()
                    .expect("sync failures lock")
                    .remove(&actor.user_id);
                self.read_status(actor).await
            }
            .await;
            self.schedule_refresh(actor);
            result
        })
    }
}

fn combine_refresh_errors(mut primary: TalosError, secondary: Option<TalosError>) -> TalosError {
    if let Some(mut secondary) = secondary {
        // Retry a transient failure even if the independent operation needs
        // operator intervention. Both original failures keep their diagnostics.
        if !refresh_retryable(&primary) && refresh_retryable(&secondary) {
            std::mem::swap(&mut primary, &mut secondary);
        }
        talos_logging::report_error(&secondary);
    }
    primary
}
