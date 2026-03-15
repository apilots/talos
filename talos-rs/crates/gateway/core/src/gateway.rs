use async_trait::async_trait;
use std::sync::Arc;
use tracing::{error, info};

use crate::r#trait::Gateway;
use crate::types::{GatewayRequest, GatewayResponse};
use talos_error::TalosResult;
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
            "Handling gateway request: channel={}, user={}, content_len={}",
            request.channel,
            request.user_id,
            request.content.len()
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
    ) -> TalosResult<tokio::sync::mpsc::Receiver<talos_types::canonical::CanonicalEvent>> {
        self.handler.handle_stream(request).await
    }

    async fn start(&self) -> TalosResult<()> {
        info!("Starting gateway...");
        Ok(())
    }
}
