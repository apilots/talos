use super::*;
use crate::{
    ProviderModelConfig, WebSearchProviderConfig, WebSearchProviderRuntimeConfig,
    WebSearchToolConfig,
};
use std::io::Write;
use std::sync::Arc;
use tempfile::{NamedTempFile, TempDir};

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
    models:
      - id: claude-3-5-sonnet
        name: Claude 3.5 Sonnet
        context_window: 200000
        reasoning: true
        reasoning_effort: medium
        speed: standard
        input: [text]
        cost: {{ input: 0, output: 0, cache_read: 0, cache_write: 0 }}
  openai:
    models: []
  bailian:
    models: []
  deepseek:
    api_key: ${{TALOS_LLM_DEEPSEEK_API_KEY}}
    models: []
  ollama:
    models: []
  openrouter:
    models: []
"#
    )
}

fn minimal_yaml_with_llm_and_web_search(web_search_api_key: &str) -> String {
    format!(
        r#"gateway:
  port: 8080
agent:
  name: talos
  kernel:
    tools:
      web_search:
        provider: brave
        firecrawl:
          api_key: {web_search_api_key}
          base_url: https://firecrawl.example
          timeout_ms: 11000
        brave:
          api_key: ${{TALOS_WEB_SEARCH_BRAVE_API_KEY}}
          base_url: https://brave.example
          timeout_ms: 12000
        tavily:
          api_key: ${{TALOS_WEB_SEARCH_TAVILY_API_KEY}}
          base_url: https://tavily.example
          timeout_ms: 13000
llm:
  provider: anthropic
  model: claude-3-5-sonnet
  anthropic:
    api_key: ${{TALOS_LLM_ANTHROPIC_API_KEY}}
    models: []
  openai:
    models: []
  bailian:
    models: []
  deepseek:
    models: []
  ollama:
    models: []
  openrouter:
    models: []
"#
    )
}

fn store_for(file: &NamedTempFile) -> YamlConfigStore {
    YamlConfigStore::with_env_path(file.path().to_path_buf(), file.path().with_extension("env"))
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
fn test_load_read_reports_configured_deepseek_secret() {
    let file = write_yaml(&minimal_yaml_with_llm("${TALOS_LLM_ANTHROPIC_API_KEY}"));
    let env_path = file.path().with_extension("env");
    std::fs::write(&env_path, "TALOS_LLM_DEEPSEEK_API_KEY=deepseek-secret\n").unwrap();
    let store = YamlLlmConfigStore::with_env_path(file.path().to_path_buf(), env_path);

    let read = store.load_read().unwrap();

    assert_eq!(
        read.config.deepseek.api_key.as_deref(),
        Some(REDACTED_API_KEY)
    );
    assert!(read.provider_keys["deepseek"].api_key_configured);
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
    });
    store.save(&llm).unwrap();

    let loaded = store.load_raw().unwrap();
    assert_eq!(loaded.provider, "bailian");
    assert_eq!(loaded.model, "qwen-max");
    assert_eq!(
        loaded
            .anthropic
            .models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        vec!["claude-3-5-sonnet", "test-model"]
    );
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
fn test_load_web_search_read_reports_configured_placeholder_secret() {
    let file = write_yaml(&minimal_yaml_with_llm_and_web_search(
        "${TALOS_WEB_SEARCH_FIRECRAWL_API_KEY}",
    ));
    let env_path = file.path().with_extension("env");
    std::fs::write(
        &env_path,
        "TALOS_WEB_SEARCH_FIRECRAWL_API_KEY=firecrawl-secret\n",
    )
    .unwrap();
    let store = YamlConfigStore::with_env_path(file.path().to_path_buf(), env_path);

    let read = store.load_web_search_read().unwrap();

    assert_eq!(read.provider, WebSearchProviderConfig::Brave);
    assert_eq!(read.firecrawl.api_key.as_deref(), Some(REDACTED_API_KEY));
    assert_eq!(read.brave.api_key.as_deref(), None);
    assert_eq!(
        read.firecrawl.base_url.as_deref(),
        Some("https://firecrawl.example")
    );
}

