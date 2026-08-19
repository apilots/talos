use async_trait::async_trait;
use bytes::Bytes;
use talos_error::{TalosError, TalosResult};
use talos_sdk::{
    AuthSessionReadRequest, AuthSessionReadResponse, FileUploadRequest, FileUploadResponse,
    GatewayClient as SdkGatewayClient, InterruptTurnRequest, MessageQueue, MessageQueueAddRequest,
    MessageQueueChangedNotification, MessageQueueDeleteRequest, MessageQueueListRequest,
    MessageQueueReorderRequest, MessageQueueStartRequest, MessageQueueSteerRequest,
    MessageQueueUpdateRequest, ModelListRequest, ModelListResponse, ModelSelection,
    RecoverTimelineRequest, SessionActiveTurnRequest, SessionClearRequest, SessionCloseRequest,
    SessionCompactRequest, SessionConf, SessionGetRequest, SessionHistoryRequest,
    SessionHistorySnapshot, SessionInfo, SessionListRequest, SessionPatchRequest, SkillSuggestion,
    SkillsSuggestRequest, StartTurnRequest, StartTurnResponse, SubmitUserActionRequest,
    SubmitUserActionResponse, SubscribeTimelineRequest, TimelineActiveTurnSnapshot,
    TimelineEventEnvelope, TimelineRecoveryResponse, UserPermissionMode, WorkspaceInfoRequest,
    WorkspaceInfoResponse,
};
use tokio::sync::mpsc;
use tokio::sync::Mutex;

/// Receives live queue invalidations while draining the shared Session stream.
///
/// Dropping the subscription stops the TUI-side forwarding task. Queue state
/// remains owned by the server and must be read through `list_message_queue`.
pub struct MessageQueueSubscription {
    receiver: mpsc::Receiver<MessageQueueChangedNotification>,
    forward_task: Option<tokio::task::JoinHandle<()>>,
}

impl MessageQueueSubscription {
    pub(crate) fn new(
        receiver: mpsc::Receiver<MessageQueueChangedNotification>,
        forward_task: Option<tokio::task::JoinHandle<()>>,
    ) -> Self {
        Self {
            receiver,
            forward_task,
        }
    }

    pub(crate) fn try_recv(
        &mut self,
    ) -> Result<MessageQueueChangedNotification, mpsc::error::TryRecvError> {
        self.receiver.try_recv()
    }
}

impl Drop for MessageQueueSubscription {
    fn drop(&mut self) {
        if let Some(task) = self.forward_task.take() {
            task.abort();
        }
    }
}

