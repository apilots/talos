//! Login and configuration policy over the existing synchronization and auth owners.
use super::application_host::ApplicationHost;
use agent_protocol::AgentFuture;
use std::sync::Arc;
use talos_auth::RequestIdentity;
use talos_config::YamlConfigStore;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_service::{Actor, ManagementHost};
#[cfg(not(feature = "sync"))]
use talos_types::SyncConnectionState;
use talos_types::{SyncEnableRequest, SyncStatusResponse};

#[derive(Debug)]
pub(crate) enum RefreshPhase {
    AwaitLifecycle,
    VerifyAccount,
    #[cfg(feature = "sync")]
    LocalSettings,
    #[cfg(feature = "sync")]
    RestoreSync,
    #[cfg(feature = "sync")]
    RestoreSyncAndWorker,
    ReconcileWorkerAndBinding,
    ObserveWorker,
}

pub(crate) struct AccountCredential {
    pub(crate) token: String,
    _lease: talos_auth::AccessTokenLease,
}

impl ApplicationHost {
    pub(crate) fn token_for_owner(&self, owner: &str) -> TalosResult<String> {
        let credential = self
            .tokens
            .read()
            .expect("management tokens lock")
            .get(owner)
            .cloned()
            .ok_or_else(|| {
                TalosError::new(
                    ErrorCode::Unauthenticated,
                    "Management account is not signed in.",
                )
            })?;
        let credential = credential.read().expect("management token lock");
        let identity = self.auth_service.local_identity(&credential.token)?;
        if identity.user_id != owner {
            return Err(TalosError::authorization_denied());
        }
        Ok(credential.token.clone())
    }

    #[cfg(not(feature = "sync"))]
    pub(crate) async fn refresh_account(
        &self,
        actor: &Actor,
        phase: &mut RefreshPhase,
    ) -> TalosResult<()> {
        *phase = RefreshPhase::VerifyAccount;
        let verified = self.verified_actor(actor).await?;
        *phase = RefreshPhase::ReconcileWorkerAndBinding;
        self.attach_signed_in(&verified).await?.map_or(Ok(()), Err)
    }

    pub(crate) async fn verified_actor(&self, actor: &Actor) -> TalosResult<RequestIdentity> {
        let token = actor.access_token.as_deref().ok_or_else(|| {
            TalosError::new(
                ErrorCode::Unauthenticated,
                "Sign in before managing this host.",
            )
        })?;
        let verified = self.auth_service.verify_access_token(token).await?;
        if verified.auth_disabled || verified.user_id != actor.user_id {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "Management account does not match.",
            ));
        }
        Ok(verified)
    }

    pub(crate) fn account_credential(
        &self,
        actor: &RequestIdentity,
    ) -> TalosResult<Arc<std::sync::RwLock<AccountCredential>>> {
        let token = actor.access_token.as_deref().expect("verified token");
        let credential = AccountCredential {
            token: token.to_owned(),
            _lease: self.auth_service.retain_access_token(token)?,
        };
        let token = match self
            .tokens
            .write()
            .expect("management tokens lock")
            .entry(actor.user_id.clone())
        {
            std::collections::hash_map::Entry::Occupied(entry) => {
                *entry.get().write().expect("management token lock") = credential;
                entry.get().clone()
            }
            std::collections::hash_map::Entry::Vacant(entry) => entry
                .insert(Arc::new(std::sync::RwLock::new(credential)))
                .clone(),
        };
        Ok(token)
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
            .stores
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
        if self.worker.is_none() {
            return Ok(false);
        }
        if YamlConfigStore::new(self.config.source_path())
            .removed_registration_owner()?
            .is_some()
        {
            return Ok(false);
        }
        if self.product_owner == talos_types::NodeProductOwner::Computer {
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
                &Actor::regular(&actor.user_id)
                    .with_role(actor.role)
                    .with_user_type(actor.user_type),
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
                #[cfg(feature = "sync")]
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
                #[cfg(feature = "sync")]
                if has_alias {
                    self.stop_account_sync(&actor.user_id).await?;
                }
                self.stores
                    .nodes
                    .reconcile_deleted_registration(&runner.node_id, &actor.user_id)
                    .await?;
                self.stores
                    .sessions
                    .retire_local_registration(&actor.user_id, &runner.node_id)
                    .await?;
                #[cfg(feature = "sync")]
                if let Some(binding) = self.sync.account_binding(actor).await? {
                    self.install_capture(binding)?;
                }
            }
        }
        Ok(())
    }

    pub(crate) async fn attach_signed_in(
        &self,
        actor: &RequestIdentity,
    ) -> TalosResult<Option<TalosError>> {
        let mut enrollment_error = None;
        let store = YamlConfigStore::new(self.config.source_path());
        self.refresh_removed_registration(actor).await?;
        let previous = self
            .local_owner
            .read()
            .expect("local computer owner lock")
            .clone();
        if self.product_owner == talos_types::NodeProductOwner::Computer
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
                host: self,
                config: &self.config,
                store: &store,
                account: &self.service.account,
                auth: &self.auth_service,
                worker: &self.worker_registration,
                execution: self
                    .worker
                    .as_ref()
                    .expect("Enrollment requires a local Worker"),
                rpc: self
                    .worker_rpc
                    .as_ref()
                    .expect("Enrollment requires a local Worker transport"),
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
        #[cfg(feature = "relay-client")]
        self.refresh_product_relay().await?;
        self.account_credential(actor)?;
        #[cfg(feature = "sync")]
        self.install_account_access(actor, self.credentials(actor)?)
            .await?;
        Ok(enrollment_error)
    }

    pub(crate) async fn stop_account(&self, account_id: &str) -> TalosResult<()> {
        #[cfg(feature = "sync")]
        self.stop_account_sync(account_id).await?;
        #[cfg(feature = "sync")]
        self.stores
            .credentials
            .write()
            .expect("Sync credentials lock")
            .remove(account_id);
        self.tokens
            .write()
            .expect("management tokens lock")
            .remove(account_id);
        #[cfg(feature = "relay-client")]
        self.stop_product_relay().await;
        self.computer_directory.clear_account(account_id).await;
        Ok(())
    }
}

