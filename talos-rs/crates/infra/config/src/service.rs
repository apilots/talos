use std::sync::Arc;

use talos_error::TalosResult;

use crate::{store::LlmConfigStore, LlmConfig};

pub trait LlmConfigService: Send + Sync {
    /// Returns the raw LlmConfig (${VAR} placeholders not expanded).
    fn get(&self) -> TalosResult<LlmConfig>;

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

    fn update(&self, llm: LlmConfig) -> TalosResult<LlmConfig> {
        self.store.save(&llm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::InMemoryLlmConfigStore;

    fn make_service() -> LlmConfigManager {
        LlmConfigManager::new(Arc::new(InMemoryLlmConfigStore::new(LlmConfig::default())))
    }

    #[test]
    fn test_get_delegates_to_store() {
        let service = make_service();
        let llm = service.get().unwrap();
        assert_eq!(llm.provider, LlmConfig::default().provider);
    }

    #[test]
    fn test_update_saves_and_returns_config() {
        let service = make_service();
        let mut llm = service.get().unwrap();
        llm.model = "updated-model".to_string();
        let returned = service.update(llm.clone()).unwrap();
        assert_eq!(returned.model, "updated-model");
        assert_eq!(service.get().unwrap().model, "updated-model");
    }
}