#[async_trait]
pub trait GatewayClient: Send + Sync {
    async fn list_models(&self) -> TalosResult<ModelListResponse>;
    async fn read_auth_session(&self) -> TalosResult<AuthSessionReadResponse>;
    async fn workspace_info(&self) -> TalosResult<WorkspaceInfoResponse>;
    async fn suggest_skills(&self, query: &str) -> TalosResult<Vec<SkillSuggestion>>;
    /// Uploads one composer attachment and returns its stable file metadata.
    async fn upload_file(
        &self,
        request: FileUploadRequest,
        data: Bytes,
    ) -> TalosResult<FileUploadResponse>;
    async fn list_sessions(&self) -> TalosResult<Vec<SessionInfo>>;
    async fn get_session(&self, session_id: &str) -> TalosResult<SessionInfo>;
    async fn close_session(&self, session_id: &str) -> TalosResult<()>;
    async fn cancel_active_turn(&self, session_id: &str, turn_id: &str) -> TalosResult<()>;
    /// Adds one message to the server-owned Session queue.
    async fn add_queued_message(
        &self,
        request: MessageQueueAddRequest,
    ) -> TalosResult<MessageQueue>;
    /// Reads the complete server-owned Session queue.
    async fn list_message_queue(
        &self,
        request: MessageQueueListRequest,
    ) -> TalosResult<MessageQueue>;
    /// Replaces one waiting message without changing its identity or position.
    async fn update_queued_message(
        &self,
        request: MessageQueueUpdateRequest,
    ) -> TalosResult<MessageQueue>;
    /// Deletes one waiting message by server-assigned ID.
    async fn delete_queued_message(
        &self,
        request: MessageQueueDeleteRequest,
    ) -> TalosResult<MessageQueue>;
    /// Applies a complete ordered permutation of current message IDs.
    async fn reorder_message_queue(
        &self,
        request: MessageQueueReorderRequest,
    ) -> TalosResult<MessageQueue>;
    /// Starts the queue head or one explicitly selected waiting message.
    async fn start_queued_message(
        &self,
        request: MessageQueueStartRequest,
    ) -> TalosResult<StartTurnResponse>;
    /// Submits one waiting message to the exact active Turn.
    async fn steer_queued_message(
        &self,
        request: MessageQueueSteerRequest,
    ) -> TalosResult<MessageQueue>;
    /// Reuses the persistent Session event stream for queue invalidations.
    async fn subscribe_message_queue_changes(
        &self,
        session_id: &str,
    ) -> TalosResult<MessageQueueSubscription>;
    async fn update_session_model_selection(
        &self,
        session_id: &str,
        model_selection: &ModelSelection,
    ) -> TalosResult<SessionInfo>;
    async fn update_session_permission_mode(
        &self,
        session_id: &str,
        mode: UserPermissionMode,
    ) -> TalosResult<SessionInfo>;
    async fn submit_user_action(
        &self,
        request: SubmitUserActionRequest,
    ) -> TalosResult<SubmitUserActionResponse>;
    async fn compact_session(&self, session_id: &str) -> TalosResult<SessionInfo>;
    async fn clear_session(&self, session_id: &str) -> TalosResult<SessionInfo>;
    async fn get_session_history(&self, session_id: &str) -> TalosResult<SessionHistorySnapshot>;
    async fn get_active_turn(&self, session_id: &str) -> TalosResult<TimelineActiveTurnSnapshot>;
    async fn start_turn_stream(
        &self,
        request: StartTurnRequest,
    ) -> TalosResult<mpsc::Receiver<TimelineEventEnvelope>>;
    async fn reconnect_active_turn_stream(
        &self,
        session_id: &str,
        since_event_id: Option<String>,
    ) -> TalosResult<mpsc::Receiver<TimelineEventEnvelope>>;
    async fn subscribe_session_timeline(
        &self,
        session_id: &str,
        since_event_id: Option<String>,
    ) -> TalosResult<mpsc::Receiver<TimelineEventEnvelope>>;
    async fn recover_timeline(
        &self,
        session_id: &str,
        after_event_id: Option<String>,
    ) -> TalosResult<TimelineRecoveryResponse>;
    /// Shut down the gateway client. After this call, the client must not be reused.
    async fn shutdown(&self) -> TalosResult<()>;
}

