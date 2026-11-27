//! Host composition over original Stores, authenticated SDKs and notifications.
use std::{collections::HashMap, sync::Arc};

use talos_auth::RequestIdentity;
use talos_error::{ErrorCode, TalosError, TalosResult};
#[cfg(feature = "sync")]
use talos_store::{file::DatabaseFileStore, project::StorageProjectStore};
use talos_store::{node::DatabaseAgentNodeStore, session::DatabaseSessionStore};
#[cfg(feature = "sync")]
use talos_sync::SyncHandle;

pub(crate) struct ApplicationStores {
    #[cfg(any(feature = "sync", test))]
    pub storage: Arc<dyn talos_storage::Storage>,
    pub sessions: Arc<DatabaseSessionStore>,
    #[cfg(feature = "sync")]
    pub projects: Arc<StorageProjectStore>,
    pub nodes: Arc<DatabaseAgentNodeStore>,
    #[cfg(feature = "sync")]
    pub files: Arc<DatabaseFileStore>,
    #[cfg(feature = "sync")]
    pub handles: tokio::sync::Mutex<HashMap<String, Arc<SyncHandle>>>,
    #[cfg(feature = "sync")]
    pub observers: std::sync::Mutex<super::application_sync_observer::SyncObservers>,
    #[cfg(feature = "sync")]
    pub credentials: std::sync::RwLock<HashMap<String, talos_sync_sdk::CredentialCallback>>,
}

pub(crate) struct ApplicationHost {
    pub self_weak: std::sync::Weak<Self>,
    pub computer_directory: super::application_computer_directory::ComputerDirectory,
    pub registration_retirement: tokio::sync::Mutex<()>,
    #[cfg(feature = "relay")]
    pub product_router: axum::Router,
    #[cfg(feature = "relay")]
    pub product_relay: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    pub config: talos_config::Config,
    #[cfg(feature = "relay")]
    pub host_id: String,
    pub product_owner: talos_types::NodeProductOwner,
    #[cfg(feature = "sync")]
    pub sync: Arc<talos_sync::TalosSync>,
    pub stores: Arc<ApplicationStores>,
    pub auth_service: Arc<dyn talos_auth::AuthService>,
    pub service: Arc<talos_service::TalosService>,
    #[cfg(feature = "sync")]
    pub event_service: Arc<talos_service::TimelineEventService>,
    pub worker: talos_worker::TalosWorker,
    pub worker_rpc: Arc<talos_worker::WorkerRpcHost>,
    pub worker_registration: Arc<tokio::sync::Mutex<Option<talos_worker::RegisteredWorker>>>,
    pub worker_failures: tokio::sync::watch::Sender<Option<talos_error::ApiError>>,
    pub trusted_local_host: bool,
    pub local_owner: std::sync::RwLock<Option<String>>,
    pub tokens: std::sync::RwLock<
        HashMap<
            String,
            Arc<std::sync::RwLock<super::application_management_host::AccountCredential>>,
        >,
    >,
    pub lifecycle: tokio::sync::Mutex<()>,
    pub background: std::sync::Mutex<super::application_sync_background::RefreshTasks>,
    pub failures: std::sync::RwLock<HashMap<String, talos_error::ApiError>>,
}

impl Drop for ApplicationHost {
    fn drop(&mut self) {
        #[cfg(feature = "relay")]
        if let Some(task) = self.product_relay.get_mut().take() {
            task.abort();
        }
        // Resource callbacks also retain the Stores. Break observer captures here
        // even when the host is dropped without its asynchronous shutdown path.
        #[cfg(feature = "sync")]
        self.stores
            .observers
            .lock()
            .expect("Sync observers lock")
            .drain();
    }
}

impl crate::TalosApplicationRuntime {
    pub async fn shutdown_host(&self) -> TalosResult<()> {
        self.host.shutdown().await
    }

    pub async fn connect_computer(
        &self,
        actor: &RequestIdentity,
        computer_id: &str,
    ) -> TalosResult<()> {
        self.host.connect_computer(actor, computer_id).await
    }
}

