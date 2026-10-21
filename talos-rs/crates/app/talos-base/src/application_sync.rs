//! Host composition over original Stores, authenticated SDKs and notifications.
use std::{collections::HashMap, sync::Arc};

use talos_auth::RequestIdentity;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_store::{
    file::DatabaseFileStore, node::DatabaseAgentNodeStore, project::StorageProjectStore,
    session::DatabaseSessionStore, sync::StoreSyncAdapter,
};
use talos_sync::{SyncBindParams, SyncHandle, SyncStartParams};

pub(crate) struct ApplicationSyncStores {
    pub storage: Arc<dyn talos_storage::Storage>,
    pub sessions: Arc<DatabaseSessionStore>,
    pub projects: Arc<StorageProjectStore>,
    pub nodes: Arc<DatabaseAgentNodeStore>,
    pub files: Arc<DatabaseFileStore>,
    pub handles: tokio::sync::Mutex<HashMap<String, Arc<SyncHandle>>>,
    pub observers: std::sync::Mutex<super::application_sync_observer::SyncObservers>,
    pub credentials: std::sync::RwLock<HashMap<String, talos_sync_sdk::CredentialCallback>>,
    pub connections: tokio::sync::Mutex<
        HashMap<
            String,
            (
                String,
                std::sync::Weak<agent_client::AgentExecutionConnection>,
            ),
        >,
    >,
}

pub(crate) struct ApplicationSyncHost {
    pub self_weak: std::sync::Weak<Self>,
    pub registration_retirement: tokio::sync::Mutex<()>,
    pub config: talos_config::Config,
    pub sync: Arc<talos_sync::TalosSync>,
    pub sync_stores: Arc<ApplicationSyncStores>,
    pub auth_service: Arc<dyn talos_auth::AuthService>,
    pub service: Arc<talos_service::TalosService>,
    pub coordinator: Arc<talos_coordinator::CoordinatorImpl>,
    pub event_service: Arc<talos_service::TimelineEventService>,
    pub data: crate::NodeSessionDataRpc,
    pub images: crate::NodeToolImageRpc,
    pub control: Arc<dyn talos_tools::AgentControl>,
    pub worker: talos_worker::TalosWorker,
    pub worker_rpc: Arc<talos_worker::WorkerRpcHost>,
    pub worker_registration: Arc<tokio::sync::Mutex<Option<talos_worker::RegisteredWorker>>>,
    pub worker_failures: tokio::sync::watch::Sender<Option<talos_error::ApiError>>,
    pub trusted_local_host: bool,
    pub local_owner: std::sync::RwLock<Option<String>>,
    pub tokens: std::sync::RwLock<HashMap<String, Arc<std::sync::RwLock<String>>>>,
    pub lifecycle: tokio::sync::Mutex<()>,
    pub background: std::sync::Mutex<super::application_sync_background::RefreshTasks>,
    pub failures: std::sync::RwLock<HashMap<String, talos_error::ApiError>>,
}

impl Drop for ApplicationSyncHost {
    fn drop(&mut self) {
        // Resource callbacks also retain the Stores. Break observer captures here
        // even when the host is dropped without its asynchronous shutdown path.
        self.sync_stores
            .observers
            .lock()
            .expect("Sync observers lock")
            .drain();
    }
}

impl crate::TalosApplicationRuntime {
    pub async fn shutdown_sync(&self) -> TalosResult<()> {
        self.sync_host.shutdown().await
    }
    pub async fn start_account_sync(
        &self,
        actor: &RequestIdentity,
        binding: SyncBindParams,
        credentials: talos_sync_sdk::CredentialCallback,
    ) -> TalosResult<Arc<SyncHandle>> {
        self.sync_host
            .start_account_sync(actor, binding, credentials)
            .await
    }
    pub async fn connect_computer(
        &self,
        actor: &RequestIdentity,
        computer_id: &str,
    ) -> TalosResult<()> {
        self.sync_host.connect_computer(actor, computer_id).await
    }
}

impl ApplicationSyncHost {
    pub(crate) async fn shutdown(&self) -> TalosResult<()> {
        self.shutdown_refreshes().await;
        for account_id in self.sync_stores.handles.lock().await.keys() {
            self.service.remove_sync_handle(account_id);
        }
        self.tokens.write().expect("management tokens lock").clear();
        self.sync_stores.shutdown().await
    }

    fn coordinator_resource_handler(
        &self,
        computer_id: String,
    ) -> Arc<dyn agent_client::AgentResourceRpcHandler> {
        Arc::new(crate::agent_node_rpc::CoordinatorResources {
            service: self.service.clone(),
            computer_id,
            data: self.data.clone(),
            images: self.images.clone(),
            control: self.control.clone(),
            files: self.service.files.clone(),
            auth: self.auth_service.clone(),
        })
    }

