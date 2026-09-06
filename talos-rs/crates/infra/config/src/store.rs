use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use talos_error::{TalosError, TalosResult};
use talos_fs::FileSystem;

use crate::{
    model_readiness_for_llm, EnvSecretStore, LlmConfig, LlmConfigRead, ProviderConfig,
    ProviderKeyStatus, RuntimeEnv, WebSearchProviderRuntimeConfig, WebSearchToolConfig,
};

const ANTHROPIC_API_KEY_ENV: &str = "TALOS_LLM_ANTHROPIC_API_KEY";
const OPENAI_API_KEY_ENV: &str = "TALOS_LLM_OPENAI_API_KEY";
const BAILIAN_API_KEY_ENV: &str = "TALOS_LLM_BAILIAN_API_KEY";
const DEEPSEEK_API_KEY_ENV: &str = "TALOS_LLM_DEEPSEEK_API_KEY";
const OLLAMA_API_KEY_ENV: &str = "TALOS_LLM_OLLAMA_API_KEY";
const OPENROUTER_API_KEY_ENV: &str = "TALOS_LLM_OPENROUTER_API_KEY";
const WEB_SEARCH_FIRECRAWL_API_KEY_ENV: &str = "TALOS_WEB_SEARCH_FIRECRAWL_API_KEY";
const WEB_SEARCH_BRAVE_API_KEY_ENV: &str = "TALOS_WEB_SEARCH_BRAVE_API_KEY";
const WEB_SEARCH_TAVILY_API_KEY_ENV: &str = "TALOS_WEB_SEARCH_TAVILY_API_KEY";
const REDACTED_API_KEY: &str = "********";

/// Stores the editable Talos configuration sections owned by the config layer.
///
/// Implementations must preserve unrelated YAML sections and keep API keys in
/// the runtime env file instead of the YAML config file.
pub trait ConfigStore: Send + Sync {
    /// Read the llm: section from yaml without expanding ${VAR} placeholders.
    fn load_llm_raw(&self) -> TalosResult<LlmConfig>;

    /// Read the UI-facing llm config with non-sensitive key status.
    fn load_llm_read(&self) -> TalosResult<LlmConfigRead>;

    /// Replace only the llm: node in the yaml file; all other sections are preserved as-is.
    fn save_llm(&self, llm: &LlmConfig) -> TalosResult<LlmConfig>;

    /// Read the UI-facing web search config with API keys redacted.
    fn load_web_search_read(&self) -> TalosResult<WebSearchToolConfig>;

    /// Replace only agent.kernel.tools.web_search and persist provider API keys to env.
    fn save_web_search(&self, web_search: &WebSearchToolConfig)
        -> TalosResult<WebSearchToolConfig>;
}

pub trait LlmConfigStore: Send + Sync {
    /// Read the llm: section from yaml without expanding ${VAR} placeholders.
    fn load_raw(&self) -> TalosResult<LlmConfig>;

    /// Read the UI-facing llm config with non-sensitive key status.
    fn load_read(&self) -> TalosResult<LlmConfigRead>;

    /// Replace only the llm: node in the yaml file; all other sections are preserved as-is.
    fn save(&self, llm: &LlmConfig) -> TalosResult<LlmConfig>;
}

impl<S: ConfigStore + ?Sized> LlmConfigStore for S {
    fn load_raw(&self) -> TalosResult<LlmConfig> {
        self.load_llm_raw()
    }

    fn load_read(&self) -> TalosResult<LlmConfigRead> {
        self.load_llm_read()
    }

    fn save(&self, llm: &LlmConfig) -> TalosResult<LlmConfig> {
        self.save_llm(llm)
    }
}

pub struct YamlConfigStore {
    config_path: PathBuf,
    env_path: PathBuf,
}

impl YamlConfigStore {
    pub fn new(config_path: PathBuf) -> Self {
        let env_path = config_path
            .parent()
            .and_then(|conf_dir| conf_dir.parent())
            .map(|talos_home| talos_home.join(".env"))
            .unwrap_or_else(|| PathBuf::from(".env"));
        Self {
            config_path,
            env_path,
        }
    }

