use std::sync::Arc;

use talos_config::{
    provider_config_for_llm, Config, ConfigStore, LlmConfig, WebSearchProviderRuntimeConfig,
    WebSearchToolConfig,
};
use talos_error::{TalosError, TalosResult};
use talos_types::{
    resolve_default_model_selection, resolve_model_catalog, ConfigDeleteRequest,
    ConfigDeleteResponse, ConfigGetRequest, ConfigGetResponse, ConfigPath, ConfigReloadRequest,
    ConfigReloadResponse, ConfigUpdateRequest, ConfigUpdateResponse, ConfigValue, ModelInfo,
    ModelListResponse, ModelRef, ModelSelection,
};
use tokio::sync::RwLock;

use super::types::RuntimeConfigReloadHook;

#[derive(Clone)]
pub struct ConfigService {
    config_store: Arc<dyn ConfigStore>,
    models: Arc<RwLock<Vec<ModelInfo>>>,
    reload_hook: Option<RuntimeConfigReloadHook>,
}

impl ConfigService {
    pub fn new(config_store: Arc<dyn ConfigStore>, models: Vec<ModelInfo>) -> Self {
        Self::new_with_reload_hook(config_store, models, None)
    }

    pub fn new_with_reload_hook(
        config_store: Arc<dyn ConfigStore>,
        models: Vec<ModelInfo>,
        reload_hook: Option<RuntimeConfigReloadHook>,
    ) -> Self {
        Self {
            config_store,
            models: Arc::new(RwLock::new(models)),
            reload_hook,
        }
    }

    pub async fn list_models(&self) -> TalosResult<ModelListResponse> {
        let llm = self.config_store.load_llm_raw()?;
        let default_model_selection = resolve_default_model_selection(&llm)?;
        Ok(ModelListResponse {
            models: self.models.read().await.clone(),
            default_model_selection,
        })
    }

    pub fn validate_model_selection_supported(
        &self,
        selection: &ModelSelection,
    ) -> TalosResult<()> {
        let model = self.model_info_for_ref(&selection.model_ref)?;
        selection.resolve_reasoning(&model)?;
        Ok(())
    }

    fn model_info_for_ref(&self, model_ref: &ModelRef) -> TalosResult<ModelInfo> {
        if model_ref.provider.trim().is_empty() || model_ref.id.trim().is_empty() {
            return Err(TalosError::public(
                talos_error::ErrorCode::ProviderUnavailable,
                format!("{}:{}", model_ref.provider, model_ref.id),
            ));
        }
        let llm = self.config_store.load_llm_raw()?;
        let Some(provider) = provider_config_for_llm(&llm, &model_ref.provider) else {
            return Err(TalosError::public(
                talos_error::ErrorCode::ProviderUnavailable,
                format!("{}:{}", model_ref.provider, model_ref.id),
            ));
        };
        if let Some(model) = provider
            .models
            .iter()
            .find(|entry| entry.id == model_ref.id)
        {
            return Ok(ModelInfo::from_provider_model(
                model_ref.provider.clone(),
                model.clone(),
            ));
        }
        Err(TalosError::public(
            talos_error::ErrorCode::ProviderUnavailable,
            format!("{}:{}", model_ref.provider, model_ref.id),
        ))
    }

    pub fn get_llm(&self) -> TalosResult<LlmConfig> {
        self.config_store.load_llm_raw()
    }

    pub fn get_config(&self, request: ConfigGetRequest) -> TalosResult<ConfigGetResponse> {
        match request.path {
            ConfigPath::Llm => self.config_store.load_llm_read().map(Into::into),
            ConfigPath::WebSearch => self.config_store.load_web_search_read().map(Into::into),
        }
    }

