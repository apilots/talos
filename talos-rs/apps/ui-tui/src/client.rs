use async_trait::async_trait;
use talos_client::{
    AppServerClient, ModelInfo, SessionCreateParams, TalosEvent, TalosMessageRequest,
    TalosMessageResponse, TalosSession,
};
use talos_error::{TalosError, TalosResult};
use tokio::sync::mpsc;
use tokio::sync::Mutex;

#[async_trait]
pub trait GatewayClient: Send + Sync {
    async fn list_models(&self) -> TalosResult<Vec<ModelInfo>>;
    async fn list_sessions(&self) -> TalosResult<Vec<TalosSession>>;
    async fn create_session(&self, params: &SessionCreateParams) -> TalosResult<TalosSession>;
    async fn get_session(&self, session_id: &str) -> TalosResult<TalosSession>;
    async fn close_session(&self, session_id: &str) -> TalosResult<()>;
    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()>;
    async fn update_session_model(
        &self,
        session_id: &str,
        model: &str,
    ) -> TalosResult<TalosSession>;
    async fn compact_session(&self, session_id: &str) -> TalosResult<TalosSession>;
    async fn clear_session(&self, session_id: &str) -> TalosResult<TalosSession>;
    async fn send_message_events(
        &self,
        session_id: &str,
        request: &TalosMessageRequest,
    ) -> TalosResult<mpsc::Receiver<TalosEvent>>;
    async fn submit_tool_approval(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        approved: bool,
        reason: Option<String>,
    ) -> TalosResult<()>;
    async fn get_session_turns(&self, session_id: &str) -> TalosResult<Vec<TalosMessageResponse>>;
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

    async fn send_message_events(
        &self,
        session_id: &str,
        request: &TalosMessageRequest,
    ) -> TalosResult<mpsc::Receiver<TalosEvent>> {
        self.lock()
            .await
            .send_message_events(session_id, request.clone())
            .await
    }

    async fn submit_tool_approval(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        approved: bool,
        reason: Option<String>,
    ) -> TalosResult<()> {
        self.lock()
            .await
            .submit_tool_approval(session_id, call_id, tool_name, approved, reason)
            .await
    }

    async fn get_session_turns(&self, _session_id: &str) -> TalosResult<Vec<TalosMessageResponse>> {
        self.lock().await.get_session_turns(_session_id).await
    }
}