    pub fn with_env_path(config_path: PathBuf, env_path: PathBuf) -> Self {
        Self {
            config_path,
            env_path,
        }
    }
}

impl ConfigStore for YamlConfigStore {
    fn load_llm_raw(&self) -> TalosResult<LlmConfig> {
        let mut llm = self.load_llm_raw_unsanitized()?;
        sanitize_llm_for_read(&mut llm);
        Ok(llm)
    }

    fn load_llm_read(&self) -> TalosResult<LlmConfigRead> {
        let raw = self.load_llm_raw_unsanitized()?;
        let runtime_env = RuntimeEnv::load(&self.env_path)?;
        Ok(build_llm_config_read(raw, &runtime_env))
    }

    fn save_llm(&self, llm: &LlmConfig) -> TalosResult<LlmConfig> {
        let _lock = talos_fs::std_fs().with_file_lock(&self.lock_path())?;
        let text = self.read_config_text()?;
        let mut root = parse_yaml_root(&text)?;
        let existing_llm = load_llm_from_root_unsanitized(&root)?;
        let mut env_values = EnvSecretStore::new(self.env_path.clone()).read_filtered()?;
        let old_env_values = env_values.clone();
        let mut safe_llm = llm.clone();
        persist_llm_provider_secret(
            &mut env_values,
            ANTHROPIC_API_KEY_ENV,
            true,
            &mut safe_llm.anthropic,
            Some(&existing_llm.anthropic),
        )?;
        persist_llm_provider_secret(
            &mut env_values,
            OPENAI_API_KEY_ENV,
            true,
            &mut safe_llm.openai,
            Some(&existing_llm.openai),
        )?;
        persist_llm_provider_secret(
            &mut env_values,
            BAILIAN_API_KEY_ENV,
            true,
            &mut safe_llm.bailian,
            Some(&existing_llm.bailian),
        )?;
        persist_llm_provider_secret(
            &mut env_values,
            DEEPSEEK_API_KEY_ENV,
            true,
            &mut safe_llm.deepseek,
            Some(&existing_llm.deepseek),
        )?;
        persist_llm_provider_secret(
            &mut env_values,
            OLLAMA_API_KEY_ENV,
            false,
            &mut safe_llm.ollama,
            Some(&existing_llm.ollama),
        )?;
        persist_llm_provider_secret(
            &mut env_values,
            OPENROUTER_API_KEY_ENV,
            true,
            &mut safe_llm.openrouter,
            Some(&existing_llm.openrouter),
        )?;

        let llm_value = serde_yaml::to_value(&safe_llm)
            .map_err(|e| TalosError::internal(format!("failed to serialize llm config: {}", e)))?;

        replace_top_level_section(&mut root, "llm", llm_value)?;

        let output = serde_yaml::to_string(&root)
            .map_err(|e| TalosError::internal(format!("failed to serialize yaml: {}", e)))?;

        self.write_config_and_env(&text, &output, &old_env_values, &env_values)?;

        sanitize_llm_for_read(&mut safe_llm);
        Ok(safe_llm)
    }

    fn load_web_search_read(&self) -> TalosResult<WebSearchToolConfig> {
        let raw = self.load_web_search_raw_unsanitized()?;
        let runtime_env = RuntimeEnv::load(&self.env_path)?;
        Ok(build_web_search_config_read(raw, &runtime_env))
    }