#[test]
fn test_save_web_search_writes_provider_keys_to_env_and_yaml_placeholders() {
    let file = write_yaml(&minimal_yaml_with_llm_and_web_search(
        "${TALOS_WEB_SEARCH_FIRECRAWL_API_KEY}",
    ));
    let env_path = file.path().with_extension("env");
    let store = YamlConfigStore::with_env_path(file.path().to_path_buf(), env_path.clone());
    let mut web_search = store.load_web_search_read().unwrap();
    web_search.firecrawl.api_key = Some("firecrawl-new".to_string());
    web_search.brave.api_key = Some("brave-new".to_string());
    web_search.tavily.api_key = Some("tavily-new".to_string());

    let saved = store.save_web_search(&web_search).unwrap();

    assert_eq!(saved.firecrawl.api_key.as_deref(), Some(REDACTED_API_KEY));
    assert_eq!(saved.brave.api_key.as_deref(), Some(REDACTED_API_KEY));
    assert_eq!(saved.tavily.api_key.as_deref(), Some(REDACTED_API_KEY));
    let env = std::fs::read_to_string(&env_path).unwrap();
    assert!(env.contains("TALOS_WEB_SEARCH_FIRECRAWL_API_KEY=firecrawl-new"));
    assert!(env.contains("TALOS_WEB_SEARCH_BRAVE_API_KEY=brave-new"));
    assert!(env.contains("TALOS_WEB_SEARCH_TAVILY_API_KEY=tavily-new"));
    let raw: crate::Config =
        serde_yaml::from_str(&std::fs::read_to_string(file.path()).unwrap()).unwrap();
    assert_eq!(
        raw.agent
            .kernel
            .tools
            .web_search
            .firecrawl
            .api_key
            .as_deref(),
        Some("${TALOS_WEB_SEARCH_FIRECRAWL_API_KEY}")
    );
    assert_eq!(
        raw.agent.kernel.tools.web_search.brave.api_key.as_deref(),
        Some("${TALOS_WEB_SEARCH_BRAVE_API_KEY}")
    );
    assert_eq!(
        raw.agent.kernel.tools.web_search.tavily.api_key.as_deref(),
        Some("${TALOS_WEB_SEARCH_TAVILY_API_KEY}")
    );
}

#[test]
fn test_save_web_search_redacted_keeps_and_empty_clears_env_secret() {
    let file = write_yaml(&minimal_yaml_with_llm_and_web_search(
        "${TALOS_WEB_SEARCH_FIRECRAWL_API_KEY}",
    ));
    let env_path = file.path().with_extension("env");
    std::fs::write(
        &env_path,
        "TALOS_WEB_SEARCH_FIRECRAWL_API_KEY=firecrawl-secret\nTALOS_WEB_SEARCH_BRAVE_API_KEY=brave-secret\n",
    )
    .unwrap();
    let store = YamlConfigStore::with_env_path(file.path().to_path_buf(), env_path.clone());
    let mut web_search = store.load_web_search_read().unwrap();
    web_search.brave.api_key = Some(String::new());

    store.save_web_search(&web_search).unwrap();

    let env = std::fs::read_to_string(&env_path).unwrap();
    assert!(env.contains("TALOS_WEB_SEARCH_FIRECRAWL_API_KEY=firecrawl-secret"));
    assert!(!env.contains("TALOS_WEB_SEARCH_BRAVE_API_KEY="));
}

#[test]
fn test_save_web_search_migrates_existing_plaintext_api_key_when_ui_omits_secret() {
    let file = write_yaml(&minimal_yaml_with_llm_and_web_search("firecrawl-existing"));
    let env_path = file.path().with_extension("env");
    let store = YamlConfigStore::with_env_path(file.path().to_path_buf(), env_path.clone());
    let mut web_search = store.load_web_search_read().unwrap();
    web_search.firecrawl.timeout_ms = 21000;

    store.save_web_search(&web_search).unwrap();

    let env = std::fs::read_to_string(&env_path).unwrap();
    assert!(env.contains("TALOS_WEB_SEARCH_FIRECRAWL_API_KEY=firecrawl-existing"));
    let yaml = std::fs::read_to_string(file.path()).unwrap();
    assert!(!yaml.contains("firecrawl-existing"));
    assert!(yaml.contains("timeout_ms: 21000"));
}

