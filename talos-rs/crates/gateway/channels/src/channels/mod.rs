use std::sync::Arc;

use async_trait::async_trait;
use talos_config::Config;
use talos_error::TalosResult;
use talos_gateway::Gateway;
use tokio::task::JoinHandle;

pub mod telegram;

#[async_trait]
pub trait Channel: Send + Sync {
    fn name(&self) -> &str;
    async fn start(
        &self,
        gateway: Arc<dyn Gateway>,
        config: &Config,
    ) -> TalosResult<Option<JoinHandle<()>>>;
}
