use std::sync::Arc;

use talos_error::TalosResult;

use crate::{store::LlmConfigStore, LlmConfig, LlmConfigRead};

pub trait LlmConfigService: Send + Sync {
    /// Returns the raw LlmConfig (${VAR} placeholders not expanded).
    fn get(&self) -> TalosResult<LlmConfig>;

    /// Returns UI-facing config plus non-sensitive readiness/secret status.
    fn get_read(&self) -> TalosResult<LlmConfigRead>;

    /// Persists the given LlmConfig and returns it.
    fn update(&self, llm: LlmConfig) -> TalosResult<LlmConfig>;
}

pub struct LlmConfigManager {
    store: Arc<dyn LlmConfigStore>,
}

impl LlmConfigManager {
    pub fn new(store: Arc<dyn LlmConfigStore>) -> Self {
        Self { store }
    }
}

impl LlmConfigService for LlmConfigManager {
    fn get(&self) -> TalosResult<LlmConfig> {
        self.store.load_raw()
    }

    fn get_read(&self) -> TalosResult<LlmConfigRead> {
        self.store.load_read()
    }

    fn update(&self, llm: LlmConfig) -> TalosResult<LlmConfig> {
        self.store.save(&llm)
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
