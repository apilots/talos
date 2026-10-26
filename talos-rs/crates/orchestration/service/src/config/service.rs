use std::sync::Arc;

use talos_config::{ConfigStore, WebSearchProviderRuntimeConfig, WebSearchToolConfig};
use talos_error::{TalosError, TalosResult};
use talos_types::{
    ConfigDeleteRequest, ConfigDeleteResponse, ConfigGetRequest, ConfigPath, ConfigReloadRequest,
    ConfigReloadResponse, ConfigResponse, ConfigUpdateRequest, ConfigValue,
};
use tokio::sync::RwLock;

use super::types::{ConfigReloadHook, ConfigReloadPlan, ConfigReloadTrigger};

#[derive(Clone)]
pub struct ConfigService {
    config_store: Arc<dyn ConfigStore>,
    execution_gate: Arc<RwLock<()>>,
    reload_hook: ConfigReloadHook,
}

impl ConfigService {
    pub fn new(
        config_store: Arc<dyn ConfigStore>,
        execution_gate: Arc<RwLock<()>>,
        reload_hook: ConfigReloadHook,
    ) -> Self {
        Self {
            config_store,
            execution_gate,
            reload_hook,
        }
    }

    pub fn get_config(&self, request: ConfigGetRequest) -> TalosResult<ConfigResponse> {
        match request.path {
            ConfigPath::Llm => self.config_store.load_llm_read().map(Into::into),
            ConfigPath::WebSearch => self.config_store.load_web_search_read().map(Into::into),
        }
    }

    pub async fn update_config(&self, request: ConfigUpdateRequest) -> TalosResult<ConfigResponse> {
        let _execution_guard = self.execution_gate.clone().try_write_owned().map_err(|_| {
            TalosError::new(
                talos_error::ErrorCode::Conflict,
                "Worker configuration cannot change while execution is active",
            )
        })?;
        match request.value {
            ConfigValue::Llm(llm) => {
                let llm = *llm;
                let prepared = self.prepare_reload(ConfigReloadTrigger::Updated(
                    ConfigValue::Llm(Box::new(llm.clone())),
                ))?;
                self.config_store.save_llm(&llm)?;
                prepared.publish();
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
        let _execution_guard = self.execution_gate.clone().try_write_owned().map_err(|_| {
            TalosError::new(
                talos_error::ErrorCode::Conflict,
                "Worker configuration cannot change while execution is active",
            )
        })?;
        self.prepare_reload(ConfigReloadTrigger::Explicit)?
            .publish();
        Ok(ConfigReloadResponse { success: true })
    }

    /// Builds the composition-root replacement before config persistence.
    fn prepare_reload(&self, trigger: ConfigReloadTrigger) -> TalosResult<ConfigReloadPlan> {
        (self.reload_hook)(trigger)
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
