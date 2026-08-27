use super::*;

#[test]
fn reasoning_effort_uses_light_product_value() {
    let effort: ReasoningEffort = serde_yaml::from_str("light").unwrap();

    assert_eq!(effort, ReasoningEffort::Light);
}

#[test]
fn test_default_config() {
    let config = Config::default();
    assert_eq!(config.agent.name, "talos");
    assert!(config.agent.agents.is_empty());
    assert_eq!(config.agent.message_queue.max_messages_per_session, 10);
    assert_eq!(config.gateway.port, 18789);
    assert!(!config.gateway.auth);
    assert_eq!(config.storage.database_type, DatabaseType::Sqlite);
    assert!(config.storage.connection_string.ends_with("/data/talos.db"));
    assert_eq!(
        config.logging.file.as_deref(),
        Some(config.talos_paths().log_file.to_string_lossy().as_ref())
    );
}

#[test]
fn memory_config_rejects_removed_root_memory_filename() {
    let error = serde_yaml::from_str::<MemoryConfig>(
        "root_memory_filename: MEMORY.md\nmemory_dirname: memory\n",
    )
    .expect_err("removed root memory filename must not be silently ignored");

    assert!(error.to_string().contains("root_memory_filename"));
}

#[test]
fn message_queue_config_parses_custom_capacity() {
    let config: Config = serde_yaml::from_str(
        r#"
agent:
  message_queue:
    max_messages_per_session: 4
"#,
    )
    .expect("message queue config should parse");

    assert_eq!(config.agent.message_queue.max_messages_per_session, 4);
}

#[test]
fn inner_model_config_parses_and_validates_against_model_catalog() {
    let mut config = Config::default();
    config.agent.inner_model = Some(ModelRef {
        provider: "deepseek".to_string(),
        id: "deepseek-v4-flash".to_string(),
    });
    config.llm.deepseek.models = vec![ProviderModelConfig {
        id: "deepseek-v4-flash".to_string(),
        name: "DeepSeek V4 Flash".to_string(),
        context_window: 1_000_000,
        reasoning: false,
        reasoning_effort: ReasoningEffort::Medium,
        speed: ModelSpeed::Fast,
        input: vec![ModelInputKind::Text],
        cost: ModelCostConfig::default(),
    }];

    assert!(config.validate_runtime().is_ok());
}

#[test]
fn runtime_validation_rejects_unknown_inner_model() {
    let mut config = Config::default();
    config.agent.inner_model = Some(ModelRef {
        provider: "deepseek".to_string(),
        id: "missing-model".to_string(),
    });

    let error = config
        .validate_runtime()
        .expect_err("unknown inner model should fail validation");

    assert!(error
        .diagnostic_message()
        .contains("deepseek:missing-model"));
}

#[test]
fn runtime_validation_rejects_zero_message_queue_capacity() {
    let mut config = Config::default();
    config.agent.message_queue.max_messages_per_session = 0;

    let error = config
        .validate_runtime()
        .expect_err("zero queue capacity should fail validation");

    assert!(error
        .diagnostic_message()
        .contains("agent.message_queue.max_messages_per_session"));
}

#[test]
fn test_attach_runtime_env_keeps_values_runtime_only() {
    let mut config = Config::default();
    let runtime_env = RuntimeEnv::load_from_iter(
        std::path::Path::new("__talos_missing_env_file__"),
        [("TALOS_WEB_SEARCH_FIRECRAWL_API_KEY", "firecrawl-key")],
    )
    .expect("runtime env should load");

    config.attach_runtime_env(runtime_env);

    assert_eq!(
        config
            .agent
            .kernel
            .runtime_env
            .get("TALOS_WEB_SEARCH_FIRECRAWL_API_KEY"),
        Some("firecrawl-key")
    );
    let serialized = serde_yaml::to_string(&config).expect("config should encode");
    assert!(!serialized.contains("runtime_env"));
    assert!(!serialized.contains("firecrawl-key"));
}

#[test]
fn memory_directory_must_not_escape_the_workspace() {
    let mut config = Config::default();
    config.memory.memory_dirname = "../../shared".to_string();

    let error = config
        .validate_runtime()
        .expect_err("parent memory path must be rejected");

    assert!(error.to_string().contains("memory.memory_dirname"));
}

