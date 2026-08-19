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
    GatewayClient, MessageQueueChangeStream, TimelineEventStream, TimelineStreamSubscription,
};
use talos_service::{Actor, TimelineSubscriptionMessage};
use talos_types::{
    AuthLogoutRequest, AuthLogoutResponse, AuthSessionClearRequest, AuthSessionClearResponse,
    AuthSessionReadRequest, AuthSessionReadResponse, CloudLoginCancelResponse, ConfigDeleteRequest,
    ConfigDeleteResponse, ConfigGetRequest, ConfigGetResponse, ConfigReloadRequest,
    ConfigReloadResponse, ConfigUpdateRequest, ConfigUpdateResponse, EventUnsubscribeRequest,
    EventUnsubscribeResponse, FileDeleteRequest, FileDeleteResponse, FileListRequest,
    FileListResponse, FileReadRequest, FileReadResponse, FileUpdateRequest, FileUpdateResponse,
    FileUploadRequest, FileUploadResponse, FsDirectoryCreateRequest, FsDirectoryCreateResponse,
    FsDirectoryReadRequest, FsDirectoryReadResponse, InterruptTurnRequest, InterruptTurnResponse,
    MemoryFileListRequest, MemoryFileListResponse, MemoryFileReadRequest, MemoryFileReadResponse,
    MemoryFileWriteRequest, MemoryFileWriteResponse, MemoryGetRequest, MemoryGetResponse,
    MemorySearchRequest, MemorySearchResponse, MessageQueue, MessageQueueAddRequest,
    MessageQueueDeleteRequest, MessageQueueListRequest, MessageQueueReorderRequest,
    MessageQueueStartRequest, MessageQueueSteerRequest, MessageQueueUpdateRequest,
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
    RecoverTimelineRequest, RetryTurnRequest, RetryTurnResponse, SessionActiveTurnRequest,
    SessionActiveTurnResponse, SessionClearRequest, SessionClearResponse, SessionCloseRequest,
    SessionCloseResponse, SessionCompactRequest, SessionCompactResponse, SessionDeleteRequest,
    SessionDeleteResponse, SessionGetRequest, SessionGetResponse, SessionHistoryRequest,
    SessionHistoryResponse, SessionListRequest, SessionListResponse, SessionPatchRequest,
    SessionUpdateResponse, SkillsReadResponse, StartTurnRequest, StartTurnResponse,
    SubmitDynamicToolResultRequest, SubmitDynamicToolResultResponse, SubmitUserActionRequest,
    SubmitUserActionResponse, SubscribeTimelineRequest, SubscribeTimelineResponse,
    TimelineRecoveryResponse, UserCurrentRequest, UserCurrentResponse, WorkspaceDocListRequest,
    WorkspaceDocListResponse, WorkspaceDocReadRequest, WorkspaceDocReadResponse,
    WorkspaceDocWriteRequest, WorkspaceDocWriteResponse, WorkspaceInfoRequest,
    WorkspaceInfoResponse,
};
use tokio::task::JoinHandle;

mod request_context;
use request_context::{
    authenticated_actor_from_access_token, normalize_embedded_start_turn_request,
    resolve_cached_actor,
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
            Actor::regular("local"),
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
        Ok(Self {
            runtime,
            actor,
            subscriptions: HashMap::new(),
        })
    }
}

#[async_trait]
impl GatewayClient for EmbeddedServerClient {
    async fn read_auth_session(
        &mut self,
        request: AuthSessionReadRequest,
    ) -> TalosResult<AuthSessionReadResponse> {
        self.runtime
            .gateway
            .read_auth_session(&self.actor, request)
            .await
    }

    async fn clear_auth_session(
        &mut self,
        request: AuthSessionClearRequest,
    ) -> TalosResult<AuthSessionClearResponse> {
        self.runtime
            .gateway
            .clear_auth_session(&self.actor, request)
            .await
    }

    async fn auth_logout(&mut self, request: AuthLogoutRequest) -> TalosResult<AuthLogoutResponse> {
        self.runtime.gateway.logout(&self.actor, request).await
    }

    async fn start_cloud_login(
        &mut self,
        request: CloudLoginStartRequest,
    ) -> TalosResult<CloudLoginStartResponse> {
        self.runtime
            .gateway
            .start_cloud_login(&self.actor, request, CloudLoginPersistence::LocalCache)
            .await
    }

    async fn cloud_login_status(
        &mut self,
        request: CloudLoginStatusRequest,
    ) -> TalosResult<CloudLoginStatusResponse> {
        let response = self
            .runtime
            .gateway
            .cloud_login_status(&self.actor, request)
            .await?;
        if let CloudLoginStatusResponse::Authenticated { access_token, .. } = &response {
            self.actor = authenticated_actor_from_access_token(&self.runtime, access_token).await?;
        }
        Ok(response)
    }