    fn save_web_search(
        &self,
        web_search: &WebSearchToolConfig,
    ) -> TalosResult<WebSearchToolConfig> {
        let _lock = talos_fs::std_fs().with_file_lock(&self.lock_path())?;
        let text = self.read_config_text()?;
        let mut root = parse_yaml_root(&text)?;
        let existing_web_search = load_web_search_from_root_unsanitized(&root)?;
        let mut env_values = EnvSecretStore::new(self.env_path.clone()).read_filtered()?;
        let old_env_values = env_values.clone();
        let mut safe_web_search = web_search.clone();

        persist_web_search_provider_secret(
            &mut env_values,
            WEB_SEARCH_FIRECRAWL_API_KEY_ENV,
            &mut safe_web_search.firecrawl,
            Some(&existing_web_search.firecrawl),
        )?;
        persist_web_search_provider_secret(
            &mut env_values,
            WEB_SEARCH_BRAVE_API_KEY_ENV,
            &mut safe_web_search.brave,
            Some(&existing_web_search.brave),
        )?;
        persist_web_search_provider_secret(
            &mut env_values,
            WEB_SEARCH_TAVILY_API_KEY_ENV,
            &mut safe_web_search.tavily,
            Some(&existing_web_search.tavily),
        )?;

        let web_search_value = serde_yaml::to_value(&safe_web_search).map_err(|e| {
            TalosError::internal(format!("failed to serialize web search config: {}", e))
        })?;
        replace_web_search_section(&mut root, web_search_value)?;
        let output = serde_yaml::to_string(&root)
            .map_err(|e| TalosError::internal(format!("failed to serialize yaml: {}", e)))?;

        self.write_config_and_env(&text, &output, &old_env_values, &env_values)?;

        apply_web_search_api_key_displays_from_values(&mut safe_web_search, &env_values);
        Ok(safe_web_search)
    }
}

pub type YamlLlmConfigStore = YamlConfigStore;

impl YamlConfigStore {
    fn load_llm_raw_unsanitized(&self) -> TalosResult<LlmConfig> {
        let text = self.read_config_text()?;
        let root = parse_yaml_root(&text)?;
        load_llm_from_root_unsanitized(&root)
    }

    fn load_web_search_raw_unsanitized(&self) -> TalosResult<WebSearchToolConfig> {
        let text = self.read_config_text()?;
        let root = parse_yaml_root(&text)?;
        load_web_search_from_root_unsanitized(&root)
    }

    fn read_config_text(&self) -> TalosResult<String> {
        std::fs::read_to_string(&self.config_path).map_err(|e| {
            TalosError::internal(format!(
                "failed to read config file {}: {}",
                self.config_path.display(),
                e
            ))
        })
    }

    fn write_config_and_env(
        &self,
        old_config: &str,
        new_config: &str,
        old_env_values: &BTreeMap<String, String>,
        new_env_values: &BTreeMap<String, String>,
    ) -> TalosResult<()> {
        talos_fs::std_fs()
            .write_file(&self.config_path, new_config)
            .map_err(|e| {
                TalosError::internal(format!(
                    "failed to write config file {}: {}",
                    self.config_path.display(),
                    e
                ))
            })?;

        let env_store = EnvSecretStore::new(self.env_path.clone());
        match env_store.write_filtered(new_env_values) {
            Ok(()) => Ok(()),
            Err(env_error) => {
                let restore_yaml = talos_fs::std_fs().write_file(&self.config_path, old_config);
                let restore_env = env_store.write_filtered(old_env_values);
                Err(TalosError::internal(format!(
                    "failed to write env file {}; config path {}; rollback yaml: {}; rollback env: {}; error: {}",
                    self.env_path.display(),
                    self.config_path.display(),
                    rollback_status(restore_yaml),
                    rollback_status(restore_env),
                    env_error
                )))
            }
        }
    }

    fn lock_path(&self) -> PathBuf {
        self.config_path.with_extension("yaml.config.lock")
    }
}

fn parse_yaml_root(text: &str) -> TalosResult<serde_yaml::Value> {
    serde_yaml::from_str(text)
        .map_err(|e| TalosError::internal(format!("failed to parse yaml: {}", e)))
}

fn load_llm_from_root_unsanitized(root: &serde_yaml::Value) -> TalosResult<LlmConfig> {
    let llm_node = root
        .get("llm")
        .cloned()
        .unwrap_or(serde_yaml::Value::Mapping(Default::default()));

    serde_yaml::from_value(llm_node)
        .map_err(|e| TalosError::internal(format!("failed to deserialize llm config: {}", e)))
}

fn load_web_search_from_root_unsanitized(
    root: &serde_yaml::Value,
) -> TalosResult<WebSearchToolConfig> {
    let web_search_node = root
        .get("agent")
        .and_then(|agent| agent.get("kernel"))
        .and_then(|kernel| kernel.get("tools"))
        .and_then(|tools| tools.get("web_search"))
        .cloned()
        .unwrap_or(serde_yaml::Value::Mapping(Default::default()));

    serde_yaml::from_value(web_search_node).map_err(|e| {
        TalosError::internal(format!("failed to deserialize web search config: {}", e))
    })
}

