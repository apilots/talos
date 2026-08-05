use std::sync::Arc;

use talos_config::Config;
use talos_error::TalosResult;
use talos_types::ConfigValue;

pub type RuntimeConfigReloadHook = Arc<dyn Fn(ConfigValue) -> TalosResult<Config> + Send + Sync>;