#[test]
fn test_agent_definition_config_parses_kernel_runtime() {
    let yaml = r#"
agent:
  agents:
    - id: reviewer
      name: Reviewer
      kind: kernel
      enabled: true
      description: Review-focused agent
      default_workspace_id: default
      runtime_config:
        type: kernel
        kernel:
          tool_result_max_bytes: 4096
gateway:
  host: 127.0.0.1
  port: 18789
  auth: false
llm:
  provider: ollama
  model: qwen-plus
  ollama:
    base_url: http://localhost:11434
"#;

    let config = serde_yaml::from_str::<Config>(yaml).expect("agent definition should parse");
    let definition = config
        .agent
        .agents
        .first()
        .expect("agent definition should exist");
    assert_eq!(definition.id, "reviewer");
    assert_eq!(definition.name, "Reviewer");
    assert_eq!(definition.kind, AgentKind::Kernel);
    assert_eq!(definition.default_workspace_id.as_deref(), Some("default"));
    match definition
        .runtime_config
        .as_ref()
        .expect("runtime config should exist")
    {
        AgentRuntimeConfig::Kernel { kernel } => assert_eq!(kernel.tool_result_max_bytes, 4096),
    }
}

#[test]
fn test_agent_definition_rejects_unsupported_runtime_config() {
    let yaml = r#"
agent:
  agents:
    - id: codex
      name: Codex
      kind: codex
      runtime_config:
        type: codex
gateway:
  host: 127.0.0.1
  port: 18789
  auth: false
llm:
  provider: ollama
  model: qwen-plus
  ollama:
    base_url: http://localhost:11434
"#;

    let err = serde_yaml::from_str::<Config>(yaml).expect_err("unsupported runtime must fail");
    assert!(err.to_string().contains("codex"));
}

#[test]
fn test_kernel_defaults() {
    let config = Config::default();
    assert_eq!(config.agent.kernel.tool_result_max_bytes, 64 * 1024);
    assert_eq!(config.agent.kernel.context_compaction_max_bytes, 256 * 1024);
    assert_eq!(config.agent.kernel.default_workspace_id, "default");
    assert_eq!(config.agent.kernel.planner_mode, PlannerMode::BuiltIn);
    assert_eq!(config.agent.kernel.output.max_tokens, 32_000);
    assert_eq!(config.agent.kernel.output.upper_limit, 64_000);
    assert_eq!(config.agent.kernel.output.effective_max_tokens(), 32_000);
    assert_eq!(
        config.agent.kernel.prompt.window.compaction_buffer_tokens,
        4_096
    );
    assert_eq!(
        config.agent.kernel.prompt.window.reserved_output_tokens,
        ReservedOutputPolicy::UseModelDefault
    );
    assert_eq!(config.agent.kernel.compaction.model, None);
    assert_eq!(config.agent.kernel.compaction.trigger_ratio_percent, 85);
    assert_eq!(config.agent.kernel.compaction.hard_ratio_percent, 95);
    assert_eq!(config.agent.kernel.compaction.recent_messages_keep, 24);
    assert_eq!(
        config.agent.kernel.compaction.recent_messages_max_chars,
        4000
    );
    assert_eq!(
        config
            .agent
            .kernel
            .compaction
            .compacted_recent_messages_keep,
        2
    );
    assert_eq!(
        config
            .agent
            .kernel
            .compaction
            .compacted_recent_messages_max_chars,
        1200
    );
    assert_eq!(config.agent.kernel.compaction.recent_summaries_keep, 2);
    assert_eq!(config.agent.kernel.compaction.summary_max_chars, 1200);
    assert_eq!(
        config.agent.kernel.compaction.compacted_summary_max_chars,
        800
    );
    assert_eq!(
        config.agent.kernel.compaction.source_recent_messages_limit,
        48
    );
    assert_eq!(
        config
            .agent
            .kernel
            .compaction
            .source_recent_tool_results_limit,
        12
    );
    assert_eq!(config.agent.kernel.compaction.tool_output_keep_last, 2);
    assert_eq!(config.agent.kernel.compaction.tool_output_max_chars, 2000);
    assert_eq!(
        config
            .agent
            .kernel
            .compaction
            .compacted_tool_output_max_chars,
        600
    );
    assert_eq!(
        config
            .agent
            .kernel
            .compaction
            .source_recent_tool_exchanges_limit,
        12
    );
    assert_eq!(config.agent.kernel.compaction.tool_exchange_keep_last, 2);
    assert_eq!(config.agent.kernel.compaction.tool_exchange_max_chars, 600);
    assert_eq!(
        config
            .agent
            .kernel
            .compaction
            .compacted_tool_exchange_max_chars,
        300
    );
    assert_eq!(config.agent.kernel.compaction.retrieved_memories_keep, 4);
    assert_eq!(
        config.agent.kernel.compaction.retrieved_memory_max_chars,
        2000
    );
    assert_eq!(
        config
            .agent
            .kernel
            .compaction
            .compacted_retrieved_memory_max_chars,
        800
    );
    assert!(config.agent.kernel.prompt.bootstrap.enabled);
    assert!(
        config
            .agent
            .kernel
            .prompt
            .bootstrap
            .auto_create_missing_files
    );
    assert_eq!(
        config.agent.kernel.prompt.bootstrap.max_chars_per_file,
        8_000
    );
    assert_eq!(config.timezone, "UTC");
    assert_eq!(
        config.automation.profile_usage_retention.cron,
        "0 15 3 * * 1"
    );
    assert_eq!(
        config.automation.profile_usage_retention.retention_days,
        366
    );
    assert_eq!(
        config.automation.memory_daily_maintenance.cron,
        "0 30 3 * * *"
    );
    assert_eq!(config.automation.memory_daily_maintenance.batch_size, 50);
    assert_eq!(config.agent.kernel.timezone, config.timezone);
    assert_eq!(
        config.agent.kernel.workspace_dir,
        config
            .talos_paths()
            .workspace_dir_for_agent("talos")
            .to_string_lossy()
            .to_string()
    );
    assert_eq!(config.memory.file_list_default_limit, 20);
    assert_eq!(config.memory.file_list_max_limit, 90);
    assert_eq!(config.logging.level, "info");
    assert_eq!(config.storage.database_type, DatabaseType::Sqlite);
}