fn replace_top_level_section(
    root: &mut serde_yaml::Value,
    key: &str,
    value: serde_yaml::Value,
) -> TalosResult<()> {
    match root {
        serde_yaml::Value::Mapping(map) => {
            map.insert(serde_yaml::Value::String(key.to_string()), value);
            Ok(())
        }
        _ => Err(TalosError::internal(
            "config file root is not a yaml mapping".to_string(),
        )),
    }
}

fn replace_web_search_section(
    root: &mut serde_yaml::Value,
    value: serde_yaml::Value,
) -> TalosResult<()> {
    let root_map = root.as_mapping_mut().ok_or_else(|| {
        TalosError::internal("config file root is not a yaml mapping".to_string())
    })?;
    let agent = nested_mapping(root_map, "agent")?;
    let kernel = nested_mapping(agent, "kernel")?;
    let tools = nested_mapping(kernel, "tools")?;
    tools.insert(serde_yaml::Value::String("web_search".to_string()), value);
    Ok(())
}

fn nested_mapping<'a>(
    map: &'a mut serde_yaml::Mapping,
    key: &str,
) -> TalosResult<&'a mut serde_yaml::Mapping> {
    let value = map
        .entry(serde_yaml::Value::String(key.to_string()))
        .or_insert_with(|| serde_yaml::Value::Mapping(Default::default()));
    value.as_mapping_mut().ok_or_else(|| {
        TalosError::internal(format!("config section '{}' is not a yaml mapping", key))
    })
}

fn persist_llm_provider_secret(
    env_values: &mut BTreeMap<String, String>,
    env_key: &str,
    keep_placeholder: bool,
    provider: &mut ProviderConfig,
    existing_provider: Option<&ProviderConfig>,
) -> TalosResult<()> {
    match provider.api_key.as_deref().map(str::trim) {
        Some("") => {
            env_values.remove(env_key);
            provider.api_key = keep_placeholder.then(|| placeholder(env_key));
        }
        Some(value) if is_redacted_api_key(value) || is_env_placeholder(value) => {
            migrate_existing_llm_plaintext_secret(env_values, env_key, existing_provider)?;
            provider.api_key = Some(placeholder(env_key));
        }
        Some(value) => {
            validate_env_key(env_key)?;
            env_values.insert(env_key.to_string(), value.to_string());
            provider.api_key = Some(placeholder(env_key));
        }
        None => {
            migrate_existing_llm_plaintext_secret(env_values, env_key, existing_provider)?;
            provider.api_key = keep_placeholder.then(|| placeholder(env_key));
        }
    }
    Ok(())
}

fn persist_web_search_provider_secret(
    env_values: &mut BTreeMap<String, String>,
    env_key: &str,
    provider: &mut WebSearchProviderRuntimeConfig,
    existing_provider: Option<&WebSearchProviderRuntimeConfig>,
) -> TalosResult<()> {
    validate_env_key(env_key)?;
    match provider.api_key.as_deref().map(str::trim) {
        Some("") => {
            env_values.remove(env_key);
        }
        Some(value) if is_redacted_api_key(value) || is_env_placeholder(value) => {
            migrate_existing_web_search_plaintext_secret(env_values, env_key, existing_provider);
        }
        Some(value) => {
            env_values.insert(env_key.to_string(), value.to_string());
        }
        None => {
            migrate_existing_web_search_plaintext_secret(env_values, env_key, existing_provider);
        }
    }
    provider.api_key = Some(placeholder(env_key));
    Ok(())
}

fn validate_env_key(env_key: &str) -> TalosResult<()> {
    if env_key.starts_with("TALOS_") {
        return Ok(());
    }
    Err(TalosError::internal(format!(
        "invalid talos env key '{}'",
        env_key
    )))
}

fn migrate_existing_llm_plaintext_secret(
    env_values: &mut BTreeMap<String, String>,
    env_key: &str,
    existing_provider: Option<&ProviderConfig>,
) -> TalosResult<()> {
    if env_values
        .get(env_key)
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Ok(());
    }

    let Some(existing_value) = existing_provider
        .and_then(|provider| provider.api_key.as_deref())
        .map(str::trim)
    else {
        return Ok(());
    };
    if existing_value.is_empty() || is_env_placeholder(existing_value) {
        return Ok(());
    }

    validate_env_key(env_key)?;
    env_values.insert(env_key.to_string(), existing_value.to_string());
    Ok(())
}

