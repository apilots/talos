//! In-process SDK adapter for the product-managed application runtime.
//!
//! The host composes the runtime with `talos_base::run_embedded_app_server` and
//! keeps its logging guard alive. The bundled Browser MCP launches the host's
//! current executable with `--browser-mcp`; before ordinary argument parsing,
//! authentication, logging or runtime startup, the host must dispatch that
//! argument to `talos_base::run_browser_mcp` and return when it finishes.
//! This hidden stdio entrypoint negotiates MCP without an account or model and
//! leaves browser execution and authorization with the existing Worker owner.
//! `EmbeddedServerClient` only adapts an already composed runtime; it cannot
//! install executable entrypoints on behalf of an embedding application.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use talos_auth::{
    CloudLoginCancelRequest, CloudLoginPersistence, CloudLoginStartRequest,
    CloudLoginStartResponse, CloudLoginStatusRequest, CloudLoginStatusResponse,
};
use talos_base::TalosApplicationRuntime;
use talos_error::TalosResult;
use talos_sdk::{
    AutomationChangeStream, GatewayClient, InterruptTurnRequest, InterruptTurnResponse,
    MessageQueueChangeStream, RecoverTimelineResponse, SessionActiveTurnResponse,
    SessionHistoryResponse, SubmitTurnInputRequest, SubmitTurnInputResponse, TimelineEventStream,
    TimelineStreamSubscription,
};
use talos_service::{Actor, TimelineSubscriptionMessage};
use talos_types::{
    AuthLogoutRequest, AuthLogoutResponse, AuthSessionClearRequest, AuthSessionClearResponse,
    AuthSessionReadRequest, AuthSessionReadResponse, CloudLoginCancelResponse, ConfigDeleteRequest,
    ConfigDeleteResponse, ConfigGetRequest, ConfigReloadRequest, ConfigReloadResponse,
    ConfigResponse, ConfigUpdateRequest, EventUnsubscribeRequest, EventUnsubscribeResponse,
    FileDeleteRequest, FileDeleteResponse, FileListRequest, FileListResponse, FileReadRequest,
    FileReadResponse, FileUpdateRequest, FileUpdateResponse, FileUploadRequest, FileUploadResponse,
    FsDirectoryCreateRequest, FsDirectoryCreateResponse, FsDirectoryReadRequest,
    FsDirectoryReadResponse, MemoryFileListRequest, MemoryFileListResponse, MemoryFileReadRequest,
    MemoryFileReadResponse, MemoryFileWriteRequest, MemoryFileWriteResponse, MemoryGetRequest,
    MemoryGetResponse, MemorySearchRequest, MemorySearchResponse, MessageQueue,
    MessageQueueAddRequest, MessageQueueDeleteRequest, MessageQueueListRequest,
    MessageQueueReorderRequest, MessageQueueStartRequest, MessageQueueUpdateRequest,
    ModelListRequest, ModelListResponse, PluginDisableRequest, PluginDisableResponse,
    PluginEnableRequest, PluginEnableResponse, PluginInstallRequest, PluginInstallResponse,
    PluginListRequest, PluginListResponse, PluginReadRequest, PluginReadResponse,
    PluginSearchRequest, PluginSearchResponse, PluginSourceAddRequest, PluginSourceAddResponse,
    PluginSourceListRequest, PluginSourceListResponse, PluginSourceRefreshRequest,
    PluginSourceRefreshResponse, PluginSourceRemoveRequest, PluginSourceRemoveResponse,
    PluginUninstallRequest, PluginUninstallResponse, PluginUpdateRequest, PluginUpdateResponse,
    ProfileReadRequest, ProfileReadResponse, ProjectCreateRequest, ProjectCreateResponse,
    ProjectDeleteRequest, ProjectDeleteResponse, ProjectListRequest, ProjectListResponse,
    ProjectPatchRequest, ProjectPatchResponse, ProjectReadRequest, ProjectReadResponse,
    RecoverTimelineRequest, RetryTurnRequest, SessionActiveTurnRequest, SessionClearRequest,
    SessionClearResponse, SessionCloseRequest, SessionCloseResponse, SessionCompactRequest,
    SessionCompactResponse, SessionDeleteRequest, SessionDeleteResponse, SessionGetRequest,
    SessionGetResponse, SessionHistoryRequest, SessionListRequest, SessionListResponse,
    SessionPatchRequest, SessionUpdateResponse, SkillsReadResponse, StartTurnRequest,
    StartTurnResponse, SubscribeTimelineRequest, SubscribeTimelineResponse, UserCurrentRequest,
    UserCurrentResponse, WorkspaceDocListRequest, WorkspaceDocListResponse,
    WorkspaceDocReadRequest, WorkspaceDocReadResponse, WorkspaceDocWriteRequest,
    WorkspaceDocWriteResponse, WorkspaceInfoRequest, WorkspaceInfoResponse,
};
use tokio::{sync::OnceCell, task::JoinHandle};

mod request_context;
use request_context::{
    authenticated_actor_from_access_token, prepare_embedded_start_turn, resolve_cached_actor,
};

type Subscriptions = HashMap<String, JoinHandle<()>>;