    pub(crate) async fn install_account_access(
        &self,
        actor: &RequestIdentity,
        credentials: talos_sync_sdk::CredentialCallback,
    ) -> TalosResult<()> {
        self.sync_stores
            .credentials
            .write()
            .expect("Sync credentials lock")
            .insert(actor.user_id.clone(), credentials.clone());
        Ok(())
    }

    pub(crate) fn install_peer_access(&self) {
        let peer = Arc::new(super::application_sync_files::GatewayPeerAccess {
            host: self.self_weak.clone(),
            config: self.config.clone(),
            stores: self.sync_stores.clone(),
        });
        self.service.files.set_peer_access(peer.clone());
        if self.config.agent_node.control_plane.is_none() {
            self.service.nodes.set_remote_management(peer.clone());
        }
        self.service.set_active_session_commands(peer);
    }

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
            self.sync_stores.storage.clone(),
            self.sync.clone(),
            binding,
            Arc::new(move |record| {
                Ok(talos_types::SessionInfo {
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
                })
            }),
        ));
        self.sync_stores.sessions.install_sync(adapter.clone())?;
        self.sync_stores.projects.install_sync(adapter.clone())?;
        self.sync_stores.nodes.install_sync(adapter.clone())?;
        self.sync_stores.files.install_sync(adapter.clone())?;
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
        let mut installed = self.sync_stores.handles.lock().await;
        if self.refreshes_closed() {
            return Err(TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Synchronization host has stopped.",
            ));
        }
        if let Some(handle) = installed.get(&verified.user_id) {
            return Ok(handle.clone());
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
        self.service
            .install_sync_handle(verified.user_id.clone(), handle.clone());
        self.observe_sync(&verified.user_id, &handle);
        installed.insert(verified.user_id, handle.clone());
        Ok(handle)
    }

    /// Publish an owned remote Computer using G's authenticated SDK. Its local
    /// directory row came from H; it is never re-registered or given a fake lease.
    pub async fn connect_computer(
        &self,
        actor: &RequestIdentity,
        computer_id: &str,
    ) -> TalosResult<()> {
        let token = actor.access_token.as_deref().ok_or_else(|| {
            TalosError::new(
                ErrorCode::Unauthenticated,
                "Computer access requires a signed-in account.",
            )
        })?;
        let verified = self.auth_service.verify_access_token(token).await?;
        if verified.auth_disabled || verified.user_id != actor.user_id {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "Computer account does not match.",
            ));
        }
        let config = self.config.reload()?;
        let local = config
            .agent_node
            .runner
            .as_ref()
            .is_some_and(|runner| runner.node_id == computer_id);
        let mut connections = self.sync_stores.connections.lock().await;
        if connections
            .get(computer_id)
            .filter(|(owner, _)| owner == &actor.user_id)
            .and_then(|(_, connection)| connection.upgrade())
            .is_some_and(|connection| !connection.is_closed())
        {
            return Ok(());
        }
        let trust = self
            .config
            .cloud
            .agent_trusted_ca_file
            .as_ref()
            .map(std::fs::read)
            .transpose()
            .map_err(|error| {
                TalosError::internal_with_source(
                    "Read Agent trust certificate; technical details were withheld.",
                    error,
                )
            })?
            .map_or(
                agent_client::AgentTlsTrust::SystemRoots,
                agent_client::AgentTlsTrust::Certificate,
            );
        let gateway =
            agent_client::AgentNodeClient::connect(&self.config.cloud.agent_base_url, trust)
                .await?;
        let deployments = gateway.get_deployments(token, computer_id).await?;
        let (connection, auth) = gateway
            .connect_coordinator_with_resources(
                token,
                self.sync.peer_id(),
                computer_id,
                self.coordinator_resource_handler(computer_id.into()),
            )
            .await?;
        if !local {
            for deployment in deployments {
                let harness = agent_client::RemoteAgentClient::from_connection(
                    connection.clone(),
                    deployment.agent_id.clone(),
                    auth.clone(),
                )
                .with_coordinator_id(self.sync.peer_id().into())?;
                self.coordinator
                    .replace_remote_agent(
                        computer_id.into(),
                        deployment.agent_id,
                        Arc::new(harness),
                    )
                    .await?;
            }
            self.coordinator
                .register_environment(
                    computer_id.into(),
                    Arc::new(agent_client::AgentEnvironmentClient::from_connection(
                        connection.clone(),
                        auth,
                    )),
                )
                .await;
        }
        connections.insert(
            computer_id.into(),
            (actor.user_id.clone(), Arc::downgrade(&connection)),
        );
        Ok(())
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

impl ApplicationSyncStores {
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
        for connection in self
            .connections
            .lock()
            .await
            .drain()
            .filter_map(|(_, (_, connection))| connection.upgrade())
        {
            connection.close();
        }
        self.credentials
            .write()
            .expect("Sync credentials lock")
            .clear();
        self.shutdown_handles().await
    }
}
