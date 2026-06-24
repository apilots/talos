use async_trait::async_trait;
use talos_error::{TalosError, TalosResult};
use talos_sdk::{
    AppServerClient, ModelInfo, SessionHistorySnapshot, SessionInfo, SkillSuggestion,
    SkillsSuggestRequest, StartTurnRequest, TimelineActiveTurnSnapshot, TimelineEventEnvelope,
    TimelineRecoveryResponse,
};
use tokio::sync::mpsc;
use tokio::sync::Mutex;

#[async_trait]
pub trait GatewayClient: Send + Sync {
    async fn list_models(&self) -> TalosResult<Vec<ModelInfo>>;
    async fn suggest_skills(&self, query: &str) -> TalosResult<Vec<SkillSuggestion>>;
    async fn list_sessions(&self) -> TalosResult<Vec<SessionInfo>>;
    async fn get_session(&self, session_id: &str) -> TalosResult<SessionInfo>;
    async fn close_session(&self, session_id: &str) -> TalosResult<()>;
    async fn delete_session(&self, session_id: &str) -> TalosResult<()>;
    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()>;
    async fn update_session_model(&self, session_id: &str, model: &str)
        -> TalosResult<SessionInfo>;
    async fn compact_session(&self, session_id: &str) -> TalosResult<SessionInfo>;
    async fn clear_session(&self, session_id: &str) -> TalosResult<SessionInfo>;
    async fn get_session_history(&self, session_id: &str) -> TalosResult<SessionHistorySnapshot>;
    async fn get_active_turn(&self, session_id: &str) -> TalosResult<TimelineActiveTurnSnapshot>;
    async fn send_timeline_events(
        &self,
        request: StartTurnRequest,
    ) -> TalosResult<mpsc::Receiver<TimelineEventEnvelope>>;
    async fn reconnect_active_timeline_events(
        &self,
        session_id: &str,
        since_event_id: Option<String>,
    ) -> TalosResult<mpsc::Receiver<TimelineEventEnvelope>>;
    async fn recover_session(
        &self,
        session_id: &str,
        after_event_id: Option<String>,
    ) -> TalosResult<TimelineRecoveryResponse>;
    /// Shut down the app-server process. After this call, the client must not be reused.
    async fn shutdown(&self) -> TalosResult<()>;
}

#[async_trait]
impl GatewayClient for Mutex<AppServerClient> {
    async fn list_models(&self) -> TalosResult<Vec<ModelInfo>> {
        self.lock()
            .await
            .list_models()
            .await
            .map(|response| response.models)
    }

    async fn suggest_skills(&self, query: &str) -> TalosResult<Vec<SkillSuggestion>> {
        self.lock()
            .await
            .suggest_skills(SkillsSuggestRequest {
                query: query.to_string(),
                limit: Some(8),
            })
            .await
            .map(|response| response.data)
    }

    async fn list_sessions(&self) -> TalosResult<Vec<SessionInfo>> {
        self.lock()
            .await
            .list_sessions(Default::default())
            .await
            .map(|response| response.sessions)
    }

    async fn get_session(&self, session_id: &str) -> TalosResult<SessionInfo> {
        self.lock()
            .await
            .get_session_by_id(session_id)
            .await
            .map(|response| response.session)
    }

    async fn close_session(&self, session_id: &str) -> TalosResult<()> {
        self.lock()
            .await
            .close_session_by_id(session_id)
            .await
            .map(|_| ())
    }

    async fn delete_session(&self, session_id: &str) -> TalosResult<()> {
        self.lock()
            .await
            .delete_session_by_id(session_id)
            .await
            .map(|_| ())
    }

    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()> {
        self.lock()
            .await
            .interrupt_turn_by_id(session_id)
            .await
            .map(|_| ())
    }

    async fn update_session_model(
        &self,
        session_id: &str,
        model: &str,
    ) -> TalosResult<SessionInfo> {
        self.lock()
            .await
            .update_session_model(session_id, model)
            .await
            .map(|response| response.session)
    }

    async fn compact_session(&self, _session_id: &str) -> TalosResult<SessionInfo> {
        Err(TalosError::ValidationError(
            "app-server TUI spike does not support session compact yet".to_string(),
        ))
    }

    async fn clear_session(&self, _session_id: &str) -> TalosResult<SessionInfo> {
        Err(TalosError::ValidationError(
            "app-server TUI spike does not support session clear yet".to_string(),
        ))
    }

    async fn get_session_history(&self, session_id: &str) -> TalosResult<SessionHistorySnapshot> {
        self.lock()
            .await
            .get_session_history_by_id(session_id)
            .await
            .map(|response| response.history)
    }

    async fn get_active_turn(&self, session_id: &str) -> TalosResult<TimelineActiveTurnSnapshot> {
        self.lock()
            .await
            .get_active_turn_by_id(session_id)
            .await
            .map(|response| response.active_turn)
    }

    async fn send_timeline_events(
        &self,
        request: StartTurnRequest,
    ) -> TalosResult<mpsc::Receiver<TimelineEventEnvelope>> {
        self.lock().await.send_timeline_events(request).await
    }

    async fn reconnect_active_timeline_events(
        &self,
        session_id: &str,
        since_event_id: Option<String>,
    ) -> TalosResult<mpsc::Receiver<TimelineEventEnvelope>> {
        self.lock()
            .await
            .reconnect_active_timeline_events(session_id, since_event_id)
            .await
    }

    async fn recover_session(
        &self,
        session_id: &str,
        after_event_id: Option<String>,
    ) -> TalosResult<TimelineRecoveryResponse> {
        self.lock()
            .await
            .recover_session_after(session_id, after_event_id)
            .await
    }

    async fn shutdown(&self) -> TalosResult<()> {
        self.lock().await.shutdown().await
    }
}
