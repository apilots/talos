use async_trait::async_trait;
use talos_error::{TalosError, TalosResult};
use talos_sdk::{
    AppServerClient, ModelInfo, SessionCreateParams, SessionHistorySnapshot, TalosMessageRequest,
    TalosSession, TimelineActiveTurnSnapshot, TimelineEventEnvelope, TimelineRecoverySnapshot,
};
use tokio::sync::mpsc;
use tokio::sync::Mutex;

#[async_trait]
pub trait GatewayClient: Send + Sync {
    async fn list_models(&self) -> TalosResult<Vec<ModelInfo>>;
    async fn list_sessions(&self) -> TalosResult<Vec<TalosSession>>;
    async fn create_session(&self, params: &SessionCreateParams) -> TalosResult<TalosSession>;
    async fn get_session(&self, session_id: &str) -> TalosResult<TalosSession>;
    async fn close_session(&self, session_id: &str) -> TalosResult<()>;
    async fn delete_session(&self, session_id: &str) -> TalosResult<()>;
    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()>;
    async fn update_session_model(
        &self,
        session_id: &str,
        model: &str,
    ) -> TalosResult<TalosSession>;
    async fn compact_session(&self, session_id: &str) -> TalosResult<TalosSession>;
    async fn clear_session(&self, session_id: &str) -> TalosResult<TalosSession>;
    async fn get_session_history(&self, session_id: &str) -> TalosResult<SessionHistorySnapshot>;
    async fn get_active_turn(&self, session_id: &str) -> TalosResult<TimelineActiveTurnSnapshot>;
    async fn send_timeline_events(
        &self,
        session_id: &str,
        request: &TalosMessageRequest,
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
    ) -> TalosResult<TimelineRecoverySnapshot>;
    /// Shut down the remote gateway process. After this call, the client must not be reused.
    async fn shutdown(&self) -> TalosResult<()>;
}

#[async_trait]
impl GatewayClient for Mutex<AppServerClient> {
    async fn list_models(&self) -> TalosResult<Vec<ModelInfo>> {
        self.lock().await.list_models().await
    }

    async fn list_sessions(&self) -> TalosResult<Vec<TalosSession>> {
        self.lock().await.list_sessions(Default::default()).await
    }

    async fn create_session(&self, params: &SessionCreateParams) -> TalosResult<TalosSession> {
        self.lock().await.create_session(params.clone()).await
    }

    async fn get_session(&self, session_id: &str) -> TalosResult<TalosSession> {
        self.lock().await.get_session(session_id).await
    }

    async fn close_session(&self, session_id: &str) -> TalosResult<()> {
        self.lock().await.close_session(session_id).await
    }

    async fn delete_session(&self, session_id: &str) -> TalosResult<()> {
        self.lock().await.delete_session(session_id).await
    }

    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()> {
        self.lock().await.interrupt_turn(session_id).await
    }

    async fn update_session_model(
        &self,
        session_id: &str,
        model: &str,
    ) -> TalosResult<TalosSession> {
        self.lock()
            .await
            .update_session_model(session_id, model)
            .await
    }

    async fn compact_session(&self, _session_id: &str) -> TalosResult<TalosSession> {
        Err(TalosError::ValidationError(
            "app-server TUI spike does not support session compact yet".to_string(),
        ))
    }

    async fn clear_session(&self, _session_id: &str) -> TalosResult<TalosSession> {
        Err(TalosError::ValidationError(
            "app-server TUI spike does not support session clear yet".to_string(),
        ))
    }

    async fn get_session_history(&self, session_id: &str) -> TalosResult<SessionHistorySnapshot> {
        self.lock().await.get_session_history(session_id).await
    }

    async fn get_active_turn(&self, session_id: &str) -> TalosResult<TimelineActiveTurnSnapshot> {
        self.lock().await.get_active_turn(session_id).await
    }

    async fn send_timeline_events(
        &self,
        session_id: &str,
        request: &TalosMessageRequest,
    ) -> TalosResult<mpsc::Receiver<TimelineEventEnvelope>> {
        self.lock()
            .await
            .send_timeline_events(session_id, request.clone())
            .await
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
    ) -> TalosResult<TimelineRecoverySnapshot> {
        self.lock()
            .await
            .recover_session_after(session_id, after_event_id)
            .await
    }

    async fn shutdown(&self) -> TalosResult<()> {
        self.lock().await.shutdown().await
    }
}