impl ApplicationHost {
    #[cfg(feature = "relay")]
    pub(crate) async fn stop_product_relay(&self) {
        if let Some(task) = self.product_relay.lock().await.take() {
            task.abort();
            if let Err(error) = task.await {
                if !error.is_cancelled() {
                    talos_logging::report_error(&TalosError::internal_with_source(
                        "Product relay task failed.",
                        error,
                    ));
                }
            }
        }
    }

    #[cfg(feature = "relay")]
    pub(crate) async fn refresh_product_relay(&self) -> TalosResult<()> {
        if self.product_owner != talos_types::NodeProductOwner::Computer {
            return Ok(());
        }
        let mut task = self.product_relay.lock().await;
        if task.as_ref().is_some_and(|task| !task.is_finished()) {
            return Ok(());
        }
        let Some(leases) = self
            .worker_registration
            .lock()
            .await
            .as_ref()
            .map(|registration| registration.lease_updates())
        else {
            return Ok(());
        };
        let config = self.config.reload()?;
        let Some(runner) = config.agent_node.runner else {
            return Ok(());
        };
        let certificate = talos_service::read_worker_trusted_ca(
            runner
                .trusted_ca_file
                .as_deref()
                .or(config.cloud.agent_trusted_ca_file.as_deref()),
        )?;
        let trust = certificate.map_or(
            agent_client::AgentTlsTrust::SystemRoots,
            agent_client::AgentTlsTrust::Certificate,
        );
        *task = Some(crate::relay_host::spawn(
            config.cloud.agent_base_url,
            trust,
            self.host_id.clone(),
            leases,
            self.product_router.clone(),
        ));
        Ok(())
    }

    pub(crate) async fn shutdown(&self) -> TalosResult<()> {
        self.shutdown_refreshes().await;
        #[cfg(feature = "relay")]
        self.stop_product_relay().await;
        self.tokens.write().expect("management tokens lock").clear();
        #[cfg(feature = "sync")]
        self.stores.shutdown().await?;
        Ok(())
    }

    #[cfg(feature = "sync")]
    pub(crate) async fn install_account_access(
        &self,
        actor: &RequestIdentity,
        credentials: talos_sync_sdk::CredentialCallback,
    ) -> TalosResult<()> {
        self.stores
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
        });
        self.service.files.set_peer_access(peer.clone());
        if self.product_owner == talos_types::NodeProductOwner::Computer {
            self.service.nodes.set_remote_management(peer.clone());
        }
    }

    /// Verify an account-owned Server directory selection without opening an Agent stream.
    /// Product requests for remote computers belong to the Talos Server API.
    pub async fn connect_computer(
        &self,
        actor: &RequestIdentity,
        computer_id: &str,
    ) -> TalosResult<()> {
        let verified = self
            .verified_actor(
                &talos_service::Actor::regular(&actor.user_id)
                    .with_access_token(actor.access_token.clone().unwrap_or_default())
                    .with_role(actor.role)
                    .with_user_type(actor.user_type),
            )
            .await?;
        if !self.account_connections_allowed(&actor.user_id) {
            return Err(account_stopped());
        }
        let token = verified
            .access_token
            .as_deref()
            .expect("Verified account token");
        let started = std::time::Instant::now();
        let node = self
            .gateway_client()
            .await?
            .get_node(token, computer_id)
            .await?;
        if node.node_id != computer_id
            || node.owner_user_id.as_deref() != Some(actor.user_id.as_str())
        {
            return Err(TalosError::authorization_denied());
        }
        self.cache_confirmed_computer(
            &talos_service::Actor::regular(&actor.user_id)
                .with_access_token(actor.access_token.clone().unwrap_or_default())
                .with_role(actor.role)
                .with_user_type(actor.user_type),
            computer_id,
            &node,
            started,
        )
        .await?;
        Ok(())
    }
}

fn account_stopped() -> TalosError {
    TalosError::new(
        ErrorCode::Unauthenticated,
        "The management account is no longer active.",
    )
}
