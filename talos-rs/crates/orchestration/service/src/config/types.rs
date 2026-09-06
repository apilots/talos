use std::sync::Arc;

use talos_config::Config;
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
    config: Config,
    publish: Box<dyn FnOnce() + Send>,
}

impl ConfigReloadPlan {
    /// Captures one validated configuration and its infallible publication.
    pub fn new(config: Config, publish: impl FnOnce() + Send + 'static) -> Self {
        Self {
            config,
            publish: Box::new(publish),
        }
    }

    /// Returns the configuration that will become active after publication.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Publishes the already prepared replacement and returns its active configuration.
    pub(crate) fn publish(self) -> Config {
        (self.publish)();
        self.config
    }
}

/// Builds a validated application-component replacement without publishing it.
pub type ConfigReloadHook =
    Arc<dyn Fn(ConfigReloadTrigger) -> TalosResult<ConfigReloadPlan> + Send + Sync>;