fn migrate_existing_web_search_plaintext_secret(
    env_values: &mut BTreeMap<String, String>,
    env_key: &str,
    existing_provider: Option<&WebSearchProviderRuntimeConfig>,
) {
    if env_values
        .get(env_key)
        .is_some_and(|value| !value.trim().is_empty())
    {
        return;
    }

    let Some(existing_value) = existing_provider
        .and_then(|provider| provider.api_key.as_deref())
        .map(str::trim)
    else {
        return;
    };
    if existing_value.is_empty() || is_env_placeholder(existing_value) {
        return;
    }

    env_values.insert(env_key.to_string(), existing_value.to_string());
}

fn sanitize_llm_for_read(llm: &mut LlmConfig) {
    sanitize_provider_for_read(&mut llm.anthropic);
    sanitize_provider_for_read(&mut llm.openai);
    sanitize_provider_for_read(&mut llm.bailian);
    sanitize_provider_for_read(&mut llm.deepseek);
    sanitize_provider_for_read(&mut llm.ollama);
    sanitize_provider_for_read(&mut llm.openrouter);
}

fn sanitize_provider_for_read(provider: &mut ProviderConfig) {
    if provider.api_key.is_some() {
        provider.api_key = None;
    }
}

fn build_llm_config_read(mut raw: LlmConfig, runtime_env: &RuntimeEnv) -> LlmConfigRead {
    let mut resolved = raw.clone();
    resolve_provider_api_key(&mut resolved.anthropic, ANTHROPIC_API_KEY_ENV, runtime_env);
    resolve_provider_api_key(&mut resolved.openai, OPENAI_API_KEY_ENV, runtime_env);
    resolve_provider_api_key(&mut resolved.bailian, BAILIAN_API_KEY_ENV, runtime_env);
    resolve_provider_api_key(&mut resolved.deepseek, DEEPSEEK_API_KEY_ENV, runtime_env);
    resolve_provider_api_key(&mut resolved.ollama, OLLAMA_API_KEY_ENV, runtime_env);
    resolve_provider_api_key(
        &mut resolved.openrouter,
        OPENROUTER_API_KEY_ENV,
        runtime_env,
    );

    let provider_keys = provider_key_statuses(&raw, runtime_env);
    sanitize_llm_for_read(&mut raw);
    apply_provider_api_key_displays(&mut raw, &provider_keys);
    LlmConfigRead {
        config: raw,
        model_readiness: model_readiness_for_llm(&resolved),
        provider_keys,
    }
}

fn build_web_search_config_read(
    mut raw: WebSearchToolConfig,
    runtime_env: &RuntimeEnv,
) -> WebSearchToolConfig {
    apply_web_search_api_key_display(
        &mut raw.firecrawl,
        WEB_SEARCH_FIRECRAWL_API_KEY_ENV,
        runtime_env,
    );
    apply_web_search_api_key_display(&mut raw.brave, WEB_SEARCH_BRAVE_API_KEY_ENV, runtime_env);
    apply_web_search_api_key_display(&mut raw.tavily, WEB_SEARCH_TAVILY_API_KEY_ENV, runtime_env);
    raw
}

fn apply_web_search_api_key_display(
    provider: &mut WebSearchProviderRuntimeConfig,
    env_key: &str,
    runtime_env: &RuntimeEnv,
) {
    provider.api_key = web_search_provider_key_configured(provider, env_key, runtime_env)
        .then(|| REDACTED_API_KEY.to_string());
}

fn apply_web_search_api_key_displays_from_values(
    web_search: &mut WebSearchToolConfig,
    env_values: &BTreeMap<String, String>,
) {
    apply_web_search_api_key_display_from_values(
        &mut web_search.firecrawl,
        WEB_SEARCH_FIRECRAWL_API_KEY_ENV,
        env_values,
    );
    apply_web_search_api_key_display_from_values(
        &mut web_search.brave,
        WEB_SEARCH_BRAVE_API_KEY_ENV,
        env_values,
    );
    apply_web_search_api_key_display_from_values(
        &mut web_search.tavily,
        WEB_SEARCH_TAVILY_API_KEY_ENV,
        env_values,
    );
}