#[test]
fn test_prompt_config_rejects_unknown_legacy_fields() {
    let yaml = r#"
agent:
  name: talos
  kernel:
    prompt:
      bootstrap:
        include_tools: true
gateway:
  host: 127.0.0.1
  port: 18789
  auth: false
logging:
  level: info
  format: text
  stdout: true
storage:
  database_type: sqlite
  connection_string: /tmp/custom.db
  max_connections: 5
llm:
  provider: ollama
  model: qwen-plus
  anthropic: {}
  openai: {}
  bailian: {}
  ollama:
    base_url: http://localhost:11434
"#;

    let err = serde_yaml::from_str::<Config>(yaml).expect_err("legacy prompt field must fail");
    assert!(err.to_string().contains("include_tools"));
}

#[test]
fn test_expand_env_in_string() {
    let raw = "token=${A}:${B}";
    // Keep missing env placeholders as-is so behavior is predictable.
    let expanded = Config::expand_env_in_string(raw);
    assert!(expanded.contains("${A}") || expanded.contains(':'));
}

#[test]
fn test_validate_runtime_requires_cloud_when_auth_enabled() {
    let mut config = Config::default();
    config.gateway.auth = true;

    config.llm.provider = "ollama".to_string();
    config.llm.model = "qwen-max".to_string();
    config.llm.ollama.base_url = Some("http://localhost:11434".to_string());
    assert!(config.validate_runtime().is_ok());
}

#[test]
fn test_validate_runtime_rejects_zero_profile_usage_retention_batch_size() {
    let mut config = Config::default();
    config.automation.profile_usage_retention.batch_size = 0;

    let error = config.validate_runtime().unwrap_err();

    assert!(error
        .to_string()
        .contains("profile_usage_retention.batch_size"));
}

#[test]
fn test_validate_runtime_rejects_zero_profile_usage_retention_days() {
    let mut config = Config::default();
    config.automation.profile_usage_retention.retention_days = 0;

    let error = config.validate_runtime().unwrap_err();

    assert!(error
        .to_string()
        .contains("profile_usage_retention.retention_days"));
}