    async fn cancel_cloud_login(
        &mut self,
        request: CloudLoginCancelRequest,
    ) -> TalosResult<CloudLoginCancelResponse> {
        self.runtime
            .gateway
            .cancel_cloud_login(&self.actor, request)
            .await
    }

    async fn current_user(
        &mut self,
        request: UserCurrentRequest,
    ) -> TalosResult<UserCurrentResponse> {
        self.runtime
            .gateway
            .current_user(&self.actor, request)
            .await
    }

    async fn read_profile(
        &mut self,
        request: ProfileReadRequest,
    ) -> TalosResult<ProfileReadResponse> {
        self.runtime
            .gateway
            .read_profile(&self.actor, request)
            .await
    }

    async fn list_models(&mut self, request: ModelListRequest) -> TalosResult<ModelListResponse> {
        self.runtime.gateway.list_models(&self.actor, request).await
    }

    async fn get_config(&mut self, request: ConfigGetRequest) -> TalosResult<ConfigGetResponse> {
        self.runtime.gateway.get_config(&self.actor, request).await
    }

    async fn update_config(
        &mut self,
        request: ConfigUpdateRequest,
    ) -> TalosResult<ConfigUpdateResponse> {
        self.runtime
            .gateway
            .update_config(&self.actor, request)
            .await
    }

    async fn delete_config(
        &mut self,
        request: ConfigDeleteRequest,
    ) -> TalosResult<ConfigDeleteResponse> {
        self.runtime
            .gateway
            .delete_config(&self.actor, request)
            .await
    }

    async fn reload_config(
        &mut self,
        request: ConfigReloadRequest,
    ) -> TalosResult<ConfigReloadResponse> {
        self.runtime
            .gateway
            .reload_config(&self.actor, request)
            .await
    }

    async fn list_skills(
        &mut self,
        request: talos_sdk::SkillsListRequest,
    ) -> TalosResult<talos_sdk::SkillsListResponse> {
        self.runtime.gateway.list_skills(&self.actor, request).await
    }

    async fn refresh_skills(
        &mut self,
        request: talos_sdk::SkillsRefreshRequest,
    ) -> TalosResult<talos_sdk::SkillsListResponse> {
        self.runtime
            .gateway
            .refresh_skills(&self.actor, request)
            .await
    }

    async fn suggest_skills(
        &mut self,
        request: talos_sdk::SkillsSuggestRequest,
    ) -> TalosResult<talos_sdk::SkillsSuggestResponse> {
        self.runtime
            .gateway
            .suggest_skills(&self.actor, request)
            .await
    }

    async fn read_skill(
        &mut self,
        request: talos_sdk::SkillsReadRequest,
    ) -> TalosResult<SkillsReadResponse> {
        self.runtime.gateway.read_skill(&self.actor, request).await
    }

    async fn resolve_skills(
        &mut self,
        request: talos_sdk::SkillsResolveRequest,
    ) -> TalosResult<talos_sdk::SkillsResolveResponse> {
        self.runtime
            .gateway
            .resolve_skills(&self.actor, request)
            .await
    }

    async fn list_sessions(
        &mut self,
        request: SessionListRequest,
    ) -> TalosResult<SessionListResponse> {
        self.runtime
            .gateway
            .list_sessions(&self.actor, request)
            .await
    }

    async fn get_session(&mut self, request: SessionGetRequest) -> TalosResult<SessionGetResponse> {
        self.runtime.gateway.get_session(&self.actor, request).await
    }

    async fn get_session_history(
        &mut self,
        request: SessionHistoryRequest,
    ) -> TalosResult<SessionHistoryResponse> {
        self.runtime
            .gateway
            .get_session_history(&self.actor, request)
            .await
    }

    async fn get_active_turn(
        &mut self,
        request: SessionActiveTurnRequest,
    ) -> TalosResult<SessionActiveTurnResponse> {
        self.runtime
            .gateway
            .get_active_turn(&self.actor, request)
            .await
    }

    async fn patch_session(
        &mut self,
        request: SessionPatchRequest,
    ) -> TalosResult<SessionUpdateResponse> {
        self.runtime
            .gateway
            .patch_session(&self.actor, request)
            .await
    }

    async fn close_session(
        &mut self,
        request: SessionCloseRequest,
    ) -> TalosResult<SessionCloseResponse> {
        self.runtime
            .gateway
            .close_session(&self.actor, request)
            .await
    }