#[test]
fn test_save_web_search_only_replaces_nested_section() {
    let file = write_yaml(&minimal_yaml_with_llm_and_web_search(
        "${TALOS_WEB_SEARCH_FIRECRAWL_API_KEY}",
    ));
    let store = store_for(&file);
    let mut web_search = store.load_web_search_read().unwrap();
    web_search.provider = WebSearchProviderConfig::Tavily;
    web_search.tavily.base_url = Some("https://new-tavily.example".to_string());

    store.save_web_search(&web_search).unwrap();

    let yaml = std::fs::read_to_string(file.path()).unwrap();
    assert!(yaml.contains("gateway:"));
    assert!(yaml.contains("port: 8080"));
    assert!(yaml.contains("llm:"));
    assert!(yaml.contains("provider: tavily"));
    assert!(yaml.contains("https://new-tavily.example"));
}

#[test]
fn test_models_and_web_search_updates_do_not_drop_each_other() {
    let file = write_yaml(&minimal_yaml_with_llm_and_web_search(
        "${TALOS_WEB_SEARCH_FIRECRAWL_API_KEY}",
    ));
    let store = Arc::new(store_for(&file));

    let llm_store = store.clone();
    let llm_thread = std::thread::spawn(move || {
        let mut llm = llm_store.load_llm_raw().unwrap();
        llm.model = "updated-model".to_string();
        llm_store.save_llm(&llm).unwrap();
    });

    let web_store = store.clone();
    let web_thread = std::thread::spawn(move || {
        let mut web_search = web_store.load_web_search_read().unwrap();
        web_search.brave.api_key = Some("brave-thread-secret".to_string());
        web_store.save_web_search(&web_search).unwrap();
    });

    llm_thread.join().unwrap();
    web_thread.join().unwrap();

    let yaml = std::fs::read_to_string(file.path()).unwrap();
    assert!(yaml.contains("updated-model"));
    assert!(yaml.contains("web_search:"));
    let env = std::fs::read_to_string(file.path().with_extension("env")).unwrap();
    assert!(env.contains("TALOS_WEB_SEARCH_BRAVE_API_KEY=brave-thread-secret"));
}

#[test]
fn test_save_web_search_rolls_back_yaml_when_env_write_fails() {
    let file = write_yaml(&minimal_yaml_with_llm_and_web_search(
        "${TALOS_WEB_SEARCH_FIRECRAWL_API_KEY}",
    ));
    let original_yaml = std::fs::read_to_string(file.path()).unwrap();
    let env_dir = TempDir::new().unwrap();
    let store = YamlConfigStore::with_env_path(file.path().to_path_buf(), env_dir.path().into());
    let web_search = WebSearchToolConfig {
        firecrawl: WebSearchProviderRuntimeConfig {
            api_key: Some("firecrawl-new".to_string()),
            ..WebSearchProviderRuntimeConfig::default()
        },
        ..WebSearchToolConfig::default()
    };

    let error = store
        .save_web_search(&web_search)
        .expect_err("env write failure must be returned");

    assert!(error.is_code(talos_error::ErrorCode::InternalError));
    assert_eq!(std::fs::read_to_string(file.path()).unwrap(), original_yaml);
}

#[test]
fn test_in_memory_store_roundtrip() {
    let initial = LlmConfig::default();
    let store = InMemoryLlmConfigStore::new(initial.clone());
    let mut llm = store.load_llm_raw().unwrap();
    llm.model = "custom-model".to_string();
    store.save_llm(&llm).unwrap();
    let loaded = store.load_llm_raw().unwrap();
    assert_eq!(loaded.model, "custom-model");
}

#[test]
fn runtime_update_resolution_matches_secret_persistence_semantics() {
    let mut current_llm = LlmConfig::default();
    current_llm.openai.api_key = Some("openai-current".to_string());
    current_llm.deepseek.api_key = Some("deepseek-current".to_string());
    let mut updated_llm = current_llm.clone();
    updated_llm.openai.api_key = Some(REDACTED_API_KEY.to_string());
    updated_llm.deepseek.api_key = Some(String::new());
    resolve_llm_update_for_runtime(&current_llm, &mut updated_llm);

    let mut current_search = WebSearchToolConfig::default();
    current_search.brave.api_key = Some("brave-current".to_string());
    let mut updated_search = current_search.clone();
    updated_search.brave.api_key = None;
    resolve_web_search_update_for_runtime(&current_search, &mut updated_search);

    assert_eq!(
        (
            updated_llm.openai.api_key,
            updated_llm.deepseek.api_key,
            updated_search.brave.api_key,
        ),
        (
            Some("openai-current".to_string()),
            None,
            Some("brave-current".to_string()),
        )
    );
}