impl ManagementHost for ApplicationHost {
    fn resume(&self, actor: &Actor) {
        self.schedule_refresh(actor);
    }
    fn resume_login(&self, actor: &Actor) {
        ApplicationHost::resume_login(self, actor);
    }
    #[cfg(feature = "sync")]
    fn metrics<'a>(
        &'a self,
        actor: &'a Actor,
    ) -> AgentFuture<'a, TalosResult<talos_types::SyncMetricsResponse>> {
        Box::pin(self.read_metrics(actor))
    }
    #[cfg(feature = "sync")]
    fn status<'a>(&'a self, actor: &'a Actor) -> AgentFuture<'a, TalosResult<SyncStatusResponse>> {
        Box::pin(self.read_status(actor))
    }

    #[cfg(feature = "sync")]
    fn enable<'a>(
        &'a self,
        actor: &'a Actor,
        request: SyncEnableRequest,
    ) -> AgentFuture<'a, TalosResult<SyncStatusResponse>> {
        self.enable_replication(actor, request)
    }
    #[cfg(feature = "sync")]
    fn disable<'a>(&'a self, actor: &'a Actor) -> AgentFuture<'a, TalosResult<SyncStatusResponse>> {
        self.disable_replication(actor)
    }
    #[cfg(not(feature = "sync"))]
    fn metrics<'a>(
        &'a self,
        _actor: &'a Actor,
    ) -> AgentFuture<'a, TalosResult<talos_types::SyncMetricsResponse>> {
        Box::pin(async { Err(sync_not_assembled()) })
    }
    #[cfg(not(feature = "sync"))]
    fn status<'a>(&'a self, actor: &'a Actor) -> AgentFuture<'a, TalosResult<SyncStatusResponse>> {
        Box::pin(async move {
            Ok(SyncStatusResponse {
                configured_enabled: Some(false),
                connection: SyncConnectionState::Disabled,
                peer_id: None,
                pending_changes: 0,
                last_acknowledged_at: None,
                last_error: None,
                local_computer_id: self.local_computer_for_client(actor).await?,
                can_claim_local_history: self.trusted_local_host,
            })
        })
    }
    #[cfg(not(feature = "sync"))]
    fn enable<'a>(
        &'a self,
        _actor: &'a Actor,
        _request: SyncEnableRequest,
    ) -> AgentFuture<'a, TalosResult<SyncStatusResponse>> {
        Box::pin(async { Err(sync_not_assembled()) })
    }
    #[cfg(not(feature = "sync"))]
    fn disable<'a>(&'a self, actor: &'a Actor) -> AgentFuture<'a, TalosResult<SyncStatusResponse>> {
        self.status(actor)
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

#[cfg(not(feature = "sync"))]
fn sync_not_assembled() -> TalosError {
    TalosError::new(
        ErrorCode::ServiceUnavailable,
        "Synchronization is not assembled in this product.",
    )
}
