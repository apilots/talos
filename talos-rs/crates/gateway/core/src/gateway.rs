use async_trait::async_trait;
use std::sync::Arc;
use tracing::{error, info};

use crate::r#trait::Gateway;
use crate::types::{GatewayRequest, GatewayResponse};
use talos_error::TalosResult;
use talos_types::talos::TalosInputItem;
use talos_types::RequestHandler;

pub struct GatewayImpl {
    handler: Arc<dyn RequestHandler>,
}

impl GatewayImpl {
    pub fn new(handler: Arc<dyn RequestHandler>) -> Self {
        Self { handler }
    }
}

#[async_trait]
impl Gateway for GatewayImpl {
    async fn handle_request(&self, request: GatewayRequest) -> TalosResult<GatewayResponse> {
        info!(
            "Handling gateway request: session_id={}, provider={}, user={}, input_items={}, text_len={}",
            request.session_id,
            request.context.provider,
            request.context.user_id,
            request.input.len(),
            text_input_len(&request.input)
        );

        match self.handler.handle(request).await {
            Ok(response) => {
                info!("Successfully processed request");
                Ok(response)
            }
            Err(err) => {
                error!("Error processing request: {:?}", err);
                Err(err)
            }
        }
    }

    async fn handle_request_stream(
        &self,
        request: GatewayRequest,
    ) -> TalosResult<tokio::sync::mpsc::Receiver<talos_types::talos::TalosTimelineEvent>> {
        self.handler.handle_stream(request).await
    }

    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()> {
        self.handler.cancel_active_turn(session_id).await
    }

    async fn submit_tool_approval(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        approved: bool,
        reason: Option<String>,
    ) -> TalosResult<()> {
        self.handler
            .submit_tool_approval(session_id, call_id, tool_name, approved, reason)
            .await
    }

    async fn submit_dynamic_tool_result(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        self.handler
            .submit_dynamic_tool_result(session_id, call_id, tool_name, output, success)
            .await
    }

    async fn start(&self) -> TalosResult<()> {
        info!("Starting gateway...");
        Ok(())
    }
}

fn text_input_len(input: &[TalosInputItem]) -> usize {
    input
        .iter()
        .map(|item| match item {
            TalosInputItem::Text { text } => text.len(),
            _ => 0,
        })
        .sum()
}
