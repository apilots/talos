use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use talos_error::{TalosError, TalosResult};

use crate::{
    model_readiness_for_llm, EnvSecretStore, LlmConfig, LlmConfigRead, ProviderConfig,
    ProviderKeyStatus, RuntimeEnv,
};

const ANTHROPIC_API_KEY_ENV: &str = "TALOS_LLM_ANTHROPIC_API_KEY";
const OPENAI_API_KEY_ENV: &str = "TALOS_LLM_OPENAI_API_KEY";
const BAILIAN_API_KEY_ENV: &str = "TALOS_LLM_BAILIAN_API_KEY";
const OLLAMA_API_KEY_ENV: &str = "TALOS_LLM_OLLAMA_API_KEY";
const OPENROUTER_API_KEY_ENV: &str = "TALOS_LLM_OPENROUTER_API_KEY";
const REDACTED_API_KEY: &str = "********";

pub trait LlmConfigStore: Send + Sync {
    /// Read the llm: section from yaml without expanding ${VAR} placeholders.
    fn load_raw(&self) -> TalosResult<LlmConfig>;

    /// Read the UI-facing llm config with non-sensitive key status.
    fn load_read(&self) -> TalosResult<LlmConfigRead>;

    /// Replace only the llm: node in the yaml file; all other sections are preserved as-is.
    fn save(&self, llm: &LlmConfig) -> TalosResult<LlmConfig>;
}

pub struct YamlLlmConfigStore {
    config_path: PathBuf,
    env_path: PathBuf,
}

impl YamlLlmConfigStore {
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

impl LlmConfigStore for YamlLlmConfigStore {
    fn load_raw(&self) -> TalosResult<LlmConfig> {
        let text = std::fs::read_to_string(&self.config_path).map_err(|e| {
            TalosError::ConfigError(format!(
                "failed to read config file {}: {}",
                self.config_path.display(),
                e
            ))
        })?;

        let root: serde_yaml::Value = serde_yaml::from_str(&text)
            .map_err(|e| TalosError::ConfigError(format!("failed to parse yaml: {}", e)))?;

        let llm_node = root
            .get("llm")
            .cloned()
            .unwrap_or(serde_yaml::Value::Mapping(Default::default()));

        let mut llm: LlmConfig = serde_yaml::from_value(llm_node).map_err(|e| {
            TalosError::ConfigError(format!("failed to deserialize llm config: {}", e))
        })?;
        sanitize_llm_for_read(&mut llm);
        Ok(llm)
    }

    fn load_read(&self) -> TalosResult<LlmConfigRead> {
        let raw = self.load_raw_unsanitized()?;
        let runtime_env = RuntimeEnv::load(&self.env_path)?;
        Ok(build_llm_config_read(raw, &runtime_env))
    }