#[async_trait]
impl<C> GatewayClient for Mutex<C>
where
    C: SdkGatewayClient + Send,
{
    async fn list_models(&self) -> TalosResult<ModelListResponse> {
        let mut client = self.lock().await;
        SdkGatewayClient::list_models(&mut *client, ModelListRequest {}).await
    }

    async fn read_auth_session(&self) -> TalosResult<AuthSessionReadResponse> {
        let mut client = self.lock().await;
        SdkGatewayClient::read_auth_session(&mut *client, AuthSessionReadRequest {}).await
    }

    async fn workspace_info(&self) -> TalosResult<WorkspaceInfoResponse> {
        let mut client = self.lock().await;
        SdkGatewayClient::workspace_info(&mut *client, WorkspaceInfoRequest {}).await
    }

    async fn suggest_skills(&self, query: &str) -> TalosResult<Vec<SkillSuggestion>> {
        let mut client = self.lock().await;
        SdkGatewayClient::suggest_skills(
            &mut *client,
            SkillsSuggestRequest {
                query: query.to_string(),
                limit: Some(8),
            },
        )
        .await
        .map(|response| response.data)
    }

    async fn upload_file(
        &self,
        request: FileUploadRequest,
        data: Bytes,
    ) -> TalosResult<FileUploadResponse> {
        let mut client = self.lock().await;
        SdkGatewayClient::upload_file(&mut *client, request, data).await
    }

    async fn list_sessions(&self) -> TalosResult<Vec<SessionInfo>> {
        let mut client = self.lock().await;
        SdkGatewayClient::list_sessions(&mut *client, SessionListRequest::default())
            .await
            .map(|response| response.sessions)
    }

    async fn get_session(&self, session_id: &str) -> TalosResult<SessionInfo> {
        let mut client = self.lock().await;
        SdkGatewayClient::get_session(
            &mut *client,
            SessionGetRequest {
                session_id: session_id.to_string(),
            },
        )
        .await
        .map(|response| response.session)
    }

    async fn close_session(&self, session_id: &str) -> TalosResult<()> {
        let mut client = self.lock().await;
        SdkGatewayClient::close_session(
            &mut *client,
            SessionCloseRequest {
                session_id: session_id.to_string(),
            },
        )
        .await
        .map(|_| ())
    }

    async fn cancel_active_turn(&self, session_id: &str, turn_id: &str) -> TalosResult<()> {
        let mut client = self.lock().await;
        SdkGatewayClient::interrupt_turn(
            &mut *client,
            InterruptTurnRequest {
                session_id: session_id.to_string(),
                turn_id: turn_id.to_string(),
            },
        )
        .await
        .map(|_| ())
    }

    async fn add_queued_message(
        &self,
        request: MessageQueueAddRequest,
    ) -> TalosResult<MessageQueue> {
        let mut client = self.lock().await;
        SdkGatewayClient::add_queued_message(&mut *client, request).await
    }

    async fn list_message_queue(
        &self,
        request: MessageQueueListRequest,
    ) -> TalosResult<MessageQueue> {
        let mut client = self.lock().await;
        SdkGatewayClient::list_message_queue(&mut *client, request).await
    }

    async fn update_queued_message(
        &self,
        request: MessageQueueUpdateRequest,
    ) -> TalosResult<MessageQueue> {
        let mut client = self.lock().await;
        SdkGatewayClient::update_queued_message(&mut *client, request).await
    }

    async fn delete_queued_message(
        &self,
        request: MessageQueueDeleteRequest,
    ) -> TalosResult<MessageQueue> {
        let mut client = self.lock().await;
        SdkGatewayClient::delete_queued_message(&mut *client, request).await
    }

    async fn reorder_message_queue(
        &self,
        request: MessageQueueReorderRequest,
    ) -> TalosResult<MessageQueue> {
        let mut client = self.lock().await;
        SdkGatewayClient::reorder_message_queue(&mut *client, request).await
    }

    async fn start_queued_message(
        &self,
        request: MessageQueueStartRequest,
    ) -> TalosResult<StartTurnResponse> {
        let mut client = self.lock().await;
        SdkGatewayClient::start_queued_message(&mut *client, request).await
    }

    async fn steer_queued_message(
        &self,
        request: MessageQueueSteerRequest,
    ) -> TalosResult<MessageQueue> {
        let mut client = self.lock().await;
        SdkGatewayClient::steer_queued_message(&mut *client, request).await
    }

    async fn subscribe_message_queue_changes(
        &self,
        session_id: &str,
    ) -> TalosResult<MessageQueueSubscription> {
        let mut client = self.lock().await;
        let subscription = SdkGatewayClient::open_timeline_stream(
            &mut *client,
            SubscribeTimelineRequest {
                session_id: session_id.to_string(),
                turn_id: None,
                since_event_id: None,
                replay_limit: Some(0),
                close_when_idle: Some(false),
                close_on_terminal: Some(false),
            },
        )
        .await?;
        drop(client);

        let mut timeline = subscription.stream.into_inner();
        let mut changes = subscription.message_queue_changes;
        let (sender, receiver) = mpsc::channel(32);
        let forward_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    event = timeline.recv() => {
                        if event.is_none() {
                            return;
                        }
                    }
                    change = changes.recv() => {
                        let Some(change) = change else {
                            return;
                        };
                        if sender.send(change).await.is_err() {
                            return;
                        }
                    }
                }
            }
        });
        Ok(MessageQueueSubscription::new(receiver, Some(forward_task)))
    }

    async fn update_session_model_selection(
        &self,
        session_id: &str,
        model_selection: &ModelSelection,
    ) -> TalosResult<SessionInfo> {
        let mut client = self.lock().await;
        SdkGatewayClient::patch_session(
            &mut *client,
            SessionPatchRequest {
                session_id: session_id.to_string(),
                conf: Some(SessionConf {
                    model_selection: Some(model_selection.clone()),
                    ..SessionConf::default()
                }),
                project_id: None,
                current_directory: None,
            },
        )
        .await
        .map(|response| response.session)
    }

    async fn update_session_permission_mode(
        &self,
        session_id: &str,
        mode: UserPermissionMode,
    ) -> TalosResult<SessionInfo> {
        let mut client = self.lock().await;
        SdkGatewayClient::patch_session(
            &mut *client,
            SessionPatchRequest {
                session_id: session_id.to_string(),
                conf: Some(SessionConf {
                    user_permission_mode: Some(mode),
                    ..SessionConf::default()
                }),
                project_id: None,
                current_directory: None,
            },
        )
        .await
        .map(|response| response.session)
    }

    async fn submit_user_action(
        &self,
        request: SubmitUserActionRequest,
    ) -> TalosResult<SubmitUserActionResponse> {
        let mut client = self.lock().await;
        SdkGatewayClient::submit_user_action(&mut *client, request).await
    }

    async fn compact_session(&self, session_id: &str) -> TalosResult<SessionInfo> {
        let mut client = self.lock().await;
        SdkGatewayClient::compact_session(
            &mut *client,
            SessionCompactRequest {
                session_id: session_id.to_string(),
            },
        )
        .await
        .map(|response| response.session)
    }

    async fn clear_session(&self, session_id: &str) -> TalosResult<SessionInfo> {
        let mut client = self.lock().await;
        SdkGatewayClient::clear_session(
            &mut *client,
            SessionClearRequest {
                session_id: session_id.to_string(),
            },
        )
        .await
        .map(|response| response.session)
    }

    async fn get_session_history(&self, session_id: &str) -> TalosResult<SessionHistorySnapshot> {
        let mut client = self.lock().await;
        SdkGatewayClient::get_session_history(
            &mut *client,
            SessionHistoryRequest {
                session_id: session_id.to_string(),
            },
        )
        .await
        .map(|response| response.history)
    }

    async fn get_active_turn(&self, session_id: &str) -> TalosResult<TimelineActiveTurnSnapshot> {
        let mut client = self.lock().await;
        SdkGatewayClient::get_active_turn(
            &mut *client,
            SessionActiveTurnRequest {
                session_id: session_id.to_string(),
            },
        )
        .await
        .map(|response| response.active_turn)
    }

    async fn start_turn_stream(
        &self,
        mut request: StartTurnRequest,
    ) -> TalosResult<mpsc::Receiver<TimelineEventEnvelope>> {
        let session_id = request.session_id.clone();
        if request.client_request_id.is_none() {
            request.client_request_id = Some(format!("tui-{}", uuid::Uuid::new_v4().simple()));
        }
        let mut client = self.lock().await;
        let response = SdkGatewayClient::start_turn(&mut *client, request).await?;
        let subscription = SdkGatewayClient::open_timeline_stream(
            &mut *client,
            SubscribeTimelineRequest {
                session_id,
                turn_id: Some(response.turn_id),
                since_event_id: None,
                replay_limit: Some(256),
                close_when_idle: Some(false),
                close_on_terminal: Some(true),
            },
        )
        .await?;
        Ok(subscription.stream.into_inner())
    }

    async fn reconnect_active_turn_stream(
        &self,
        session_id: &str,
        since_event_id: Option<String>,
    ) -> TalosResult<mpsc::Receiver<TimelineEventEnvelope>> {
        let mut client = self.lock().await;
        let active_turn = SdkGatewayClient::get_active_turn(
            &mut *client,
            SessionActiveTurnRequest {
                session_id: session_id.to_string(),
            },
        )
        .await?
        .active_turn;
        let Some(turn_id) = active_turn.turn_id else {
            return Err(TalosError::public(
                talos_error::ErrorCode::Conflict,
                "No active turn stream is available for this session.".to_string(),
            ));
        };
        let subscription = SdkGatewayClient::open_timeline_stream(
            &mut *client,
            SubscribeTimelineRequest {
                session_id: session_id.to_string(),
                turn_id: Some(turn_id),
                since_event_id,
                replay_limit: Some(256),
                close_when_idle: Some(false),
                close_on_terminal: Some(true),
            },
        )
        .await?;
        Ok(subscription.stream.into_inner())
    }

    async fn subscribe_session_timeline(
        &self,
        session_id: &str,
        since_event_id: Option<String>,
    ) -> TalosResult<mpsc::Receiver<TimelineEventEnvelope>> {
        let mut client = self.lock().await;
        let subscription = SdkGatewayClient::open_timeline_stream(
            &mut *client,
            SubscribeTimelineRequest {
                session_id: session_id.to_string(),
                turn_id: None,
                since_event_id,
                replay_limit: Some(256),
                close_when_idle: Some(false),
                close_on_terminal: Some(false),
            },
        )
        .await?;
        Ok(subscription.stream.into_inner())
    }

    async fn recover_timeline(
        &self,
        session_id: &str,
        after_event_id: Option<String>,
    ) -> TalosResult<TimelineRecoveryResponse> {
        let mut client = self.lock().await;
        SdkGatewayClient::recover_timeline(
            &mut *client,
            RecoverTimelineRequest {
                session_id: session_id.to_string(),
                after_event_id,
                replay_limit: Some(256),
            },
        )
        .await
    }

    async fn shutdown(&self) -> TalosResult<()> {
        let mut client = self.lock().await;
        SdkGatewayClient::shutdown(&mut *client).await
    }
}
