use std::sync::Arc;

use talos_config::Config;
use talos_error::TalosResult;
use talos_types::ConfigValue;

/// Identifies whether runtime reload follows a persisted update or an explicit reload request.
#[derive(Debug, Clone)]
pub enum RuntimeConfigReloadTrigger {
    /// Reloads runtime state after the supplied value has been persisted.
    Updated(ConfigValue),
    /// Reloads runtime state from its configured source without a new value payload.
    Explicit,
}

/// Applies a validated runtime configuration reload and returns the active configuration.
pub type RuntimeConfigReloadHook =
    Arc<dyn Fn(RuntimeConfigReloadTrigger) -> TalosResult<Config> + Send + Sync>;
