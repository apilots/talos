//! Login and configuration policy over the existing synchronization and auth owners.
use super::application_sync::ApplicationSyncHost;
use agent_protocol::AgentFuture;
use std::sync::Arc;
use talos_auth::RequestIdentity;
use talos_config::YamlConfigStore;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_service::{Actor, ManagementSync};
use talos_sync::{ComputerAlias, SyncBindParams};
use talos_types::{SyncConnectionState, SyncEnableRequest, SyncStatusResponse};

#[derive(Debug)]
pub(crate) enum RefreshPhase {
    AwaitLifecycle,
    VerifyAccount,
    LocalSettings,
    RestoreSync,
    ReconcileWorkerAndBinding,
    ObserveWorker,
}

impl ApplicationSyncHost {
    pub(crate) async fn verified_actor(&self, actor: &Actor) -> TalosResult<RequestIdentity> {
        let token = actor.access_token.as_deref().ok_or_else(|| {
            TalosError::new(
                ErrorCode::Unauthenticated,
                "Sign in before synchronizing this host.",
            )
        })?;
        let verified = self.auth_service.verify_access_token(token).await?;
        if verified.auth_disabled || verified.user_id != actor.user_id {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "Synchronization account does not match.",
            ));
        }
        Ok(verified)
    }

    fn credentials(&self, actor: &RequestIdentity) -> talos_sync_sdk::CredentialCallback {
        let token = self
            .tokens
            .write()
            .expect("management tokens lock")
            .entry(actor.user_id.clone())
            .or_insert_with(|| Arc::new(std::sync::RwLock::new(String::new())))
            .clone();
        *token.write().expect("management token lock") =
            actor.access_token.clone().expect("verified token");
        let account_id = actor.user_id.clone();
        let auth = self.auth_service.clone();
        Arc::new(move || {
            let account_id = account_id.clone();
            let token = token.clone();
            let auth = auth.clone();
            Box::pin(async move {
                let token = token.read().expect("management token lock").clone();
                let verified = auth.local_identity(&token).map_err(|error| {
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
                    access_token: token,
                })
            })
        })
    }

    /// Read identity and durable removal locally; retirement belongs to maintenance.
    pub(crate) async fn local_computer_id(&self) -> TalosResult<String> {
        let store = YamlConfigStore::new(self.config.source_path());
        if store.removed_registration_owner()?.is_some() {
            return Ok("embedded".into());
        }
        let Some(runner) = store.load_agent_node()?.runner else {
            return Ok("embedded".into());
        };
        use talos_store::node::AgentNodeStore;
        if self
            .sync_stores
            .nodes
            .deletion_owner(&runner.node_id)
            .await?
            .is_some()
        {
            return Ok("embedded".into());
        }
        Ok(runner.node_id)
    }

    /// The central Server's first administrator claims its Worker. Later logins
    /// must preserve that personal owner and must not interrupt its registration.
    async fn can_enroll_local_worker(&self, actor: &RequestIdentity) -> TalosResult<bool> {
        if YamlConfigStore::new(self.config.source_path())
            .removed_registration_owner()?
            .is_some()
        {
            return Ok(false);
        }
        if self.config.agent_node.control_plane.is_none() {
            return Ok(true);
        }
        let store = YamlConfigStore::new(self.config.source_path());
        let Some(runner) = store.load_agent_node()?.runner else {
            return Ok(actor.role == talos_auth::UserRole::Admin);
        };
        let node = match self
            .service
            .nodes
            .get_node(
                &Actor::regular(&actor.user_id).with_role(actor.role),
                talos_types::GetNodeRequest {
                    node_id: runner.node_id.clone(),
                },
            )
            .await
        {
            Ok(node) => node,
            // Directory reads hide another user's Computer from regular users.
            // That expected denial must not mark this host's registration failed.
            Err(error)
                if actor.role != talos_auth::UserRole::Admin
                    && error.is_code(ErrorCode::ResourceNotFound) =>
            {
                return Ok(false);
            }
            Err(error) if error.is_code(ErrorCode::ResourceExpired) => {
                self.observe_registration_removal(actor, &runner.node_id)
                    .await?;
                *self.local_owner.write().expect("local computer owner lock") =
                    Some(actor.user_id.clone());
                return Ok(false);
            }
            Err(error) => return Err(error),
        };
        Ok(node.owner_user_id.as_deref() == Some(actor.user_id.as_str()))
    }

    pub(crate) async fn retire_removed_local_registration(
        &self,
        actor: &RequestIdentity,
    ) -> TalosResult<()> {
        let store = YamlConfigStore::new(self.config.source_path());
        if store.removed_registration_owner()?.as_deref() == Some(&actor.user_id) {
            if let Some(runner) = store.load_agent_node()?.runner {
                let has_alias = self
                    .sync
                    .account_binding(actor)
                    .await?
                    .is_some_and(|binding| {
                        binding
                            .computers
                            .iter()
                            .any(|alias| alias.computer_id == runner.node_id)
                    });
                if has_alias {
                    self.stop_account_sync(&actor.user_id).await?;
                }
                self.sync_stores
                    .nodes
                    .reconcile_deleted_registration(&runner.node_id, &actor.user_id)
                    .await?;
                self.sync_stores
                    .sessions
                    .retire_local_registration(&actor.user_id, &runner.node_id)
                    .await?;
                if let Some(binding) = self.sync.account_binding(actor).await? {
                    self.install_capture(binding)?;
                }
            }
        }
        Ok(())
    }

    async fn attach_signed_in(
        &self,
        actor: &RequestIdentity,
    ) -> TalosResult<(talos_sync_sdk::CredentialCallback, Option<TalosError>)> {
        let mut enrollment_error = None;
        let store = YamlConfigStore::new(self.config.source_path());
        self.refresh_removed_registration(actor).await?;
        let previous = self
            .local_owner
            .read()
            .expect("local computer owner lock")
            .clone();
        if self.config.agent_node.control_plane.is_none()
            && previous
                .as_ref()
                .is_some_and(|owner| owner != &actor.user_id)
        {
            self.stop_account(previous.as_deref().expect("previous account"))
                .await?;
            if let Some(worker) = self.worker_registration.lock().await.take() {
                worker.shutdown().await?;
            }
            *self.local_owner.write().expect("local computer owner lock") = None;
        }
        let owner_matches = self
            .local_owner
            .read()
            .expect("local computer owner lock")
            .as_deref()
            == Some(&actor.user_id);
        let finished = self
            .worker_registration
            .lock()
            .await
            .as_ref()
            .is_none_or(talos_worker::RegisteredWorker::is_finished);
        if (!owner_matches || finished) && self.can_enroll_local_worker(actor).await? {
            let name = std::env::var("TALOS_DESKTOP_HOSTNAME")
                .unwrap_or_else(|_| self.config.agent.name.clone());
            let enrollment = super::desktop_worker::WorkerEnrollment {
                sync_host: self,
                config: &self.config,
                store: &store,
                account: &self.service.account,
                auth: &self.auth_service,
                worker: &self.worker_registration,
                execution: &self.worker,
                rpc: &self.worker_rpc,
                device_name: &name,
                worker_failures: &self.worker_failures,
            }
            .enroll_worker(None, Some(actor))
            .await;
            match enrollment {
                Ok(_) => {
                    *self.local_owner.write().expect("local computer owner lock") =
                        Some(actor.user_id.clone());
                }
                Err(error) => {
                    // Another administrator may have claimed during the initial check.
                    // Only the current owner can report this host's enrollment failure.
                    if self.can_enroll_local_worker(actor).await? {
                        self.worker_failures
                            .send_replace(Some(error.to_api_error()));
                        enrollment_error = Some(error);
                    }
                }
            }
        }
        let credentials = self.credentials(actor);
        let owns_local = self.config.agent_node.control_plane.is_none()
            && self
                .owns_local_computer(&Actor::regular(&actor.user_id))
                .await?;
        if let Some(runner) = store
            .load_agent_node()?
            .runner
            .filter(|_| owns_local && enrollment_error.is_none())
        {
            match self.connect_computer(actor, &runner.node_id).await {
                Err(error) => {
                    self.worker_failures
                        .send_replace(Some(error.to_api_error()));
                    enrollment_error = Some(error);
                }
                Ok(()) => {
                    self.worker_failures
                        .send_if_modified(|error| error.take().is_some());
                }
            }
        }

        self.install_account_access(actor, credentials.clone())
            .await?;
        Ok((credentials, enrollment_error))
    }

    async fn bind_signed_in(&self, actor: &RequestIdentity) -> TalosResult<Option<TalosError>> {
        let (credentials, enrollment_error) = self.attach_signed_in(actor).await?;
        // H initialization must still proceed when G enrollment fails. The owner
        // retries the incomplete lifecycle without duplicating H reconnect.
        match (
            self.bind_document_sync(actor, credentials).await,
            enrollment_error,
        ) {
            (Err(error), Some(enrollment_error)) => {
                // Preserve an otherwise-lost independent G failure; the caller
                // reports the primary document initialization failure once.
                talos_logging::report_error(&enrollment_error);
                Err(error)
            }
            (Err(error), None) => Err(error),
            (Ok(()), enrollment_error) => Ok(enrollment_error),
        }
    }

    async fn bind_document_sync(
        &self,
        actor: &RequestIdentity,
        credentials: talos_sync_sdk::CredentialCallback,
    ) -> TalosResult<()> {
        if !self.sync.enabled(&actor.user_id).await? {
            return Ok(());
        }
        // Late enrollment refreshes presence without rewriting capture aliases
        // or replacing the account's running document synchronization handle.
        let installed = self
            .sync_stores
            .handles
            .lock()
            .await
            .get(&actor.user_id)
            .cloned();
        if let Some(handle) = installed {
            self.refresh_presence_computers(actor, &handle).await?;
            return Ok(());
        }
        let mut binding = self.sync.account_binding(actor).await?;
        if binding.is_none() {
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
            let client = talos_sync_sdk::SyncClient::connect(options, credentials.clone())
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
            self.verify_computer_aliases(actor, &computers).await?;
            binding = Some(
                self.sync
                    .bind_account(
                        actor,
                        SyncBindParams {
                            scope,
                            source_user_id: actor.user_id.clone(),
                            computers,
                        },
                    )
                    .await?,
            );
        }
        let binding = binding.expect("account binding");
        let handle = self
            .start_account_sync(
                actor,
                SyncBindParams {
                    scope: binding.scope,
                    source_user_id: binding.source_user_id,
                    computers: binding.computers,
                },
                credentials,
            )
            .await?;
        self.refresh_presence_computers(actor, &handle).await?;
        Ok(())
    }

    async fn stop_account_sync(&self, account_id: &str) -> TalosResult<()> {
        self.service.remove_sync_handle(account_id);
        let handle = self.sync_stores.remove_handle(account_id).await;
        if let Some(handle) = handle {
            handle.shutdown().await?;
        }
        Ok(())
    }

    pub(crate) async fn stop_account(&self, account_id: &str) -> TalosResult<()> {
        self.stop_account_sync(account_id).await?;
        self.sync_stores
            .credentials
            .write()
            .expect("Sync credentials lock")
            .remove(account_id);
        self.tokens
            .write()
            .expect("management tokens lock")
            .remove(account_id);
        let mut connections = self.sync_stores.connections.lock().await;
        let removed = connections
            .extract_if(|_, (owner, _)| owner == account_id)
            .map(|(computer, (_, connection))| (computer, connection))
            .collect::<Vec<_>>();
        drop(connections);
        for (computer, connection) in removed {
            if let Some(connection) = connection.upgrade() {
                connection.close();
            }
            self.coordinator.unregister_remote_node(&computer).await;
        }
        Ok(())
    }

    /// Called only by owned maintenance or an explicit mutation lifecycle.
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
            let accounts = self
                .sync_stores
                .handles
                .lock()
                .await
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            for account_id in accounts {
                self.service.remove_sync_handle(&account_id);
            }
            self.sync_stores.shutdown_handles().await?;
            *phase = RefreshPhase::ReconcileWorkerAndBinding;
            let (_, enrollment_error) = self.attach_signed_in(&verified).await?;
            return enrollment_error.map_or(Ok(()), Err);
        }
        // Restore document transport from its durable binding before any Worker
        // registration RPC. An unavailable G must not delay an existing H handle.
        *phase = RefreshPhase::RestoreSync;
        if self.sync.enabled(&verified.user_id).await? {
            if let Some(binding) = self.sync.account_binding(&verified).await? {
                self.start_account_sync(
                    &verified,
                    SyncBindParams {
                        scope: binding.scope,
                        source_user_id: binding.source_user_id,
                        computers: binding.computers,
                    },
                    self.credentials(&verified),
                )
                .await?;
            }
        }
        *phase = RefreshPhase::ReconcileWorkerAndBinding;
        self.bind_signed_in(&verified).await?.map_or(Ok(()), Err)
    }

    async fn read_status(&self, actor: &Actor) -> TalosResult<SyncStatusResponse> {
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
            .sync_stores
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
}