    pub fn update_config(&self, request: ConfigUpdateRequest) -> TalosResult<ConfigUpdateResponse> {
        match request.value {
            ConfigValue::Llm(llm) => {
                let llm = *llm;
                let saved = self.config_store.save_llm(&llm)?;
                let reloaded =
                    self.reload_after_update(ConfigValue::Llm(Box::new(saved.clone())))?;
                self.reload_models(reloaded.as_ref().map_or(&saved, |config| &config.llm))?;
                self.config_store.load_llm_read().map(Into::into)
            }
            ConfigValue::WebSearch(web_search) => {
                validate_web_search_config(&web_search)?;
                let saved = self.config_store.save_web_search(&web_search)?;
                self.reload_after_update(ConfigValue::WebSearch(Box::new(saved.clone())))?;
                self.config_store.load_web_search_read().map(Into::into)
            }
        }
    }

    pub fn update_llm(&self, llm: LlmConfig) -> TalosResult<LlmConfig> {
        let new_llm = self.config_store.save_llm(&llm)?;
        let reloaded = self.reload_after_update(ConfigValue::Llm(Box::new(new_llm.clone())))?;
        self.reload_models(reloaded.as_ref().map_or(&new_llm, |config| &config.llm))?;
        self.config_store.load_llm_raw()
    }

    pub fn delete_config(&self, request: ConfigDeleteRequest) -> TalosResult<ConfigDeleteResponse> {
        Self::validate_delete_path(&request.path)?;
        Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "config delete is not supported for llm".to_string(),
        ))
    }

    pub fn reload_config(
        &self,
        _request: ConfigReloadRequest,
    ) -> TalosResult<ConfigReloadResponse> {
        Ok(ConfigReloadResponse { success: true })
    }

    fn reload_after_update(&self, value: ConfigValue) -> TalosResult<Option<Config>> {
        self.reload_hook
            .as_ref()
            .map(|reload_hook| reload_hook(value))
            .transpose()
    }

    fn reload_models(&self, new_config: &LlmConfig) -> TalosResult<()> {
        let new_catalog = resolve_model_catalog(new_config);
        *self
            .models
            .try_write()
            .map_err(|_| TalosError::internal("models lock poisoned".to_string()))? = new_catalog;
        Ok(())
    }

    fn validate_delete_path(path: &str) -> TalosResult<()> {
        match path {
            "" | "llm" | "webSearch" => Ok(()),
            other => Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!("unsupported config path: {}", other),
            )),
        }
    }
}

fn validate_web_search_config(web_search: &WebSearchToolConfig) -> TalosResult<()> {
    validate_web_search_provider_config("firecrawl", &web_search.firecrawl)?;
    validate_web_search_provider_config("brave", &web_search.brave)?;
    validate_web_search_provider_config("tavily", &web_search.tavily)?;
    Ok(())
}

fn validate_web_search_provider_config(
    provider_name: &str,
    provider: &WebSearchProviderRuntimeConfig,
) -> TalosResult<()> {
    if let Some(base_url) = provider.base_url.as_deref() {
        validate_web_search_base_url(provider_name, base_url)?;
    }
    if provider.timeout_ms == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            format!("web search provider `{provider_name}` timeout_ms must be greater than 0"),
        ));
    }
    Ok(())
}

fn validate_web_search_base_url(provider_name: &str, base_url: &str) -> TalosResult<()> {
    let base_url = base_url.trim();
    let has_http_authority_prefix =
        base_url.starts_with("http://") || base_url.starts_with("https://");
    let has_non_empty_authority = base_url
        .split_once("://")
        .and_then(|(_, authority_and_path)| authority_and_path.chars().next())
        .is_some_and(|first| !matches!(first, '/' | '?' | '#'));
    let parsed = url::Url::parse(base_url).map_err(|_| {
        TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            format!("web search provider `{provider_name}` base_url must be a valid HTTP(S) URL"),
        )
    })?;
    let scheme_allowed = matches!(parsed.scheme(), "http" | "https");
    if has_http_authority_prefix
        && has_non_empty_authority
        && scheme_allowed
        && parsed.host_str().is_some()
    {
        return Ok(());
    }
    Err(TalosError::public(
        talos_error::ErrorCode::InvalidRequest,
        format!("web search provider `{provider_name}` base_url must be a valid HTTP(S) URL"),
    ))
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
