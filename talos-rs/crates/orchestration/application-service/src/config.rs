use std::sync::Arc;

use talos_config::{LlmConfig, LlmConfigService};
use talos_error::{TalosError, TalosResult};
use talos_types::{resolve_model_catalog, ModelInfo};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct ConfigService {
    llm_config_service: Arc<dyn LlmConfigService>,
    models: Arc<RwLock<Vec<ModelInfo>>>,
}

impl ConfigService {
    pub fn new(llm_config_service: Arc<dyn LlmConfigService>, models: Vec<ModelInfo>) -> Self {
        Self {
            llm_config_service,
            models: Arc::new(RwLock::new(models)),
        }
    }

    pub async fn list_models(&self) -> Vec<ModelInfo> {
        self.models.read().await.clone()
    }

    pub fn validate_model_supported(&self, model: &str) -> TalosResult<()> {
        if self
            .models
            .try_read()
            .map(|models| models.iter().any(|entry| entry.id == model))
            .unwrap_or(false)
        {
            return Ok(());
        }

        Err(TalosError::UnsupportedModel(model.to_string()))
    }

    pub fn get_llm(&self) -> TalosResult<LlmConfig> {
        self.llm_config_service.get()
    }

    pub fn update_llm(&self, llm: LlmConfig) -> TalosResult<LlmConfig> {
        let new_llm = self.llm_config_service.update(llm)?;
        self.reload_models(&new_llm)?;
        self.llm_config_service.get()
    }

    fn reload_models(&self, new_config: &LlmConfig) -> TalosResult<()> {
        let new_catalog = resolve_model_catalog(new_config);
        *self
            .models
            .try_write()
            .map_err(|_| TalosError::ConfigError("models lock poisoned".to_string()))? =
            new_catalog;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use talos_config::{InMemoryLlmConfigStore, LlmConfigManager};
    use talos_error::TalosError;
    use talos_types::{ModelCompatConfig, ModelCostConfig, ModelInfo};

    use super::ConfigService;

    fn test_service() -> ConfigService {
        let llm = talos_config::LlmConfig::default();
        ConfigService::new(
            Arc::new(LlmConfigManager::new(Arc::new(
                InMemoryLlmConfigStore::new(llm),
            ))),
            vec![ModelInfo {
                id: "qwen-plus".to_string(),
                name: "Qwen Plus".to_string(),
                provider: "bailian".to_string(),
                context_window: 32_768,
                reasoning: true,
                input: Vec::new(),
                cost: ModelCostConfig::default(),
                compat: ModelCompatConfig::default(),
            }],
        )
    }

    #[test]
    fn validate_model_supported_rejects_unknown_model() {
        let service = test_service();

        let error = service
            .validate_model_supported("unknown-model")
            .expect_err("unknown model should be rejected");

        assert!(matches!(error, TalosError::UnsupportedModel(_)));
    }

    #[test]
    fn get_llm_allows_regular_users() {
        let service = test_service();

        let llm = service
            .get_llm()
            .expect("regular user config read should work");

        assert_eq!(llm.provider, "anthropic");
    }
}