#[test]
fn test_validate_runtime_rejects_invalid_profile_usage_retention_cron() {
    let mut config = Config::default();
    config.automation.profile_usage_retention.cron = "not-a-cron".to_string();

    let error = config.validate_runtime().unwrap_err();

    assert!(error
        .to_string()
        .contains("profile_usage_retention.cron is invalid"));
}

#[test]
fn test_validate_runtime_allows_cloud_auth_without_local_secret() {
    let mut config = Config::default();
    config.gateway.auth = true;
    config.llm.provider = "ollama".to_string();
    config.llm.model = "qwen-max".to_string();
    config.llm.ollama.base_url = Some("http://localhost:11434".to_string());

    assert!(config.validate_runtime().is_ok());
}

#[test]
fn test_model_readiness_reports_missing_provider_api_key() {
    let mut config = Config::default();
    config.gateway.auth = false;
    config.llm.provider = "openai".to_string();
    config.llm.model = "gpt-4o".to_string();
    config.llm.openai.base_url = Some("https://api.openai.com/v1".to_string());
    config.llm.openai.models = vec![ProviderModelConfig {
        id: "gpt-4o".to_string(),
        name: "GPT-4o".to_string(),
        context_window: 128_000,
        reasoning: true,
        reasoning_effort: ReasoningEffort::Medium,
        speed: ModelSpeed::Standard,
        input: vec![ModelInputKind::Text],
        cost: ModelCostConfig::default(),
    }];
    config.llm.openai.api_key = None;
    assert!(config.validate_runtime().is_ok());
    let readiness = config.model_readiness();
    assert!(!readiness.ready);
    assert!(readiness
        .issues
        .iter()
        .any(|issue| issue.contains("api_key")));

    config.llm.openai.api_key = Some("sk-test".to_string());
    let readiness = config.model_readiness();
    assert!(readiness.ready);
}

#[test]
fn test_provider_protocol_defaults_by_provider() {
    assert_eq!(
        default_llm_protocol("anthropic"),
        Some(LlmProtocol::Messages)
    );
    assert_eq!(default_llm_protocol("openai"), Some(LlmProtocol::Chat));
    assert_eq!(default_llm_protocol("bailian"), Some(LlmProtocol::Chat));
    assert_eq!(default_llm_protocol("ollama"), Some(LlmProtocol::Chat));
    assert_eq!(default_llm_protocol("openrouter"), Some(LlmProtocol::Chat));
    assert_eq!(default_llm_protocol("deepseek"), Some(LlmProtocol::Chat));
    assert_eq!(default_llm_protocol("unknown"), None);
}

#[test]
fn test_provider_protocol_support_is_provider_scoped() {
    assert!(llm_provider_supports_protocol(
        "openai",
        LlmProtocol::Responses
    ));
    assert!(llm_provider_supports_protocol("openai", LlmProtocol::Chat));
    assert!(!llm_provider_supports_protocol(
        "anthropic",
        LlmProtocol::Chat
    ));
    assert!(!llm_provider_supports_protocol(
        "bailian",
        LlmProtocol::Messages
    ));
    assert!(llm_provider_supports_protocol(
        "openrouter",
        LlmProtocol::Chat
    ));
    assert!(llm_provider_supports_protocol(
        "openrouter",
        LlmProtocol::Responses
    ));
    assert!(llm_provider_supports_protocol(
        "openrouter",
        LlmProtocol::Messages
    ));
    assert!(llm_provider_supports_protocol(
        "deepseek",
        LlmProtocol::Chat
    ));
    assert!(llm_provider_supports_protocol(
        "deepseek",
        LlmProtocol::Messages
    ));

    let capabilities = provider_protocol_capabilities();
    assert_eq!(
        capabilities["deepseek"],
        ProviderProtocolCapabilities {
            default_protocol: LlmProtocol::Chat,
            protocols: vec![LlmProtocol::Chat, LlmProtocol::Messages],
        }
    );
    assert_eq!(
        capabilities["openrouter"],
        ProviderProtocolCapabilities {
            default_protocol: LlmProtocol::Chat,
            protocols: vec![
                LlmProtocol::Chat,
                LlmProtocol::Responses,
                LlmProtocol::Messages,
            ],
        }
    );
}

