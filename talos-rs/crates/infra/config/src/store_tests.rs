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
    models: []
  openai:
    models: []
  bailian:
    models: []
  ollama:
    models: []
  openrouter:
    models: []
"#
    )
}

fn store_for(file: &NamedTempFile) -> YamlLlmConfigStore {
    YamlLlmConfigStore::with_env_path(file.path().to_path_buf(), file.path().with_extension("env"))
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
