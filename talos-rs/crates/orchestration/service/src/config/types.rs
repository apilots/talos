use std::sync::Arc;

use talos_error::TalosResult;
use talos_types::ConfigValue;

/// Identifies whether configuration reload follows an update or an explicit reload request.
#[derive(Debug, Clone)]
pub enum ConfigReloadTrigger {
    /// Reloads application components for the supplied update.
    Updated(ConfigValue),
    /// Reloads application components from the configured source.
    Explicit,
}

/// Prepared application-component replacement built before configuration persistence.
pub struct ConfigReloadPlan {
    publish: Box<dyn FnOnce() + Send>,
}

impl ConfigReloadPlan {
    /// Captures the infallible publication of an already validated replacement.
    pub fn new(publish: impl FnOnce() + Send + 'static) -> Self {
        Self {
            publish: Box::new(publish),
        }
    }

    /// Publishes the already prepared replacement.
    pub fn publish(self) {
        (self.publish)();
    }
}

/// Builds a validated application-component replacement without publishing it.
pub type ConfigReloadHook =
    Arc<dyn Fn(ConfigReloadTrigger) -> TalosResult<ConfigReloadPlan> + Send + Sync>;