fn apply_web_search_api_key_display_from_values(
    provider: &mut WebSearchProviderRuntimeConfig,
    env_key: &str,
    env_values: &BTreeMap<String, String>,
) {
    provider.api_key = env_values
        .get(env_key)
        .is_some_and(|value| !value.trim().is_empty())
        .then(|| REDACTED_API_KEY.to_string());
}

fn web_search_provider_key_configured(
    provider: &WebSearchProviderRuntimeConfig,
    env_key: &str,
    runtime_env: &RuntimeEnv,
) -> bool {
    match provider.api_key.as_deref().map(str::trim) {
        Some("") => false,
        Some(value) if is_env_placeholder(value) => runtime_env
            .get(env_key)
            .is_some_and(|value| !value.trim().is_empty()),
        Some(_) => true,
        None => runtime_env
            .get(env_key)
            .is_some_and(|value| !value.trim().is_empty()),
    }
}

fn apply_provider_api_key_displays(
    llm: &mut LlmConfig,
    provider_keys: &BTreeMap<String, ProviderKeyStatus>,
) {
    apply_provider_api_key_display(&mut llm.anthropic, provider_keys.get("anthropic"));
    apply_provider_api_key_display(&mut llm.openai, provider_keys.get("openai"));
    apply_provider_api_key_display(&mut llm.bailian, provider_keys.get("bailian"));
    apply_provider_api_key_display(&mut llm.deepseek, provider_keys.get("deepseek"));
    apply_provider_api_key_display(&mut llm.ollama, provider_keys.get("ollama"));
    apply_provider_api_key_display(&mut llm.openrouter, provider_keys.get("openrouter"));
}

fn apply_provider_api_key_display(
    provider: &mut ProviderConfig,
    key_status: Option<&ProviderKeyStatus>,
) {
    if key_status.is_some_and(|status| status.api_key_configured) {
        provider.api_key = Some(REDACTED_API_KEY.to_string());
    }
}

fn provider_key_statuses(
    llm: &LlmConfig,
    runtime_env: &RuntimeEnv,
) -> BTreeMap<String, ProviderKeyStatus> {
    [
        ("anthropic", ANTHROPIC_API_KEY_ENV, &llm.anthropic),
        ("openai", OPENAI_API_KEY_ENV, &llm.openai),
        ("bailian", BAILIAN_API_KEY_ENV, &llm.bailian),
        ("deepseek", DEEPSEEK_API_KEY_ENV, &llm.deepseek),
        ("ollama", OLLAMA_API_KEY_ENV, &llm.ollama),
        ("openrouter", OPENROUTER_API_KEY_ENV, &llm.openrouter),
    ]
    .into_iter()
    .map(|(provider, env_key, config)| {
        (
            provider.to_string(),
            ProviderKeyStatus {
                api_key_configured: provider_api_key_configured(config, env_key, runtime_env),
            },
        )
    })
    .collect()
}

fn provider_api_key_configured(
    provider: &ProviderConfig,
    env_key: &str,
    runtime_env: &RuntimeEnv,
) -> bool {
    match provider.api_key.as_deref().map(str::trim) {
        Some("") => false,
        Some(value) if is_env_placeholder(value) => runtime_env
            .get(env_key)
            .is_some_and(|value| !value.trim().is_empty()),
        Some(_) => true,
        None => false,
    }
}

fn resolve_provider_api_key(
    provider: &mut ProviderConfig,
    env_key: &str,
    runtime_env: &RuntimeEnv,
) {
    if provider.api_key.as_deref().is_some_and(is_env_placeholder) {
        provider.api_key = runtime_env.get(env_key).map(str::to_string);
    }
}

fn placeholder(env_key: &str) -> String {
    format!("${{{}}}", env_key)
}

fn is_env_placeholder(value: &str) -> bool {
    let value = value.trim();
    value.starts_with("${") && value.ends_with('}')
}

