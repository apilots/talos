use std::sync::Arc;

use talos_config::{LlmConfig, LlmConfigService};
use talos_error::{TalosError, TalosResult};
use talos_types::{
    resolve_model_catalog, ConfigDeleteRequest, ConfigDeleteResponse, ConfigGetRequest,
    ConfigGetResponse, ConfigReloadRequest, ConfigReloadResponse, ConfigUpdateRequest,
    ConfigUpdateResponse, ConfigValue, ModelInfo,
};
use tokio::sync::RwLock;

use super::types::RuntimeConfigReloadHook;

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

    pub fn get_config(&self, request: ConfigGetRequest) -> TalosResult<ConfigGetResponse> {
        Self::validate_config_path(request.path.as_deref())?;
        self.llm_config_service.get_read().map(Into::into)
    }

    pub fn update_config(&self, request: ConfigUpdateRequest) -> TalosResult<ConfigUpdateResponse> {
        Self::validate_config_path(Some(&request.path))?;
        let ConfigValue::Llm(llm) = request.value;
        self.update_llm(llm.into())?;
        self.llm_config_service.get_read().map(Into::into)
    }

    pub fn update_llm(&self, llm: LlmConfig) -> TalosResult<LlmConfig> {
        let new_llm = self.llm_config_service.update(llm)?;
        self.reload_models(&new_llm)?;
        if let Some(reload_hook) = &self.reload_hook {
            reload_hook(&new_llm)?;
        }
        self.llm_config_service.get()
    }

    pub fn delete_config(&self, request: ConfigDeleteRequest) -> TalosResult<ConfigDeleteResponse> {
        Self::validate_config_path(Some(&request.path))?;
        Err(TalosError::ValidationError(
            "config delete is not supported for llm".to_string(),
        ))
    }

    pub fn reload_config(
        &self,
        _request: ConfigReloadRequest,
    ) -> TalosResult<ConfigReloadResponse> {
        Ok(ConfigReloadResponse { success: true })
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

    fn validate_config_path(path: Option<&str>) -> TalosResult<()> {
        match path.unwrap_or("llm") {
            "" | "llm" => Ok(()),
            other => Err(TalosError::ValidationError(format!(
                "unsupported config path: {}",
                other
            ))),
        }
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
