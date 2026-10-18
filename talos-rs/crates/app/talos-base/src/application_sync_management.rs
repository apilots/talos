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
        let account = self.service.account.clone();
        let auth = self.auth_service.clone();
        Arc::new(move || {
            let account_id = account_id.clone();
            let token = token.clone();
            let account = account.clone();
            let auth = auth.clone();
            Box::pin(async move {
                let candidate = account
                    .read_auth_session(&Actor::regular(&account_id), Default::default())
                    .map_err(|error| {
                        talos_logging::report_error(&error);
                        talos_sync_types::SyncError::new(
                            talos_sync_types::SyncErrorCode::InternalError,
                        )
                    })?
                    .session
                    .filter(|session| session.user.user_id == account_id);
                let token = candidate.map_or_else(
                    || token.read().expect("management token lock").clone(),
                    |session| session.access_token,
                );
                let verified = auth.verify_access_token(&token).await.map_err(|error| {
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

    pub(crate) fn local_computer_id(&self) -> TalosResult<String> {
        if YamlConfigStore::new(self.config.source_path())
            .removed_registration_owner()?
            .is_some()
        {
            return Ok("embedded".into());
        }
        Ok(self
            .config
            .reload()?
            .agent_node
            .runner
            .map_or_else(|| "embedded".into(), |runner| runner.node_id))
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
    ) -> TalosResult<talos_sync_sdk::CredentialCallback> {
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
                        talos_logging::report_error(&error);
                        self.worker_failures
                            .send_replace(Some(error.to_api_error()));
                    }
                }
            }
        }
        let credentials = self.credentials(actor);
        let owns_local = self.config.agent_node.control_plane.is_none()
            && self
                .owns_local_computer(&Actor::regular(&actor.user_id))
                .await?;
        if let Some(runner) = store.load_agent_node()?.runner.filter(|_| owns_local) {
            match self.connect_computer(actor, &runner.node_id).await {
                Err(error) => {
                    self.worker_failures
                        .send_replace(Some(error.to_api_error()));
                }
                Ok(()) => {
                    self.worker_failures
                        .send_if_modified(|error| error.take().is_some());
                }
            }
        }

        self.install_account_access(actor, credentials.clone())
            .await?;
        Ok(credentials)
    }

    async fn bind_signed_in(&self, actor: &RequestIdentity) -> TalosResult<()> {
        let credentials = self.attach_signed_in(actor).await?;
        if !self.sync.enabled(&actor.user_id).await? {
            return Ok(());
        }
        if let Some(binding) = self.sync.account_binding(actor).await? {
            if !binding
                .computers
                .iter()
                .any(|alias| alias.source_node_id == "embedded")
            {
                if let Some(local) = self
                    .local_computer(
                        &Actor::regular(&actor.user_id)
                            .with_access_token(actor.access_token.clone().expect("verified token")),
                    )
                    .await?
                    .filter(|id| id != "embedded")
                {
                    self.verify_computer_aliases(
                        actor,
                        &[ComputerAlias {
                            source_node_id: "embedded".into(),
                            computer_id: local.clone(),
                        }],
                    )
                    .await?;
                    self.stop_account_sync(&actor.user_id).await?;
                    let mut computers = binding.computers;
                    computers.push(ComputerAlias {
                        source_node_id: "embedded".into(),
                        computer_id: local,
                    });
                    self.sync
                        .bind_account(
                            actor,
                            SyncBindParams {
                                scope: binding.scope,
                                source_user_id: binding.source_user_id,
                                computers,
                            },
                        )
                        .await?;
                }
            }
        }
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
        if let Some(handle) = self.sync_stores.handles.lock().await.remove(account_id) {
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

    async fn read_status(&self, actor: &Actor) -> TalosResult<SyncStatusResponse> {
        let config = YamlConfigStore::new(self.config.source_path()).load_sync()?;
        let local_computer_id = self.local_computer_for_client(actor).await?;
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
        let connection = if connection == SyncConnectionState::Disabled
            && configured_enabled != Some(false)
            && failed
        {
            SyncConnectionState::Disconnected
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
    fn metrics<'a>(
        &'a self,
        actor: &'a Actor,
    ) -> AgentFuture<'a, TalosResult<talos_types::SyncMetricsResponse>> {
        Box::pin(self.read_metrics(actor))
    }
    fn status<'a>(&'a self, actor: &'a Actor) -> AgentFuture<'a, TalosResult<SyncStatusResponse>> {
        Box::pin(async move {
            let _guard = self.lifecycle.lock().await;
            if !actor.auth_disabled && actor.access_token.is_some() {
                let verified = self.verified_actor(actor).await?;
                if let Err(error) = self.attach_signed_in(&verified).await {
                    self.worker_failures
                        .send_replace(Some(error.to_api_error()));
                }
                if YamlConfigStore::new(self.config.source_path())
                    .load_sync()?
                    .enabled
                    == Some(false)
                {
                    let handles: Vec<_> = self.sync_stores.handles.lock().await.drain().collect();
                    for (account_id, _) in &handles {
                        self.service.remove_sync_handle(account_id);
                    }
                    let mut failure = None;
                    for (_, handle) in handles {
                        if let Err(error) = handle.shutdown().await {
                            failure.get_or_insert(error);
                        }
                    }
                    if let Some(error) = failure {
                        return Err(error);
                    }
                    return self.read_status(actor).await;
                }
                if let Err(error) = self.bind_signed_in(&verified).await {
                    self.failures
                        .write()
                        .expect("sync failures lock")
                        .insert(actor.user_id.clone(), error.to_api_error());
                } else {
                    self.failures
                        .write()
                        .expect("sync failures lock")
                        .remove(&actor.user_id);
                }
            }
            self.read_status(actor).await
        })
    }
    fn enable<'a>(
        &'a self,
        actor: &'a Actor,
        request: SyncEnableRequest,
    ) -> AgentFuture<'a, TalosResult<SyncStatusResponse>> {
        Box::pin(async move {
            let _guard = self.lifecycle.lock().await;
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
            self.bind_signed_in(&verified).await?;
            self.failures
                .write()
                .expect("sync failures lock")
                .remove(&actor.user_id);
            self.read_status(actor).await
        })
    }
    fn disable<'a>(&'a self, actor: &'a Actor) -> AgentFuture<'a, TalosResult<SyncStatusResponse>> {
        Box::pin(async move {
            let _guard = self.lifecycle.lock().await;
            if !actor.auth_disabled {
                let verified = self.verified_actor(actor).await?;
                self.sync.set_enabled(&verified.user_id, false).await?;
                // Only H stops. G remains available for RPC and source FileService callbacks.
                self.service.remove_sync_handle(&actor.user_id);
                if let Some(handle) = self.sync_stores.handles.lock().await.remove(&actor.user_id) {
                    handle.shutdown().await?;
                }
            }
            self.failures
                .write()
                .expect("sync failures lock")
                .remove(&actor.user_id);
            self.read_status(actor).await
        })
    }
    fn suspend<'a>(&'a self, actor: &'a Actor) -> AgentFuture<'a, TalosResult<()>> {
        Box::pin(self.stop_account(&actor.user_id))
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
