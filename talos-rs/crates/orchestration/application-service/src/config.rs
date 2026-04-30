use std::sync::Arc;

use talos_config::{LlmConfig, LlmConfigService};
use talos_error::{TalosError, TalosResult};
use talos_types::{resolve_model_catalog, ModelInfo};
use tokio::sync::RwLock;

use crate::access::{Actor, ResourceAccessService};

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

    pub fn get_llm(&self, actor: &Actor) -> TalosResult<LlmConfig> {
        ResourceAccessService::require_admin(actor)?;
        self.llm_config_service.get()
    }

    pub fn update_llm(&self, actor: &Actor, llm: LlmConfig) -> TalosResult<LlmConfig> {
        ResourceAccessService::require_admin(actor)?;
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

    use crate::access::Actor;

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
    fn get_llm_rejects_non_admin_actor() {
        let service = test_service();

        let error = service
            .get_llm(&Actor::regular("member"))
            .expect_err("non-admin should be rejected");

        assert!(matches!(error, TalosError::AuthError(_)));
    }
}
