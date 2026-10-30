use super::*;
use crate::{model_readiness_for_llm, ConfigStore, ProviderHttpTimeouts, YamlConfigStore};

fn configured() -> LlmConfig {
    serde_yaml::from_str(
        r#"
provider: llm-gateway
model: model
custom_providers:
  llm-gateway:
    name: LLM Gateway
    protocol: chat
    base_url: http://127.0.0.1:8000/v1
    models:
      - id: model
        name: Qwen3
        reasoning: true
        request_parameters:
          chat_template_kwargs: {enable_thinking: true}
"#,
    )
    .unwrap()
}

#[test]
fn custom_no_auth_connection_reuses_protocol_capabilities_and_model_readiness() {
    let config = configured();
    config.validate_providers().unwrap();
    assert!(model_readiness_for_llm(&config).ready);
    assert_eq!(
        config.protocol_capabilities()["llm-gateway"],
        ProviderProtocolCapabilities {
            default_protocol: LlmProtocol::Chat,
            protocols: vec![
                LlmProtocol::Chat,
                LlmProtocol::Messages,
                LlmProtocol::Responses
            ],
        }
    );
}

#[test]
fn custom_ids_protocols_and_owned_parameters_are_validated() {
    for id in ["openai", "Upper", "a_b", "-local", ""] {
        let mut config = configured();
        let custom = config.custom_providers.remove("llm-gateway").unwrap();
        config.custom_providers.insert(id.to_owned(), custom);
        assert!(config.validate_providers().is_err());
    }
    let mut config = configured();
    config
        .custom_providers
        .get_mut("llm-gateway")
        .unwrap()
        .config
        .protocol = None;
    assert!(config.validate_providers().is_err());
    let mut config = configured();
    config
        .custom_providers
        .get_mut("llm-gateway")
        .unwrap()
        .config
        .models[0]
        .request_parameters
        .insert("messages".into(), serde_json::json!([]));
    assert!(config.validate_providers().is_err());
}

#[test]
fn custom_secret_is_redacted_preserved_cleared_and_removed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("talos.yaml");
    let env = dir.path().join(".env");
    std::fs::write(&path, "llm: {}\n").unwrap();
    let store = YamlConfigStore::with_env_path(path.clone(), env.clone());
    let mut config = configured();
    config
        .custom_providers
        .get_mut("llm-gateway")
        .unwrap()
        .config
        .api_key = Some("private-test-key".into());
    store.save_llm(&config).unwrap();
    assert!(!std::fs::read_to_string(&path)
        .unwrap()
        .contains("private-test-key"));
    let read = store.load_llm_read().unwrap();
    assert_eq!(
        read.config.custom_providers["llm-gateway"]
            .config
            .api_key
            .as_deref(),
        Some("********")
    );
    store.save_llm(&read.config).unwrap();
    assert!(store.load_llm_read().unwrap().provider_keys["llm-gateway"].api_key_configured);
    let mut cleared = read.config;
    cleared
        .custom_providers
        .get_mut("llm-gateway")
        .unwrap()
        .config
        .api_key = Some(String::new());
    store.save_llm(&cleared).unwrap();
    assert!(!store.load_llm_read().unwrap().provider_keys["llm-gateway"].api_key_configured);
    store.save_llm(&config).unwrap();
    config.custom_providers.clear();
    store.save_llm(&config).unwrap();
    assert!(!std::fs::read_to_string(env)
        .unwrap()
        .contains("private-test-key"));
}

#[test]
fn replay_overrides_are_model_owned_and_restricted_to_custom_chat() {
    for replay in [
        super::super::network::ReasoningReplay::Omit,
        super::super::network::ReasoningReplay::Reasoning,
        super::super::network::ReasoningReplay::ReasoningContent,
    ] {
        let mut config = configured();
        config
            .custom_providers
            .get_mut("llm-gateway")
            .unwrap()
            .config
            .models[0]
            .reasoning_replay = replay;
        config.validate_providers().unwrap();
        for protocol in [LlmProtocol::Messages, LlmProtocol::Responses] {
            config
                .custom_providers
                .get_mut("llm-gateway")
                .unwrap()
                .config
                .protocol = Some(protocol);
            assert!(config.validate_providers().is_err());
        }
        let mut builtin = LlmConfig::default();
        builtin.openai.models = config.custom_providers["llm-gateway"].config.models.clone();
        assert!(builtin.validate_providers().is_err());
    }
    let mut config = configured();
    config
        .custom_providers
        .get_mut("llm-gateway")
        .unwrap()
        .config
        .models[0]
        .request_parameters
        .insert("reasoning_replay".into(), serde_json::json!("omit"));
    assert!(config.validate_providers().is_err());
}

#[test]
fn custom_http_timeout_policy_accepts_independent_deadlines() {
    let mut config = configured();
    let policy = config
        .custom_providers
        .get_mut("llm-gateway")
        .unwrap()
        .config
        .http_timeouts
        .get_or_insert_with(ProviderHttpTimeouts::default);
    policy.response_secs = 5;
    policy.stream_total_secs = 30;
    config.validate_providers().unwrap();
    let policy = config
        .custom_providers
        .get_mut("llm-gateway")
        .unwrap()
        .config
        .http_timeouts
        .get_or_insert_with(ProviderHttpTimeouts::default);
    policy.stream_total_secs = 90_000;
    config.validate_providers().unwrap();
}
