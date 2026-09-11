use std::sync::Arc;

use talos_config::{
    provider_config_for_llm, ConfigStore, LlmConfig, WebSearchProviderRuntimeConfig,
    WebSearchToolConfig,
};
use talos_error::{TalosError, TalosResult};
use talos_store::session::SessionStore;
use talos_types::{
    resolve_default_model_selection, resolve_model_catalog, ConfigDeleteRequest,
    ConfigDeleteResponse, ConfigGetRequest, ConfigPath, ConfigReloadRequest, ConfigReloadResponse,
    ConfigResponse, ConfigUpdateRequest, ConfigValue, ModelInfo, ModelListResponse, ModelRef,
    ModelSelection,
};
use tokio::sync::RwLock;

use super::types::{ConfigReloadHook, ConfigReloadPlan, ConfigReloadTrigger};

#[derive(Clone)]
pub struct ConfigService {
    config_store: Arc<dyn ConfigStore>,
    models: Arc<RwLock<Vec<ModelInfo>>>,
    session_store: Arc<dyn SessionStore>,
    execution_gate: Arc<RwLock<()>>,
    reload_hook: ConfigReloadHook,
}

impl ConfigService {
    /// The caller holds the shared execution gate across private credentials and this commit.
    pub(crate) fn update_mcp_credentials(
        &self,
        previous: &talos_config::McpServerConfig,
        next: &talos_config::McpServerConfig,
    ) -> TalosResult<()> {
        let prepared =
            self.prepare_reload(ConfigReloadTrigger::McpCredentials(Box::new(next.clone())))?;
        if let Err(error) = self.config_store.save_mcp_server(previous, next) {
            // A successful rename followed by fsync failure must still publish the persisted candidate.
            if self
                .config_store
                .load_mcp_servers()
                .is_ok_and(|servers| servers.iter().any(|server| server == next))
            {
                prepared.publish();
            }
            return Err(error);
        }
        prepared.publish();
        Ok(())
    }

    pub fn new(
        config_store: Arc<dyn ConfigStore>,
        models: Vec<ModelInfo>,
        session_store: Arc<dyn SessionStore>,
        execution_gate: Arc<RwLock<()>>,
        reload_hook: ConfigReloadHook,
    ) -> Self {
        Self {
            config_store,
            models: Arc::new(RwLock::new(models)),
            session_store,
            execution_gate,
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
            return Err(TalosError::new(
                talos_error::ErrorCode::ProviderUnavailable,
                format!("{}:{}", model_ref.provider, model_ref.id),
            ));
        }
        let llm = self.config_store.load_llm_raw()?;
        let Some(provider) = provider_config_for_llm(&llm, &model_ref.provider) else {
            return Err(TalosError::new(
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
        Err(TalosError::new(
            talos_error::ErrorCode::ProviderUnavailable,
            format!("{}:{}", model_ref.provider, model_ref.id),
        ))
    }

    pub fn get_llm(&self) -> TalosResult<LlmConfig> {
        self.config_store.load_llm_raw()
    }

    pub fn get_config(&self, request: ConfigGetRequest) -> TalosResult<ConfigResponse> {
        match request.path {
            ConfigPath::Llm => self.config_store.load_llm_read().map(Into::into),
            ConfigPath::WebSearch => self.config_store.load_web_search_read().map(Into::into),
        }
    }

    pub async fn update_config(&self, request: ConfigUpdateRequest) -> TalosResult<ConfigResponse> {
        let _execution_guard = self.execution_gate.write().await;
        self.reject_nonterminal_turns().await?;
        match request.value {
            ConfigValue::Llm(llm) => {
                let llm = *llm;
                let prepared = self.prepare_reload(ConfigReloadTrigger::Updated(
                    ConfigValue::Llm(Box::new(llm.clone())),
                ))?;
                self.config_store.save_llm(&llm)?;
                let reloaded = prepared.publish();
                self.reload_models(&reloaded.llm).await;
                self.config_store.load_llm_read().map(Into::into)
            }
            ConfigValue::WebSearch(web_search) => {
                validate_web_search_config(&web_search)?;
                let prepared = self.prepare_reload(ConfigReloadTrigger::Updated(
                    ConfigValue::WebSearch(web_search.clone()),
                ))?;
                self.config_store.save_web_search(&web_search)?;
                prepared.publish();
                self.config_store.load_web_search_read().map(Into::into)
            }
        }
    }

    pub fn delete_config(&self, request: ConfigDeleteRequest) -> TalosResult<ConfigDeleteResponse> {
        Self::validate_delete_path(&request.path)?;
        Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "config delete is not supported for llm".to_string(),
        ))
    }

    pub async fn reload_config(
        &self,
        _request: ConfigReloadRequest,
    ) -> TalosResult<ConfigReloadResponse> {
        let _execution_guard = self.execution_gate.write().await;
        self.reject_nonterminal_turns().await?;
        let config = self
            .prepare_reload(ConfigReloadTrigger::Explicit)?
            .publish();
        self.reload_models(&config.llm).await;
        Ok(ConfigReloadResponse { success: true })
    }

    async fn reject_nonterminal_turns(&self) -> TalosResult<()> {
        if self.session_store.has_nonterminal_turns().await? {
            return Err(TalosError::new(
                talos_error::ErrorCode::Conflict,
                "configuration cannot reload while an Agent Turn is non-terminal",
            ));
        }
        Ok(())
    }

    /// Builds the composition-root replacement before config persistence.
    fn prepare_reload(&self, trigger: ConfigReloadTrigger) -> TalosResult<ConfigReloadPlan> {
        (self.reload_hook)(trigger)
    }

    async fn reload_models(&self, new_config: &LlmConfig) {
        let new_catalog = resolve_model_catalog(new_config);
        *self.models.write().await = new_catalog;
    }

    fn validate_delete_path(path: &str) -> TalosResult<()> {
        match path {
            "" | "llm" | "webSearch" => Ok(()),
            other => Err(TalosError::new(
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
        return Err(TalosError::new(
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
        TalosError::new(
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
    Err(TalosError::new(
        talos_error::ErrorCode::InvalidRequest,
        format!("web search provider `{provider_name}` base_url must be a valid HTTP(S) URL"),
    ))
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