#[test]
fn test_model_readiness_reports_unsupported_provider_protocol() {
    let mut config = Config::default();
    config.gateway.auth = false;
    config.llm.provider = "bailian".to_string();
    config.llm.model = "qwen-max".to_string();
    config.llm.bailian.api_key = Some("sk-test".to_string());
    config.llm.bailian.base_url =
        Some("https://dashscope.aliyuncs.com/compatible-mode/v1".to_string());
    config.llm.bailian.protocol = Some(LlmProtocol::Messages);

    let readiness = config.model_readiness();

    assert!(!readiness.ready);
    assert!(readiness
        .issues
        .iter()
        .any(|issue| issue.contains("protocol") && issue.contains("messages")));
}

#[test]
fn test_validate_runtime_rejects_invalid_timezone() {
    let mut config = Config::default();
    config.gateway.auth = false;
    config.llm.provider = "openai".to_string();
    config.llm.model = "gpt-4o".to_string();
    config.llm.openai.base_url = Some("https://api.openai.com/v1".to_string());
    config.llm.openai.api_key = Some("sk-test".to_string());
    config.timezone = "Mars/Olympus".to_string();
    config.apply_derived_paths();

    assert!(config.validate_runtime().is_err());
}

#[test]
fn test_provider_models_are_strongly_typed() {
    let yaml = r#"
api_key: sk-test
base_url: https://dashscope.aliyuncs.com/compatible-mode/v1
protocol: chat
models:
  - id: qwen-max
    name: qwen-max
    context_window: 32768
    reasoning: false
    input: [text, image]
    cost:
      input: 0.1
      output: 0.2
      cache_read: 0.01
      cache_write: 0.02
"#;

    let provider: ProviderConfig =
        serde_yaml::from_str(yaml).expect("provider config should parse");
    let model = &provider.models[0];
    assert_eq!(provider.protocol, Some(LlmProtocol::Chat));
    assert_eq!(model.id, "qwen-max");
    assert_eq!(model.context_window, 32_768);
    assert_eq!(
        model.input,
        vec![ModelInputKind::Text, ModelInputKind::Image]
    );
}

