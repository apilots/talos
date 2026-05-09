use std::sync::Arc;

use talos_config::{LlmConfig, LlmConfigRead, LlmConfigService};
use talos_error::{TalosError, TalosResult};
use talos_types::{resolve_model_catalog, ModelInfo};
use tokio::sync::RwLock;

pub type RuntimeConfigReloadHook = Arc<dyn Fn(&LlmConfig) -> TalosResult<()> + Send + Sync>;

#[derive(Clone)]
pub struct ConfigService {
    llm_config_service: Arc<dyn LlmConfigService>,
    models: Arc<RwLock<Vec<ModelInfo>>>,
    reload_hook: Option<RuntimeConfigReloadHook>,
}

impl ConfigService {
    pub fn new(llm_config_service: Arc<dyn LlmConfigService>, models: Vec<ModelInfo>) -> Self {
        Self::new_with_reload_hook(llm_config_service, models, None)
    }

    pub fn new_with_reload_hook(
        llm_config_service: Arc<dyn LlmConfigService>,
        models: Vec<ModelInfo>,
        reload_hook: Option<RuntimeConfigReloadHook>,
    ) -> Self {
        Self {
            llm_config_service,
            models: Arc::new(RwLock::new(models)),
            reload_hook,
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

    pub fn get_llm_read(&self) -> TalosResult<LlmConfigRead> {
        self.llm_config_service.get_read()
    }

    pub fn update_llm(&self, llm: LlmConfig) -> TalosResult<LlmConfig> {
        let new_llm = self.llm_config_service.update(llm)?;
        self.reload_models(&new_llm)?;
        if let Some(reload_hook) = &self.reload_hook {
            reload_hook(&new_llm)?;
        }
        self.llm_config_service.get()
    }

    pub fn update_llm_read(&self, llm: LlmConfig) -> TalosResult<LlmConfigRead> {
        self.update_llm(llm)?;
        self.llm_config_service.get_read()
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
    use std::sync::atomic::{AtomicUsize, Ordering};
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

    #[test]
    fn update_llm_invokes_reload_hook() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_hook = calls.clone();
        let service = ConfigService::new_with_reload_hook(
            Arc::new(LlmConfigManager::new(Arc::new(
                InMemoryLlmConfigStore::new(talos_config::LlmConfig::default()),
            ))),
            Vec::new(),
            Some(Arc::new(move |_| {
                calls_for_hook.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })),
        );

        let mut llm = service.get_llm().unwrap();
        llm.model = "updated-model".to_string();
        service.update_llm(llm).unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