impl ManagementSync for ApplicationSyncHost {
    fn resume(&self, actor: &Actor) {
        self.schedule_refresh(actor);
    }
    fn resume_login(&self, actor: &Actor) {
        ApplicationSyncHost::resume_login(self, actor);
    }
    fn metrics<'a>(
        &'a self,
        actor: &'a Actor,
    ) -> AgentFuture<'a, TalosResult<talos_types::SyncMetricsResponse>> {
        Box::pin(self.read_metrics(actor))
    }
    fn status<'a>(&'a self, actor: &'a Actor) -> AgentFuture<'a, TalosResult<SyncStatusResponse>> {
        Box::pin(self.read_status(actor))
    }

    fn enable<'a>(
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
            let pause = self.pause_refresh(&actor.user_id).await;
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
            drop(pause);
            self.schedule_refresh(actor);
            result
        })
    }
    fn disable<'a>(&'a self, actor: &'a Actor) -> AgentFuture<'a, TalosResult<SyncStatusResponse>> {
        Box::pin(async move {
            let verified = if actor.auth_disabled {
                None
            } else {
                Some(self.verified_actor(actor).await?)
            };
            let pause = self.pause_refresh(&actor.user_id).await;
            let _guard = self.lifecycle.lock().await;
            let result = async {
                if let Some(verified) = verified {
                    self.sync.set_enabled(&verified.user_id, false).await?;
                    // Only H stops. G remains available for RPC and source FileService callbacks.
                    self.service.remove_sync_handle(&actor.user_id);
                    let handle = self.sync_stores.remove_handle(&actor.user_id).await;
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
            drop(pause);
            self.schedule_refresh(actor);
            result
        })
    }
    fn suspend<'a>(&'a self, actor: &'a Actor) -> AgentFuture<'a, TalosResult<()>> {
        Box::pin(async move {
            let _pause = self.suspend_refresh(actor).await;
            let _guard = self.lifecycle.lock().await;
            self.stop_account(&actor.user_id).await
        })
    }
    fn connect_computer<'a>(
        &'a self,
        actor: &'a Actor,
        computer_id: &'a str,
    ) -> AgentFuture<'a, TalosResult<String>> {
        Box::pin(self.resolve_computer(actor, computer_id))
    }
    fn local_computer<'a>(
        &'a self,
        actor: &'a Actor,
    ) -> AgentFuture<'a, TalosResult<Option<String>>> {
        Box::pin(self.local_computer_for_client(actor))
    }
    fn resolve_node<'a>(
        &'a self,
        actor: &'a Actor,
        computer_id: &'a str,
    ) -> AgentFuture<'a, TalosResult<String>> {
        Box::pin(self.local_node(actor, computer_id))
    }
    fn computer_info<'a>(
        &'a self,
        actor: &'a Actor,
        info: talos_types::NodeInfo,
    ) -> AgentFuture<'a, TalosResult<talos_types::NodeInfo>> {
        Box::pin(self.computer_view(actor, info))
    }
}
