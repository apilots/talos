use std::path::PathBuf;
use std::sync::Arc;

use talos_error::{TalosError, TalosResult};

use crate::LlmConfig;

pub trait LlmConfigStore: Send + Sync {
    /// Read the llm: section from yaml without expanding ${VAR} placeholders.
    fn load_raw(&self) -> TalosResult<LlmConfig>;

    /// Replace only the llm: node in the yaml file; all other sections are preserved as-is.
    fn save(&self, llm: &LlmConfig) -> TalosResult<LlmConfig>;
}

pub struct YamlLlmConfigStore {
    config_path: PathBuf,
}

impl YamlLlmConfigStore {
    pub fn new(config_path: PathBuf) -> Self {
        Self { config_path }
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

        serde_yaml::from_value(llm_node).map_err(|e| {
            TalosError::ConfigError(format!("failed to deserialize llm config: {}", e))
        })
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

        let llm_value = serde_yaml::to_value(llm).map_err(|e| {
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

        Ok(llm.clone())
    }
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

    fn save(&self, llm: &LlmConfig) -> TalosResult<LlmConfig> {
        *self.llm.write().expect("llm config write lock") = llm.clone();
        Ok(llm.clone())
    }
}

impl<S: LlmConfigStore + ?Sized> LlmConfigStore for Arc<S> {
    fn load_raw(&self) -> TalosResult<LlmConfig> {
        (**self).load_raw()
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
"#
        )
    }

    #[test]
    fn test_load_raw_preserves_env_placeholders() {
        let file = write_yaml(&minimal_yaml_with_llm("${ANTHROPIC_API_KEY}"));
        let store = YamlLlmConfigStore::new(file.path().to_path_buf());
        let llm = store.load_raw().unwrap();
        assert_eq!(
            llm.anthropic.api_key.as_deref(),
            Some("${ANTHROPIC_API_KEY}")
        );
    }

    #[test]
    fn test_save_only_replaces_llm_section() {
        let file = write_yaml(&minimal_yaml_with_llm("${ANTHROPIC_API_KEY}"));
        let store = YamlLlmConfigStore::new(file.path().to_path_buf());

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
        let store = YamlLlmConfigStore::new(file.path().to_path_buf());

        let mut llm = store.load_raw().unwrap();
        llm.provider = "bailian".to_string();
        llm.model = "qwen-max".to_string();
        llm.anthropic.models.push(ProviderModelConfig {
            id: "test-model".to_string(),
            name: "Test Model".to_string(),
            context_window: 32768,
            reasoning: false,
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
        let store = YamlLlmConfigStore::new(file.path().to_path_buf());
        let llm = LlmConfig::default();
        store.save(&llm).unwrap();
        let loaded = store.load_raw().unwrap();
        assert_eq!(loaded.provider, llm.provider);
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