fn is_redacted_api_key(value: &str) -> bool {
    value.trim() == REDACTED_API_KEY
}

/// Resolves one LLM update into runtime credentials without persisting it.
pub fn resolve_llm_update_for_runtime(current: &LlmConfig, update: &mut LlmConfig) {
    preserve_provider_runtime_secret(&current.anthropic, &mut update.anthropic);
    preserve_provider_runtime_secret(&current.openai, &mut update.openai);
    preserve_provider_runtime_secret(&current.bailian, &mut update.bailian);
    preserve_provider_runtime_secret(&current.deepseek, &mut update.deepseek);
    preserve_provider_runtime_secret(&current.ollama, &mut update.ollama);
    preserve_provider_runtime_secret(&current.openrouter, &mut update.openrouter);
}

/// Resolves one Web Search update into runtime credentials without persisting it.
pub fn resolve_web_search_update_for_runtime(
    current: &WebSearchToolConfig,
    update: &mut WebSearchToolConfig,
) {
    preserve_web_search_runtime_secret(&current.firecrawl, &mut update.firecrawl);
    preserve_web_search_runtime_secret(&current.brave, &mut update.brave);
    preserve_web_search_runtime_secret(&current.tavily, &mut update.tavily);
}

fn preserve_provider_runtime_secret(current: &ProviderConfig, update: &mut ProviderConfig) {
    preserve_runtime_secret(&current.api_key, &mut update.api_key);
}

fn preserve_web_search_runtime_secret(
    current: &WebSearchProviderRuntimeConfig,
    update: &mut WebSearchProviderRuntimeConfig,
) {
    preserve_runtime_secret(&current.api_key, &mut update.api_key);
}

fn preserve_runtime_secret(current: &Option<String>, update: &mut Option<String>) {
    match update.as_deref().map(str::trim) {
        Some("") => *update = None,
        Some(value) if !is_redacted_api_key(value) && !is_env_placeholder(value) => {}
        Some(_) | None => *update = current.clone(),
    }
}

fn rollback_status(result: TalosResult<()>) -> &'static str {
    if result.is_ok() {
        "ok"
    } else {
        "failed"
    }
}

pub struct InMemoryLlmConfigStore {
    llm: std::sync::RwLock<LlmConfig>,
    web_search: std::sync::RwLock<WebSearchToolConfig>,
}

impl InMemoryLlmConfigStore {
    pub fn new(llm: LlmConfig) -> Self {
        Self {
            llm: std::sync::RwLock::new(llm),
            web_search: std::sync::RwLock::new(WebSearchToolConfig::default()),
        }
    }
}

impl ConfigStore for InMemoryLlmConfigStore {
    fn load_llm_raw(&self) -> TalosResult<LlmConfig> {
        Ok(self.llm.read().expect("llm config read lock").clone())
    }

    fn load_llm_read(&self) -> TalosResult<LlmConfigRead> {
        let llm = self.load_llm_raw()?;
        Ok(LlmConfigRead {
            model_readiness: model_readiness_for_llm(&llm),
            config: llm,
            provider_keys: BTreeMap::new(),
        })
    }

    fn save_llm(&self, llm: &LlmConfig) -> TalosResult<LlmConfig> {
        *self.llm.write().expect("llm config write lock") = llm.clone();
        Ok(llm.clone())
    }

    fn load_web_search_read(&self) -> TalosResult<WebSearchToolConfig> {
        Ok(self
            .web_search
            .read()
            .expect("web search config read lock")
            .clone())
    }

    fn save_web_search(
        &self,
        web_search: &WebSearchToolConfig,
    ) -> TalosResult<WebSearchToolConfig> {
        *self
            .web_search
            .write()
            .expect("web search config write lock") = web_search.clone();
        Ok(web_search.clone())
    }
}

impl<S: LlmConfigStore + ?Sized> LlmConfigStore for Arc<S> {
    fn load_raw(&self) -> TalosResult<LlmConfig> {
        (**self).load_raw()
    }

    fn load_read(&self) -> TalosResult<LlmConfigRead> {
        (**self).load_read()
    }

    fn save(&self, llm: &LlmConfig) -> TalosResult<LlmConfig> {
        (**self).save(llm)
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
