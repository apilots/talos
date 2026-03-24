use async_trait::async_trait;
use talos_config::Config;
use talos_error::TalosResult;

use super::adapters::{OutboundAdapter, StreamingAdapter};
use super::types::{ChannelCapabilities, ChannelDefinition, ChannelRuntimeContext};

#[async_trait]
pub trait Channel: Send + Sync {
    fn definition(&self) -> &ChannelDefinition;
    fn enabled_account_ids(&self, config: &Config) -> Vec<String>;

    async fn start_account(&self, ctx: ChannelRuntimeContext) -> TalosResult<()>;
    async fn stop_account(&self, account_id: &str) -> TalosResult<()>;

    fn capabilities(&self) -> &ChannelCapabilities {
        &self.definition().capabilities
    }

    fn outbound_adapter(&self) -> Option<&dyn OutboundAdapter> {
        None
    }

    fn streaming_adapter(&self) -> Option<&dyn StreamingAdapter> {
        None
    }
}