struct EmbeddedRuntime {
    product: talos_base::TalosProductRuntime,
    execution: Option<Arc<TalosApplicationRuntime>>,
}
impl std::ops::Deref for EmbeddedRuntime {
    type Target = talos_base::TalosProductRuntime;
    fn deref(&self) -> &Self::Target {
        &self.product
    }
}
impl EmbeddedRuntime {
    fn from_full(runtime: TalosApplicationRuntime) -> Self {
        let product = talos_base::TalosProductRuntime {
            config: runtime.config.clone(),
            service: runtime.service.clone(),
            event_service: runtime.event_service.clone(),
            auth_service: runtime.auth_service.clone(),
            gateway: runtime.gateway.clone(),
        };
        Self {
            product,
            execution: Some(Arc::new(runtime)),
        }
    }
    fn resume_account_host(&self, actor: &Actor) {
        if self.execution.is_some() {
            self.service.resume_account_host(actor);
        }
    }
    async fn shutdown(&self) -> TalosResult<()> {
        match &self.execution {
            Some(runtime) => runtime.shutdown().await,
            None => Ok(()),
        }
    }
}

pub struct EmbeddedServerClient {
    remote: Option<Arc<tokio::sync::Mutex<talos_sdk::AppServerClient>>>,
    selected_computer: Option<String>,
    runtime: Arc<EmbeddedRuntime>,
    actor: Actor,
    subscriptions: Arc<Mutex<Option<Subscriptions>>>,
    shutdown_result: Arc<OnceCell<TalosResult<()>>>,
}

impl EmbeddedServerClient {
    pub async fn from_runtime(runtime: TalosApplicationRuntime) -> TalosResult<Self> {
        let auth_enabled = runtime.config.gateway.auth;
        Self::from_runtime_with_actor(
            EmbeddedRuntime::from_full(runtime),
            if auth_enabled {
                Actor::regular("local")
            } else {
                Actor::auth_disabled()
            },
            auth_enabled,
            /*actor_overridden*/ false,
        )
        .await
    }

    /// Reuse the existing SDK/Gateway for definition-only management without a Worker host.
    pub async fn from_product_runtime(
        runtime: talos_base::TalosProductRuntime,
    ) -> TalosResult<Self> {
        let auth_enabled = runtime.config.gateway.auth;
        Self::from_runtime_with_actor(
            EmbeddedRuntime {
                product: runtime,
                execution: None,
            },
            if auth_enabled {
                Actor::regular("local")
            } else {
                Actor::auth_disabled()
            },
            auth_enabled,
            false,
        )
        .await
    }

    async fn from_runtime_with_actor(
        runtime: EmbeddedRuntime,
        actor: Actor,
        auth_enabled: bool,
        actor_overridden: bool,
    ) -> TalosResult<Self> {
        let actor = if auth_enabled && !actor_overridden {
            resolve_cached_actor(&runtime).await?.unwrap_or(actor)
        } else {
            actor
        };
        if !actor.auth_disabled && actor.access_token.is_some() {
            runtime.resume_account_host(&actor);
        }
        Ok(Self {
            remote: None,
            selected_computer: None,
            runtime: Arc::new(runtime),
            actor,
            subscriptions: Arc::new(Mutex::new(Some(HashMap::new()))),
            shutdown_result: Arc::new(OnceCell::new()),
        })
    }

    fn control_plane_client(&self) -> TalosResult<Option<talos_sdk::AppServerClient>> {
        let Some(token) = self.actor.access_token.clone() else {
            return Ok(None);
        };
        let config = self.runtime.config.reload()?;
        let ca =
            talos_service::read_worker_trusted_ca(config.cloud.agent_trusted_ca_file.as_deref())?;
        talos_sdk::AppServerClient::connect_product_http(
            &config.cloud.agent_base_url,
            token,
            "embedded".into(),
            ca.as_deref(),
        )
        .map(Some)
    }

    /// Select a remote product through Talos Server without copying its state
    /// into the local runtime. Embedded or this machine's registration selects local.
    pub fn select_computer(&mut self, computer_id: &str) -> TalosResult<()> {
        let config = self.runtime.config.reload()?;
        let local = computer_id == "embedded"
            || config
                .agent_node
                .runner
                .as_ref()
                .is_some_and(|runner| runner.node_id == computer_id);
        if local {
            self.remote = None;
            self.selected_computer = None;
            return Ok(());
        }
        if self.selected_computer.as_deref() == Some(computer_id) {
            return Ok(());
        }
        let token = self.actor.access_token.clone().ok_or_else(|| {
            talos_error::TalosError::new(
                talos_error::ErrorCode::Unauthenticated,
                "Sign in before selecting a remote Computer.",
            )
        })?;
        let ca =
            talos_service::read_worker_trusted_ca(config.cloud.agent_trusted_ca_file.as_deref())?;
        let client = talos_sdk::AppServerClient::connect_product_http(
            &config.cloud.agent_base_url,
            token,
            computer_id.into(),
            ca.as_deref(),
        )?;
        self.remote = Some(Arc::new(tokio::sync::Mutex::new(client)));
        self.selected_computer = Some(computer_id.into());
        Ok(())
    }

    /// Take an independent request handle with the current authenticated identity.
    /// Authenticate the owner before creating handles. Subscriptions and shutdown
    /// are shared with the owner and all handles for this runtime.
    pub fn request_handle(&self) -> Self {
        Self {
            remote: self.remote.clone(),
            selected_computer: self.selected_computer.clone(),
            runtime: self.runtime.clone(),
            actor: self.actor.clone(),
            subscriptions: self.subscriptions.clone(),
            shutdown_result: self.shutdown_result.clone(),
        }
    }
}

mod gateway;

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
