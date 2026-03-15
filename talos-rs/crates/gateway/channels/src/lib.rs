use std::sync::Arc;

use talos_config::Config;
use talos_error::TalosResult;
use talos_gateway::Gateway;
use tokio::task::JoinHandle;

pub mod channels;

pub use channels::telegram::ChannelsTelegram;
pub use channels::Channel;

pub struct Channels {
    handles: Vec<JoinHandle<()>>,
}

impl Channels {
    pub async fn start_enabled(gateway: Arc<dyn Gateway>, config: &Config) -> TalosResult<Self> {
        let mut handles = Vec::new();
        let telegram = ChannelsTelegram::new();
        if let Some(handle) = telegram.start(gateway, config).await? {
            handles.push(handle);
        }
        Ok(Self { handles })
    }

    pub fn stop_all(self) {
        for handle in self.handles {
            handle.abort();
        }
    }
}
