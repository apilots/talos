use crate::types::{DeliveryResult, MediaContent, MessageTarget, OutboundContext};
use async_trait::async_trait;
use talos_error::TalosResult;

#[async_trait]
pub trait OutboundAdapter: Send + Sync {
    async fn send_text(&self, ctx: &OutboundContext) -> TalosResult<DeliveryResult>;
    async fn send_media(
        &self,
        ctx: &OutboundContext,
        media: MediaContent,
    ) -> TalosResult<DeliveryResult>;
}

#[async_trait]
pub trait StreamingAdapter: Send + Sync {
    async fn send_typing(&self, target: &MessageTarget) -> TalosResult<()>;
    async fn edit_text(
        &self,
        target: &MessageTarget,
        message_id: &str,
        text: &str,
    ) -> TalosResult<()>;
}
