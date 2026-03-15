use crate::types::{GatewayRequest, GatewayResponse};
use async_trait::async_trait;
use talos_error::TalosResult;
use talos_types::canonical::CanonicalEvent;
use tokio::sync::mpsc;

#[async_trait]
pub trait Gateway: Send + Sync {
    async fn handle_request(&self, request: GatewayRequest) -> TalosResult<GatewayResponse>;
    async fn handle_request_stream(
        &self,
        request: GatewayRequest,
    ) -> TalosResult<mpsc::Receiver<CanonicalEvent>>;
    async fn start(&self) -> TalosResult<()>;
}