    async fn delete_session(
        &mut self,
        request: SessionDeleteRequest,
    ) -> TalosResult<SessionDeleteResponse> {
        self.runtime
            .gateway
            .delete_session(&self.actor, request)
            .await
    }

    async fn compact_session(
        &mut self,
        request: SessionCompactRequest,
    ) -> TalosResult<SessionCompactResponse> {
        self.runtime
            .gateway
            .compact_session(&self.actor, request)
            .await
    }

    async fn clear_session(
        &mut self,
        request: SessionClearRequest,
    ) -> TalosResult<SessionClearResponse> {
        self.runtime
            .gateway
            .clear_session(&self.actor, request)
            .await
    }

    async fn recover_timeline(
        &mut self,
        request: RecoverTimelineRequest,
    ) -> TalosResult<TimelineRecoveryResponse> {
        self.runtime
            .gateway
            .recover_timeline(&self.actor, request)
            .await
    }

    async fn subscribe_timeline(
        &mut self,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<SubscribeTimelineResponse> {
        self.runtime
            .gateway
            .subscribe_timeline(&self.actor, request)
            .await
    }

    async fn open_timeline_stream(
        &mut self,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<TimelineStreamSubscription> {
        let source = self
            .runtime
            .service
            .timeline_subscription_source(&self.actor, request)
            .await?;
        let response = source.response;
        let subscription_id = response.subscription_id.clone();
        let mut events = source.subscription.events;
        let (tx, rx) = tokio::sync::mpsc::channel(256);
        let (message_queue_tx, message_queue_rx) = tokio::sync::mpsc::channel(256);
        let task = tokio::spawn(async move {
            while let Some(message) = events.recv().await {
                match message {
                    TimelineSubscriptionMessage::Event(event) => {
                        if tx.send(event).await.is_err() {
                            return;
                        }
                    }
                    TimelineSubscriptionMessage::MessageQueueChanged(change) => {
                        if message_queue_tx.send(change).await.is_err() {
                            return;
                        }
                    }
                    TimelineSubscriptionMessage::Closed(_) => return,
                }
            }
        });
        if let Some(previous) = self.subscriptions.insert(subscription_id, task) {
            previous.abort();
            let _ = previous.await;
        }
        Ok(TimelineStreamSubscription {
            response,
            stream: TimelineEventStream::new(rx),
            message_queue_changes: MessageQueueChangeStream::new(message_queue_rx),
        })
    }

    async fn unsubscribe_timeline(
        &mut self,
        request: EventUnsubscribeRequest,
    ) -> TalosResult<EventUnsubscribeResponse> {
        let unsubscribed = if let Some(task) = self.subscriptions.remove(&request.subscription_id) {
            task.abort();
            let _ = task.await;
            true
        } else {
            false
        };
        Ok(EventUnsubscribeResponse {
            subscription_id: request.subscription_id,
            unsubscribed,
        })
    }

    async fn start_turn(&mut self, request: StartTurnRequest) -> TalosResult<StartTurnResponse> {
        let request = normalize_embedded_start_turn_request(request)?;
        let idempotency_key = request.client_request_id.clone();
        self.runtime
            .gateway
            .start_turn(&self.actor, request, idempotency_key)
            .await
    }

    async fn retry_turn(&mut self, request: RetryTurnRequest) -> TalosResult<RetryTurnResponse> {
        self.runtime.gateway.retry_turn(&self.actor, request).await
    }

    async fn interrupt_turn(
        &mut self,
        request: InterruptTurnRequest,
    ) -> TalosResult<InterruptTurnResponse> {
        self.runtime
            .gateway
            .interrupt_turn(&self.actor, request)
            .await
    }

    async fn add_queued_message(
        &mut self,
        request: MessageQueueAddRequest,
    ) -> TalosResult<MessageQueue> {
        self.runtime
            .gateway
            .add_queued_message(&self.actor, request)
            .await
    }

    async fn list_message_queue(
        &mut self,
        request: MessageQueueListRequest,
    ) -> TalosResult<MessageQueue> {
        self.runtime
            .gateway
            .list_message_queue(&self.actor, request)
            .await
    }

    async fn update_queued_message(
        &mut self,
        request: MessageQueueUpdateRequest,
    ) -> TalosResult<MessageQueue> {
        self.runtime
            .gateway
            .update_queued_message(&self.actor, request)
            .await
    }

    async fn delete_queued_message(
        &mut self,
        request: MessageQueueDeleteRequest,
    ) -> TalosResult<MessageQueue> {
        self.runtime
            .gateway
            .delete_queued_message(&self.actor, request)
            .await
    }

    async fn reorder_message_queue(
        &mut self,
        request: MessageQueueReorderRequest,
    ) -> TalosResult<MessageQueue> {
        self.runtime
            .gateway
            .reorder_message_queue(&self.actor, request)
            .await
    }

    async fn start_queued_message(
        &mut self,
        request: MessageQueueStartRequest,
    ) -> TalosResult<StartTurnResponse> {
        self.runtime
            .gateway
            .start_queued_message(&self.actor, request)
            .await
    }

    async fn steer_queued_message(
        &mut self,
        request: MessageQueueSteerRequest,
    ) -> TalosResult<MessageQueue> {
        self.runtime
            .gateway
            .steer_queued_message(&self.actor, request)
            .await
    }

    async fn submit_dynamic_tool_result(
        &mut self,
        request: SubmitDynamicToolResultRequest,
    ) -> TalosResult<SubmitDynamicToolResultResponse> {
        self.runtime
            .gateway
            .submit_dynamic_tool_result(&self.actor, request)
            .await
    }

    async fn submit_user_action(
        &mut self,
        request: SubmitUserActionRequest,
    ) -> TalosResult<SubmitUserActionResponse> {
        self.runtime
            .gateway
            .submit_user_action(&self.actor, request)
            .await
    }

    async fn list_projects(
        &mut self,
        request: ProjectListRequest,
    ) -> TalosResult<ProjectListResponse> {
        self.runtime
            .gateway
            .list_projects(&self.actor, request)
            .await
    }

    async fn get_project(
        &mut self,
        request: ProjectReadRequest,
    ) -> TalosResult<ProjectReadResponse> {
        self.runtime.gateway.get_project(&self.actor, request).await
    }

    async fn create_project(
        &mut self,
        request: ProjectCreateRequest,
    ) -> TalosResult<ProjectCreateResponse> {
        self.runtime
            .gateway
            .create_project(&self.actor, request)
            .await
    }

    async fn patch_project(
        &mut self,
        request: ProjectPatchRequest,
    ) -> TalosResult<ProjectPatchResponse> {
        self.runtime
            .gateway
            .patch_project(&self.actor, request)
            .await
    }

    async fn delete_project(
        &mut self,
        request: ProjectDeleteRequest,
    ) -> TalosResult<ProjectDeleteResponse> {
        self.runtime
            .gateway
            .delete_project(&self.actor, request)
            .await
    }

    async fn read_fs_directory(
        &mut self,
        request: FsDirectoryReadRequest,
    ) -> TalosResult<FsDirectoryReadResponse> {
        self.runtime
            .gateway
            .read_fs_directory(&self.actor, request)
            .await
    }

    async fn create_fs_directory(
        &mut self,
        request: FsDirectoryCreateRequest,
    ) -> TalosResult<FsDirectoryCreateResponse> {
        self.runtime
            .gateway
            .create_fs_directory(&self.actor, request)
            .await
    }

    async fn list_plugin_sources(
        &mut self,
        request: PluginSourceListRequest,
    ) -> TalosResult<PluginSourceListResponse> {
        self.runtime
            .gateway
            .list_plugin_sources(&self.actor, request)
            .await
    }

    async fn add_plugin_source(
        &mut self,
        request: PluginSourceAddRequest,
    ) -> TalosResult<PluginSourceAddResponse> {
        self.runtime
            .gateway
            .add_plugin_source(&self.actor, request)
            .await
    }

    async fn refresh_plugin_source(
        &mut self,
        request: PluginSourceRefreshRequest,
    ) -> TalosResult<PluginSourceRefreshResponse> {
        self.runtime
            .gateway
            .refresh_plugin_source(&self.actor, request)
            .await
    }

    async fn remove_plugin_source(
        &mut self,
        request: PluginSourceRemoveRequest,
    ) -> TalosResult<PluginSourceRemoveResponse> {
        self.runtime
            .gateway
            .remove_plugin_source(&self.actor, request)
            .await
    }

    async fn search_plugins(
        &mut self,
        request: PluginSearchRequest,
    ) -> TalosResult<PluginSearchResponse> {
        self.runtime
            .gateway
            .search_plugins(&self.actor, request)
            .await
    }

    async fn list_plugins(
        &mut self,
        request: PluginListRequest,
    ) -> TalosResult<PluginListResponse> {
        self.runtime
            .gateway
            .list_plugins(&self.actor, request)
            .await
    }

    async fn read_plugin(&mut self, request: PluginReadRequest) -> TalosResult<PluginReadResponse> {
        self.runtime.gateway.read_plugin(&self.actor, request).await
    }

    async fn install_plugin(
        &mut self,
        request: PluginInstallRequest,
    ) -> TalosResult<PluginInstallResponse> {
        self.runtime
            .gateway
            .install_plugin(&self.actor, request)
            .await
    }

    async fn update_plugin(
        &mut self,
        request: PluginUpdateRequest,
    ) -> TalosResult<PluginUpdateResponse> {
        self.runtime
            .gateway
            .update_plugin(&self.actor, request)
            .await
    }

    async fn uninstall_plugin(
        &mut self,
        request: PluginUninstallRequest,
    ) -> TalosResult<PluginUninstallResponse> {
        self.runtime
            .gateway
            .uninstall_plugin(&self.actor, request)
            .await
    }

    async fn enable_plugin(
        &mut self,
        request: PluginEnableRequest,
    ) -> TalosResult<PluginEnableResponse> {
        self.runtime
            .gateway
            .enable_plugin(&self.actor, request)
            .await
    }

    async fn disable_plugin(
        &mut self,
        request: PluginDisableRequest,
    ) -> TalosResult<PluginDisableResponse> {
        self.runtime
            .gateway
            .disable_plugin(&self.actor, request)
            .await
    }

    async fn upload_file(
        &mut self,
        request: FileUploadRequest,
        data: Bytes,
    ) -> TalosResult<FileUploadResponse> {
        self.runtime
            .gateway
            .upload_file(
                &self.actor,
                request,
                Box::pin(futures_util::stream::iter([Ok(data)])),
            )
            .await
    }

    async fn list_files(&mut self, request: FileListRequest) -> TalosResult<FileListResponse> {
        self.runtime.gateway.list_files(&self.actor, request).await
    }

    async fn read_file(&mut self, request: FileReadRequest) -> TalosResult<FileReadResponse> {
        self.runtime.gateway.read_file(&self.actor, request).await
    }

    async fn update_file(&mut self, request: FileUpdateRequest) -> TalosResult<FileUpdateResponse> {
        self.runtime.gateway.update_file(&self.actor, request).await
    }

    async fn delete_file(&mut self, request: FileDeleteRequest) -> TalosResult<FileDeleteResponse> {
        self.runtime.gateway.delete_file(&self.actor, request).await
    }

    async fn list_memory_files(
        &mut self,
        request: MemoryFileListRequest,
    ) -> TalosResult<MemoryFileListResponse> {
        self.runtime
            .gateway
            .list_memory_files(&self.actor, request)
            .await
    }

    async fn read_memory_file(
        &mut self,
        request: MemoryFileReadRequest,
    ) -> TalosResult<MemoryFileReadResponse> {
        self.runtime
            .gateway
            .read_memory_file(&self.actor, request)
            .await
    }

    async fn write_memory_file(
        &mut self,
        request: MemoryFileWriteRequest,
    ) -> TalosResult<MemoryFileWriteResponse> {
        self.runtime
            .gateway
            .write_memory_file(&self.actor, request)
            .await
    }

    async fn search_memory(
        &mut self,
        request: MemorySearchRequest,
    ) -> TalosResult<MemorySearchResponse> {
        self.runtime
            .gateway
            .search_memory(&self.actor, request)
            .await
    }

    async fn get_memory(&mut self, request: MemoryGetRequest) -> TalosResult<MemoryGetResponse> {
        self.runtime.gateway.get_memory(&self.actor, request).await
    }

    async fn list_workspace_docs(
        &mut self,
        request: WorkspaceDocListRequest,
    ) -> TalosResult<WorkspaceDocListResponse> {
        self.runtime
            .gateway
            .list_workspace_docs(&self.actor, request)
            .await
    }

    async fn read_workspace_doc(
        &mut self,
        request: WorkspaceDocReadRequest,
    ) -> TalosResult<WorkspaceDocReadResponse> {
        self.runtime
            .gateway
            .read_workspace_doc(&self.actor, request)
            .await
    }

    async fn write_workspace_doc(
        &mut self,
        request: WorkspaceDocWriteRequest,
    ) -> TalosResult<WorkspaceDocWriteResponse> {
        self.runtime
            .gateway
            .write_workspace_doc(&self.actor, request)
            .await
    }

    async fn workspace_info(
        &mut self,
        request: WorkspaceInfoRequest,
    ) -> TalosResult<WorkspaceInfoResponse> {
        self.runtime
            .gateway
            .workspace_info(&self.actor, request)
            .await
    }

    async fn shutdown(&mut self) -> TalosResult<()> {
        for (_, task) in self.subscriptions.drain() {
            task.abort();
            let _ = task.await;
        }
        self.runtime.shutdown().await?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
