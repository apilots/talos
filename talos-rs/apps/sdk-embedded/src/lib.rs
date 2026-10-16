use std::collections::HashMap;

use async_trait::async_trait;
use bytes::Bytes;
use talos_auth::{
    CloudLoginCancelRequest, CloudLoginPersistence, CloudLoginStartRequest,
    CloudLoginStartResponse, CloudLoginStatusRequest, CloudLoginStatusResponse,
};
use talos_base::TalosApplicationRuntime;
use talos_error::TalosResult;
use talos_sdk::{
    GatewayClient, InterruptTurnRequest, InterruptTurnResponse, MessageQueueChangeStream,
    RecoverTimelineResponse, SessionActiveTurnResponse, SessionHistoryResponse,
    SubmitTurnInputRequest, SubmitTurnInputResponse, TimelineEventStream,
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
use tokio::task::JoinHandle;

mod request_context;
use request_context::{
    authenticated_actor_from_access_token, prepare_embedded_start_turn, resolve_cached_actor,
};

pub struct EmbeddedServerClient {
    runtime: TalosApplicationRuntime,
    actor: Actor,
    subscriptions: HashMap<String, JoinHandle<()>>,
}

impl EmbeddedServerClient {
    pub async fn from_runtime(runtime: TalosApplicationRuntime) -> TalosResult<Self> {
        let auth_enabled = runtime.config.gateway.auth;
        Self::from_runtime_with_actor(
            runtime,
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

    async fn from_runtime_with_actor(
        runtime: TalosApplicationRuntime,
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
            runtime.service.sync_status(&actor).await?;
        }
        Ok(Self {
            runtime,
            actor,
            subscriptions: HashMap::new(),
        })
    }
}

mod gateway;

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
