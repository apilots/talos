use async_trait::async_trait;
use talos_client::{
    ModelInfo, SessionCreateParams, TalosClient, TalosEvent, TalosMessageRequest,
    TalosMessageResponse, TalosSession,
};
use talos_error::TalosResult;
use tokio::sync::mpsc;

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
    async fn send_message_stream(
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
impl GatewayClient for TalosClient {
    async fn list_models(&self) -> TalosResult<Vec<ModelInfo>> {
        TalosClient::list_models(self).await
    }

    async fn list_sessions(&self) -> TalosResult<Vec<TalosSession>> {
        TalosClient::list_sessions(self).await
    }

    async fn create_session(&self, params: &SessionCreateParams) -> TalosResult<TalosSession> {
        TalosClient::create_session(self, params).await
    }

    async fn get_session(&self, session_id: &str) -> TalosResult<TalosSession> {
        TalosClient::get_session(self, session_id).await
    }

    async fn close_session(&self, session_id: &str) -> TalosResult<()> {
        TalosClient::close_session(self, session_id).await
    }

    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()> {
        TalosClient::cancel_active_turn(self, session_id).await
    }

    async fn update_session_model(
        &self,
        session_id: &str,
        model: &str,
    ) -> TalosResult<TalosSession> {
        TalosClient::update_session_model(self, session_id, model).await?;
        TalosClient::get_session(self, session_id).await
    }

    async fn send_message_stream(
        &self,
        session_id: &str,
        request: &TalosMessageRequest,
    ) -> TalosResult<mpsc::Receiver<TalosEvent>> {
        TalosClient::send_message_stream(self, session_id, request).await
    }

    async fn submit_tool_approval(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        approved: bool,
        reason: Option<String>,
    ) -> TalosResult<()> {
        TalosClient::submit_tool_approval(self, session_id, call_id, tool_name, approved, reason)
            .await
    }

    async fn get_session_turns(&self, session_id: &str) -> TalosResult<Vec<TalosMessageResponse>> {
        TalosClient::get_session_turns(self, session_id).await
    }
}
