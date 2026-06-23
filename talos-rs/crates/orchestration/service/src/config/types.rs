use std::sync::Arc;

use talos_config::LlmConfig;
use talos_error::TalosResult;

pub type RuntimeConfigReloadHook = Arc<dyn Fn(&LlmConfig) -> TalosResult<()> + Send + Sync>;