#[test]
fn test_product_configs_include_configured_provider_models() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../..");
    let config_paths = [
        repo_root.join("conf/talos.yaml"),
        repo_root.join("conf/talos-dev.yaml"),
        repo_root.join("conf/talos-prod.yaml"),
    ];
    for config_path in config_paths {
        let content =
            std::fs::read_to_string(config_path).expect("product config should be readable");
        let config: Config =
            serde_yaml::from_str(&content).expect("product config should deserialize");
        assert_eq!(
            config
                .llm
                .deepseek
                .models
                .iter()
                .map(|model| (model.id.as_str(), model.context_window))
                .collect::<Vec<_>>(),
            vec![
                ("deepseek-v4-pro", 1_000_000),
                ("deepseek-v4-flash", 1_000_000),
            ]
        );
        let openrouter_models = config
            .llm
            .openrouter
            .models
            .iter()
            .map(|model| (model.id.as_str(), model.context_window))
            .collect::<Vec<_>>();
        assert!(openrouter_models.contains(&("tencent/hy3:free", 262_144)));
        assert!(openrouter_models.contains(&("meta-llama/llama-3.3-70b-instruct", 131_072)));
        assert!(openrouter_models.contains(&("meta-llama/llama-3.3-70b-instruct:free", 131_072)));
        assert_eq!(
            config
                .llm
                .ollama
                .models
                .iter()
                .map(|model| {
                    (
                        model.id.as_str(),
                        model.context_window,
                        model.reasoning,
                        model.reasoning_effort,
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                ("qwen3:8b", 40_960, true, ReasoningEffort::Medium),
                ("deepseek-r1:8b", 131_072, true, ReasoningEffort::Medium),
                ("gpt-oss:20b", 131_072, true, ReasoningEffort::Medium),
            ]
        );
    }
}

#[test]
fn test_provider_models_reject_legacy_output_metadata() {
    let yaml = r#"
api_key: sk-test
base_url: https://dashscope.aliyuncs.com/compatible-mode/v1
models:
  - id: qwen-max
    name: qwen-max
    context_window: 32768
    max_tokens: 81920
    reasoning: false
    input: [text]
    cost:
      input: 0.1
      output: 0.2
      cache_read: 0.01
      cache_write: 0.02
"#;

    assert!(serde_yaml::from_str::<ProviderConfig>(yaml).is_err());
}

#[test]
fn test_load_from_path_fails_when_required_keys_missing() {
    let temp_file =
        std::env::temp_dir().join(format!("talos-config-missing-{}.yaml", std::process::id()));
    std::fs::write(&temp_file, "gateway:\n  host: 127.0.0.1\n")
        .expect("failed to write temp config");
    let result = Config::load_from_path(Some(
        temp_file
            .to_str()
            .expect("temp path should be valid unicode"),
    ));
    let _ = std::fs::remove_file(&temp_file);
    assert!(result.is_err());
}

#[test]
fn test_load_from_path_uses_prompt_defaults_when_prompt_block_is_missing() {
    let yaml = r#"
agent:
  name: talos
gateway:
  host: 127.0.0.1
  port: 18789
  auth: false
logging:
  level: info
  format: text
  stdout: true
storage:
  database_type: sqlite
  max_connections: 5
llm:
  provider: ollama
  model: qwen-plus
  anthropic: {}
  openai: {}
  bailian: {}
  ollama:
    base_url: http://localhost:11434
"#;
    let temp_file = std::env::temp_dir().join(format!(
        "talos-config-prompt-defaults-{}.yaml",
        std::process::id()
    ));
    std::fs::write(&temp_file, yaml).expect("failed to write temp config");
    let config = Config::load_from_path(Some(
        temp_file
            .to_str()
            .expect("temp path should be valid unicode"),
    ))
    .expect("config without prompt block should load");
    assert_eq!(config.source_path(), temp_file);
    let _ = std::fs::remove_file(&temp_file);

    assert_eq!(config.agent.kernel.compaction.recent_messages_keep, 24);
    assert_eq!(
        config.agent.kernel.compaction.recent_messages_max_chars,
        4000
    );
    assert_eq!(
        config
            .agent
            .kernel
            .compaction
            .compacted_recent_messages_keep,
        2
    );
    assert_eq!(
        config
            .agent
            .kernel
            .compaction
            .compacted_recent_messages_max_chars,
        1200
    );
    assert_eq!(config.agent.kernel.compaction.recent_summaries_keep, 2);
    assert_eq!(config.agent.kernel.compaction.summary_max_chars, 1200);
    assert_eq!(
        config.agent.kernel.compaction.compacted_summary_max_chars,
        800
    );
    assert_eq!(
        config
            .agent
            .kernel
            .compaction
            .source_recent_tool_results_limit,
        12
    );
    assert_eq!(config.agent.kernel.compaction.tool_output_keep_last, 2);
    assert_eq!(config.agent.kernel.compaction.tool_output_max_chars, 2000);
    assert_eq!(
        config
            .agent
            .kernel
            .compaction
            .compacted_tool_output_max_chars,
        600
    );
    assert_eq!(
        config
            .agent
            .kernel
            .compaction
            .source_recent_tool_exchanges_limit,
        12
    );
    assert_eq!(config.agent.kernel.compaction.tool_exchange_keep_last, 2);
    assert_eq!(config.agent.kernel.compaction.tool_exchange_max_chars, 600);
    assert_eq!(
        config
            .agent
            .kernel
            .compaction
            .compacted_tool_exchange_max_chars,
        300
    );
    assert_eq!(
        config
            .agent
            .kernel
            .compaction
            .compacted_retrieved_memory_max_chars,
        800
    );
}

#[test]
fn test_compacted_char_budgets_cannot_exceed_normal_budgets() {
    let yaml = r#"
agent:
  name: talos
  kernel:
    compaction:
      recent_messages_max_chars: 100
      compacted_recent_messages_max_chars: 120
gateway:
  host: 127.0.0.1
  port: 18789
  auth: false
logging:
  level: info
  format: text
  stdout: true
storage:
  database_type: sqlite
  connection_string: /tmp/custom.db
  max_connections: 5
llm:
  provider: ollama
  model: qwen-plus
  anthropic: {}
  openai: {}
  bailian: {}
  ollama:
    base_url: http://localhost:11434
"#;
    let temp_file = std::env::temp_dir().join(format!(
        "talos-config-invalid-compaction-{}.yaml",
        std::process::id()
    ));
    std::fs::write(&temp_file, yaml).expect("failed to write temp config");
    let result = Config::load_from_path(Some(
        temp_file
            .to_str()
            .expect("temp path should be valid unicode"),
    ));
    let _ = std::fs::remove_file(&temp_file);

    let _ = result.expect_err("invalid compacted char budget should fail");
}

#[test]
fn test_load_from_path_uses_optional_block_defaults_when_omitted() {
    let yaml = r#"
agent:
  name: talos
gateway:
  host: 127.0.0.1
  port: 18789
  auth: false
llm:
  provider: ollama
  model: qwen-plus
  ollama:
    base_url: http://localhost:11434
"#;
    let temp_file = std::env::temp_dir().join(format!(
        "talos-config-optional-defaults-{}.yaml",
        std::process::id()
    ));
    std::fs::write(&temp_file, yaml).expect("failed to write temp config");
    let config = Config::load_from_path(Some(
        temp_file
            .to_str()
            .expect("temp path should be valid unicode"),
    ))
    .expect("config without optional blocks should load");
    let _ = std::fs::remove_file(&temp_file);

    assert_eq!(config.logging.level, "info");
    assert_eq!(config.logging.format, "text");
    assert!(config.logging.stdout);
    assert_eq!(config.storage.database_type, DatabaseType::Sqlite);
    assert_eq!(config.storage.max_connections, 5);
}

#[test]
fn test_load_from_path_fails_when_file_missing() {
    let missing_file = std::env::temp_dir().join(format!(
        "talos-config-not-found-{}.yaml",
        std::process::id()
    ));
    let result = Config::load_from_path(Some(
        missing_file
            .to_str()
            .expect("temp path should be valid unicode"),
    ));
    assert!(result.is_err());
}

#[test]
fn test_sanitize_agent_name_for_workspace() {
    assert_eq!(sanitize_agent_name("Talos Agent"), "talos-agent");
    assert_eq!(sanitize_agent_name("..."), "default");
}

#[test]
fn test_forbidden_path_config_is_rejected() {
    let yaml = r#"
agent:
  name: talos
  kernel:
    prompt:
      window:
        reserved_output_tokens:
          kind: use_model_default
        compaction_buffer_tokens: 4096
    compaction:
      recent_messages_keep: 16
      compacted_recent_messages_keep: 2
      recent_summaries_keep: 2
      summary_max_chars: 1000
    workspace_dir: /tmp/custom
gateway:
  host: 127.0.0.1
  port: 18789
  auth: false
logging:
  level: info
  format: text
  stdout: true
storage:
  database_type: sqlite
  connection_string: /tmp/custom.db
  max_connections: 5
llm:
  provider: ollama
  model: qwen-plus
  anthropic: {}
  openai: {}
  bailian: {}
  ollama:
base_url: http://localhost:11434
"#;
    let temp_file = std::env::temp_dir().join(format!(
        "talos-config-forbidden-{}.yaml",
        std::process::id()
    ));
    std::fs::write(&temp_file, yaml).expect("failed to write temp config");
    let result = Config::load_from_path(Some(
        temp_file
            .to_str()
            .expect("temp path should be valid unicode"),
    ));
    let _ = std::fs::remove_file(&temp_file);
    assert!(result.is_err());
}

#[test]
fn test_kernel_config_rejects_legacy_binding_field() {
    let yaml = r#"
agent:
  name: talos
  kernel:
    binding:
      default_agent_kind: llm
gateway:
  host: 127.0.0.1
  port: 18789
  auth: false
llm:
  provider: ollama
  model: qwen-plus
  ollama:
    base_url: http://localhost:11434
"#;

    let err = serde_yaml::from_str::<Config>(yaml).expect_err("legacy binding field must fail");
    assert!(err.to_string().contains("binding"));
}