    fn save(&self, llm: &LlmConfig) -> TalosResult<LlmConfig> {
        let text = std::fs::read_to_string(&self.config_path).map_err(|e| {
            TalosError::ConfigError(format!(
                "failed to read config file {}: {}",
                self.config_path.display(),
                e
            ))
        })?;

        let mut root: serde_yaml::Value = serde_yaml::from_str(&text)
            .map_err(|e| TalosError::ConfigError(format!("failed to parse yaml: {}", e)))?;

        let existing_llm = self.load_raw_unsanitized()?;
        let mut safe_llm = llm.clone();
        persist_provider_secret(
            &self.env_path,
            ANTHROPIC_API_KEY_ENV,
            true,
            &mut safe_llm.anthropic,
            Some(&existing_llm.anthropic),
        )?;
        persist_provider_secret(
            &self.env_path,
            OPENAI_API_KEY_ENV,
            true,
            &mut safe_llm.openai,
            Some(&existing_llm.openai),
        )?;
        persist_provider_secret(
            &self.env_path,
            BAILIAN_API_KEY_ENV,
            true,
            &mut safe_llm.bailian,
            Some(&existing_llm.bailian),
        )?;
        persist_provider_secret(
            &self.env_path,
            OLLAMA_API_KEY_ENV,
            false,
            &mut safe_llm.ollama,
            Some(&existing_llm.ollama),
        )?;
        persist_provider_secret(
            &self.env_path,
            OPENROUTER_API_KEY_ENV,
            true,
            &mut safe_llm.openrouter,
            Some(&existing_llm.openrouter),
        )?;

        let llm_value = serde_yaml::to_value(&safe_llm).map_err(|e| {
            TalosError::ConfigError(format!("failed to serialize llm config: {}", e))
        })?;

        match &mut root {
            serde_yaml::Value::Mapping(map) => {
                map.insert(serde_yaml::Value::String("llm".to_string()), llm_value);
            }
            _ => {
                return Err(TalosError::ConfigError(
                    "config file root is not a yaml mapping".to_string(),
                ));
            }
        }

        let output = serde_yaml::to_string(&root)
            .map_err(|e| TalosError::ConfigError(format!("failed to serialize yaml: {}", e)))?;

        std::fs::write(&self.config_path, output).map_err(|e| {
            TalosError::ConfigError(format!(
                "failed to write config file {}: {}",
                self.config_path.display(),
                e
            ))
        })?;

        sanitize_llm_for_read(&mut safe_llm);
        Ok(safe_llm)
    }
}

impl YamlLlmConfigStore {
    fn load_raw_unsanitized(&self) -> TalosResult<LlmConfig> {
        let text = std::fs::read_to_string(&self.config_path).map_err(|e| {
            TalosError::ConfigError(format!(
                "failed to read config file {}: {}",
                self.config_path.display(),
                e
            ))
        })?;

        let root: serde_yaml::Value = serde_yaml::from_str(&text)
            .map_err(|e| TalosError::ConfigError(format!("failed to parse yaml: {}", e)))?;

        let llm_node = root
            .get("llm")
            .cloned()
            .unwrap_or(serde_yaml::Value::Mapping(Default::default()));

        serde_yaml::from_value(llm_node).map_err(|e| {
            TalosError::ConfigError(format!("failed to deserialize llm config: {}", e))
        })
    }
}

fn persist_provider_secret(
    env_path: &PathBuf,
    env_key: &str,
    keep_placeholder: bool,
    provider: &mut ProviderConfig,
    existing_provider: Option<&ProviderConfig>,
) -> TalosResult<()> {
    let store = EnvSecretStore::new(env_path.clone());
    match provider.api_key.as_deref().map(str::trim) {
        Some(value) if value.is_empty() => {
            store.remove(env_key)?;
            provider.api_key = keep_placeholder.then(|| placeholder(env_key));
        }
        Some(value) if is_redacted_api_key(value) || is_env_placeholder(value) => {
            migrate_existing_plaintext_secret(env_path, env_key, existing_provider)?;
            provider.api_key = Some(placeholder(env_key));
        }
        Some(value) => {
            store.set(env_key, value)?;
            provider.api_key = Some(placeholder(env_key));
        }
        None => {
            migrate_existing_plaintext_secret(env_path, env_key, existing_provider)?;
            provider.api_key = keep_placeholder.then(|| placeholder(env_key));
        }
    }
    Ok(())
}

fn migrate_existing_plaintext_secret(
    env_path: &PathBuf,
    env_key: &str,
    existing_provider: Option<&ProviderConfig>,
) -> TalosResult<()> {
    let runtime_env = RuntimeEnv::load(env_path)?;
    if runtime_env
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

    EnvSecretStore::new(env_path.clone()).set(env_key, existing_value)
}

fn sanitize_llm_for_read(llm: &mut LlmConfig) {
    sanitize_provider_for_read(&mut llm.anthropic);
    sanitize_provider_for_read(&mut llm.openai);
    sanitize_provider_for_read(&mut llm.bailian);
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

fn apply_provider_api_key_displays(
    llm: &mut LlmConfig,
    provider_keys: &BTreeMap<String, ProviderKeyStatus>,
) {
    apply_provider_api_key_display(&mut llm.anthropic, provider_keys.get("anthropic"));
    apply_provider_api_key_display(&mut llm.openai, provider_keys.get("openai"));
    apply_provider_api_key_display(&mut llm.bailian, provider_keys.get("bailian"));
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
        Some(value) if value.is_empty() => false,
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

pub struct InMemoryLlmConfigStore {
    llm: std::sync::RwLock<LlmConfig>,
}

impl InMemoryLlmConfigStore {
    pub fn new(llm: LlmConfig) -> Self {
        Self {
            llm: std::sync::RwLock::new(llm),
        }
    }
}

impl LlmConfigStore for InMemoryLlmConfigStore {
    fn load_raw(&self) -> TalosResult<LlmConfig> {
        Ok(self.llm.read().expect("llm config read lock").clone())
    }

    fn load_read(&self) -> TalosResult<LlmConfigRead> {
        let llm = self.load_raw()?;
        Ok(LlmConfigRead {
            model_readiness: model_readiness_for_llm(&llm),
            config: llm,
            provider_keys: BTreeMap::new(),
        })
    }

    fn save(&self, llm: &LlmConfig) -> TalosResult<LlmConfig> {
        *self.llm.write().expect("llm config write lock") = llm.clone();
        Ok(llm.clone())
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
mod tests {
    use super::*;
    use crate::ProviderModelConfig;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_yaml(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    fn minimal_yaml_with_llm(api_key: &str) -> String {
        format!(
            r#"gateway:
  port: 8080
llm:
  provider: anthropic
  model: claude-3-5-sonnet
  anthropic:
    api_key: {api_key}
    enabled: true
    models: []
  openai:
    enabled: false
    models: []
  bailian:
    enabled: false
    models: []
  ollama:
    enabled: false
    models: []
  openrouter:
    enabled: false
    models: []
"#
        )
    }

    fn store_for(file: &NamedTempFile) -> YamlLlmConfigStore {
        YamlLlmConfigStore::with_env_path(
            file.path().to_path_buf(),
            file.path().with_extension("env"),
        )
    }

    #[test]
    fn test_load_raw_sanitizes_env_placeholders() {
        let file = write_yaml(&minimal_yaml_with_llm("${TALOS_LLM_ANTHROPIC_API_KEY}"));
        let store = store_for(&file);
        let llm = store.load_raw().unwrap();
        assert_eq!(llm.anthropic.api_key.as_deref(), None);
    }

    #[test]
    fn test_load_read_reports_missing_placeholder_secret() {
        let file = write_yaml(&minimal_yaml_with_llm("${TALOS_LLM_ANTHROPIC_API_KEY}"));
        let store = store_for(&file);

        let read = store.load_read().unwrap();

        assert_eq!(read.config.anthropic.api_key.as_deref(), None);
        assert!(!read.model_readiness.ready);
        assert!(!read.provider_keys["anthropic"].api_key_configured);
    }

    #[test]
    fn test_load_read_reports_configured_placeholder_secret() {
        let file = write_yaml(&minimal_yaml_with_llm("${TALOS_LLM_ANTHROPIC_API_KEY}"));
        let env_path = file.path().with_extension("env");
        std::fs::write(&env_path, "TALOS_LLM_ANTHROPIC_API_KEY=sk-secret\n").unwrap();
        let store = YamlLlmConfigStore::with_env_path(file.path().to_path_buf(), env_path);

        let read = store.load_read().unwrap();

        assert_eq!(
            read.config.anthropic.api_key.as_deref(),
            Some(REDACTED_API_KEY)
        );
        assert!(read.model_readiness.ready);
        assert!(read.provider_keys["anthropic"].api_key_configured);
    }

    #[test]
    fn test_save_only_replaces_llm_section() {
        let file = write_yaml(&minimal_yaml_with_llm("${TALOS_LLM_ANTHROPIC_API_KEY}"));
        let store = store_for(&file);

        let mut llm = store.load_raw().unwrap();
        llm.model = "new-model".to_string();
        store.save(&llm).unwrap();

        let raw = std::fs::read_to_string(file.path()).unwrap();
        assert!(
            raw.contains("port: 8080"),
            "gateway section must be preserved"
        );
        assert!(
            raw.contains("gateway:"),
            "gateway section header must be preserved"
        );
        assert!(
            raw.contains("new-model"),
            "updated model name must be present"
        );
    }

    #[test]
    fn test_save_then_load_raw_roundtrip() {
        let file = write_yaml(&minimal_yaml_with_llm("sk-literal-key"));
        let store = store_for(&file);

        let mut llm = store.load_raw().unwrap();
        llm.provider = "bailian".to_string();
        llm.model = "qwen-max".to_string();
        llm.anthropic.models.push(ProviderModelConfig {
            id: "test-model".to_string(),
            name: "Test Model".to_string(),
            context_window: 32768,
            reasoning: false,
            reasoning_effort: Default::default(),
            speed: Default::default(),
            input: vec![],
            cost: Default::default(),
            compat: Default::default(),
        });
        store.save(&llm).unwrap();

        let loaded = store.load_raw().unwrap();
        assert_eq!(loaded.provider, "bailian");
        assert_eq!(loaded.model, "qwen-max");
        assert_eq!(loaded.anthropic.models.len(), 1);
        assert_eq!(loaded.anthropic.models[0].id, "test-model");
    }

    #[test]
    fn test_save_adds_llm_section_when_missing() {
        let file = write_yaml("gateway:\n  port: 8080\n");
        let store = store_for(&file);
        let llm = LlmConfig::default();
        store.save(&llm).unwrap();
        let loaded = store.load_raw().unwrap();
        assert_eq!(loaded.provider, llm.provider);
    }

    #[test]
    fn test_save_writes_api_key_to_env_and_keeps_yaml_placeholder() {
        let file = write_yaml(&minimal_yaml_with_llm("${TALOS_LLM_ANTHROPIC_API_KEY}"));
        let env_path = file.path().with_extension("env");
        let store = YamlLlmConfigStore::with_env_path(file.path().to_path_buf(), env_path.clone());
        let mut llm = store.load_raw().unwrap();
        llm.anthropic.api_key = Some("sk-secret".to_string());

        let saved = store.save(&llm).unwrap();

        assert_eq!(saved.anthropic.api_key.as_deref(), None);
        let env = std::fs::read_to_string(&env_path).unwrap();
        assert!(env.contains("TALOS_LLM_ANTHROPIC_API_KEY=sk-secret"));
        let yaml = std::fs::read_to_string(file.path()).unwrap();
        assert!(yaml.contains("api_key: ${TALOS_LLM_ANTHROPIC_API_KEY}"));
        assert!(!yaml.contains("sk-secret"));
    }

    #[test]
    fn test_save_migrates_existing_plaintext_api_key_when_ui_omits_secret() {
        let file = write_yaml(&minimal_yaml_with_llm("sk-existing"));
        let env_path = file.path().with_extension("env");
        let store = YamlLlmConfigStore::with_env_path(file.path().to_path_buf(), env_path.clone());
        let mut llm = store.load_raw().unwrap();
        llm.model = "new-model".to_string();

        let saved = store.save(&llm).unwrap();

        assert_eq!(saved.anthropic.api_key.as_deref(), None);
        let env = std::fs::read_to_string(&env_path).unwrap();
        assert!(env.contains("TALOS_LLM_ANTHROPIC_API_KEY=sk-existing"));
        let yaml = std::fs::read_to_string(file.path()).unwrap();
        assert!(yaml.contains("api_key: ${TALOS_LLM_ANTHROPIC_API_KEY}"));
        assert!(!yaml.contains("sk-existing"));
    }

    #[test]
    fn test_save_empty_api_key_clears_existing_env_secret() {
        let file = write_yaml(&minimal_yaml_with_llm("${TALOS_LLM_ANTHROPIC_API_KEY}"));
        let env_path = file.path().with_extension("env");
        std::fs::write(&env_path, "TALOS_LLM_ANTHROPIC_API_KEY=sk-secret\n").unwrap();
        let store = YamlLlmConfigStore::with_env_path(file.path().to_path_buf(), env_path.clone());
        let mut llm = store.load_raw().unwrap();
        llm.anthropic.api_key = Some(String::new());

        store.save(&llm).unwrap();

        let env = std::fs::read_to_string(&env_path).unwrap();
        assert!(!env.contains("TALOS_LLM_ANTHROPIC_API_KEY="));
    }

    #[test]
    fn test_save_redacted_api_key_keeps_existing_env_secret() {
        let file = write_yaml(&minimal_yaml_with_llm("${TALOS_LLM_ANTHROPIC_API_KEY}"));
        let env_path = file.path().with_extension("env");
        std::fs::write(&env_path, "TALOS_LLM_ANTHROPIC_API_KEY=sk-secret\n").unwrap();
        let store = YamlLlmConfigStore::with_env_path(file.path().to_path_buf(), env_path.clone());
        let mut read = store.load_read().unwrap();
        read.config.model = "new-model".to_string();

        store.save(&read.config).unwrap();

        let env = std::fs::read_to_string(&env_path).unwrap();
        assert!(env.contains("TALOS_LLM_ANTHROPIC_API_KEY=sk-secret"));
        assert!(!env.contains("TALOS_LLM_ANTHROPIC_API_KEY=********"));
    }

    #[test]
    fn test_in_memory_store_roundtrip() {
        let initial = LlmConfig::default();
        let store = InMemoryLlmConfigStore::new(initial.clone());
        let mut llm = store.load_raw().unwrap();
        llm.model = "custom-model".to_string();
        store.save(&llm).unwrap();
        let loaded = store.load_raw().unwrap();
        assert_eq!(loaded.model, "custom-model");
    }
}
