use async_trait::async_trait;
use talos_error::{TalosError, TalosResult};
use talos_sdk::{
    AuthSessionReadRequest, AuthSessionReadResponse, GatewayClient as SdkGatewayClient,
    InterruptTurnRequest, ModelListRequest, ModelListResponse, ModelSelection,
    RecoverTimelineRequest, SessionActiveTurnRequest, SessionClearRequest, SessionCloseRequest,
    SessionCompactRequest, SessionConf, SessionGetRequest, SessionHistoryRequest,
    SessionHistorySnapshot, SessionInfo, SessionListRequest, SessionPatchRequest, SkillSuggestion,
    SkillsSuggestRequest, StartTurnRequest, SubscribeTimelineRequest, TimelineActiveTurnSnapshot,
    TimelineEventEnvelope, TimelineRecoveryResponse, WorkspaceInfoRequest, WorkspaceInfoResponse,
};
use tokio::sync::mpsc;
use tokio::sync::Mutex;

#[async_trait]
pub trait GatewayClient: Send + Sync {
    async fn list_models(&self) -> TalosResult<ModelListResponse>;
    async fn read_auth_session(&self) -> TalosResult<AuthSessionReadResponse>;
    async fn workspace_info(&self) -> TalosResult<WorkspaceInfoResponse>;
    async fn suggest_skills(&self, query: &str) -> TalosResult<Vec<SkillSuggestion>>;
    async fn list_sessions(&self) -> TalosResult<Vec<SessionInfo>>;
    async fn get_session(&self, session_id: &str) -> TalosResult<SessionInfo>;
    async fn close_session(&self, session_id: &str) -> TalosResult<()>;
    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()>;
    async fn update_session_model_selection(
        &self,
        session_id: &str,
        model_selection: &ModelSelection,
    ) -> TalosResult<SessionInfo>;
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

    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()> {
        let mut client = self.lock().await;
        SdkGatewayClient::interrupt_turn(
            &mut *client,
            InterruptTurnRequest {
                session_id: session_id.to_string(),
            },
        )
        .await
        .map(|_| ())
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
                }),
                project_id: None,
                current_directory: None,
            },
        )
        .await
        .map(|response| response.session)
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
            return Err(TalosError::Conflict(
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
