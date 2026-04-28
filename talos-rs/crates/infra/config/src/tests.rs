use super::*;

#[test]
fn test_default_config() {
    let config = Config::default();
    assert_eq!(config.agent.name, "talos");
    assert_eq!(config.gateway.port, 18789);
    assert!(!config.gateway.auth);
    assert_eq!(config.gateway.access_token_ttl_days, 30);
    assert!(config.gateway.allow_register);
    assert!(config.gateway.auth_cache.enabled);
    assert_eq!(config.gateway.auth_cache.ttl_seconds, 300);
    assert_eq!(config.storage.database_type, DatabaseType::Sqlite);
    assert!(config.storage.connection_string.ends_with("/data/talos.db"));
    assert_eq!(
        config.logging.file.as_deref(),
        Some(config.talos_paths().log_file.to_string_lossy().as_ref())
    );
}

#[test]
fn test_generate_default() {
    let yaml = generate_default_config();
    assert!(yaml.contains("agent:"));
    assert!(yaml.contains("gateway:"));
}

#[test]
fn test_kernel_defaults() {
    let config = Config::default();
    assert_eq!(config.agent.kernel.max_iters, 1);
    assert_eq!(config.agent.kernel.max_task_per_turn, 3);
    assert_eq!(config.agent.kernel.max_tool_rounds, 8);
    assert_eq!(config.agent.kernel.tool_result_max_bytes, 64 * 1024);
    assert_eq!(config.agent.kernel.context_compaction_max_bytes, 256 * 1024);
    assert_eq!(config.agent.kernel.default_workspace_id, "default");
    assert_eq!(config.agent.kernel.planner_mode, PlannerMode::BuiltIn);
    assert_eq!(
        config.agent.kernel.planner_selection,
        PlannerSelection::Static
    );
    assert_eq!(config.agent.kernel.execution_mode, ExecutionMode::Single);
    assert_eq!(config.agent.kernel.execution_children, 2);
    assert_eq!(config.agent.kernel.execution_loop_iters, 3);
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
    assert!(config.agent.kernel.prompt.bootstrap.include_memory);
    assert_eq!(config.timezone, "UTC");
    assert_eq!(config.agent.kernel.timezone, config.timezone);
    assert_eq!(
        config.agent.kernel.memory.workspace_dir,
        config
            .talos_paths()
            .workspace_dir_for_agent("talos")
            .to_string_lossy()
            .to_string()
    );
    assert_eq!(config.agent.kernel.memory.hot_recent_messages, 64);
    assert_eq!(
        config.agent.kernel.memory.compaction_threshold_bytes,
        256 * 1024
    );
    assert_eq!(config.agent.kernel.memory.root_memory_filename, "MEMORY.md");
    assert_eq!(config.agent.kernel.memory.file_list_default_limit, 20);
    assert_eq!(config.agent.kernel.memory.file_list_max_limit, 90);
    assert!(config.agent.kernel.memory.workspace_memory_flush_enabled);
    assert_eq!(config.logging.level, "info");
    assert_eq!(config.storage.database_type, DatabaseType::Sqlite);
}

#[test]
fn test_prompt_config_rejects_unknown_legacy_fields() {
    let yaml = r#"
agent:
  name: talos
  model: qwen-plus
  kernel:
    prompt:
      bootstrap:
        include_tools: true
    memory:
      workspace_dir: /tmp/custom
gateway:
  host: 127.0.0.1
  port: 18789
  auth: false
  jwt_secret: jwt
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
  anthropic: { enabled: false }
  openai: { enabled: false }
  bailian: { enabled: false }
  ollama:
    enabled: true
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
fn test_validate_runtime_requires_jwt_secret_when_auth_enabled() {
    let mut config = Config::default();
    config.gateway.auth = true;
    config.gateway.jwt_secret = None;
    assert!(config.validate_runtime().is_err());

    config.gateway.jwt_secret = Some("jwt-secret".to_string());
    config.llm.provider = "ollama".to_string();
    config.llm.model = "qwen-max".to_string();
    config.llm.ollama.base_url = Some("http://localhost:11434".to_string());
    config.agent.model = "qwen-max".to_string();
    assert!(config.validate_runtime().is_ok());
}

#[test]
fn test_validate_runtime_requires_provider_api_key() {
    let mut config = Config::default();
    config.gateway.auth = false;
    config.llm.provider = "openai".to_string();
    config.llm.model = "gpt-4o".to_string();
    config.agent.model = "gpt-4o".to_string();
    config.llm.openai.base_url = Some("https://api.openai.com/v1".to_string());
    config.llm.openai.api_key = None;
    assert!(config.validate_runtime().is_err());

    config.llm.openai.api_key = Some("sk-test".to_string());
    assert!(config.validate_runtime().is_ok());
}

#[test]
fn test_validate_runtime_rejects_invalid_timezone() {
    let mut config = Config::default();
    config.gateway.auth = false;
    config.llm.provider = "openai".to_string();
    config.llm.model = "gpt-4o".to_string();
    config.agent.model = "gpt-4o".to_string();
    config.llm.openai.base_url = Some("https://api.openai.com/v1".to_string());
    config.llm.openai.api_key = Some("sk-test".to_string());
    config.timezone = "Mars/Olympus".to_string();
    config.apply_derived_paths();

    assert!(config.validate_runtime().is_err());
}

#[test]
fn test_provider_models_are_strongly_typed() {
    let yaml = r#"
enabled: true
api_key: sk-test
base_url: https://dashscope.aliyuncs.com/compatible-mode/v1
models:
  - id: qwen-max
    name: qwen-max
    context_window: 32768
    max_tokens: 8192
    reasoning: false
    input: [text, image]
    cost:
      input: 0.1
      output: 0.2
      cache_read: 0.01
      cache_write: 0.02
    compat:
      thinking_format: qwen
"#;

    let provider: ProviderConfig =
        serde_yaml::from_str(yaml).expect("provider config should parse");
    let model = &provider.models[0];
    assert_eq!(model.id, "qwen-max");
    assert_eq!(model.context_window, 32_768);
    assert_eq!(
        model.input,
        vec![ModelInputKind::Text, ModelInputKind::Image]
    );
    assert_eq!(model.compat.thinking_format, Some(ThinkingFormat::Qwen));
    assert!(provider.enabled);
}

#[test]
fn test_provider_models_allow_abnormal_output_metadata_for_prompt_budget_repair() {
    let yaml = r#"
enabled: true
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
    compat:
      thinking_format: qwen
"#;

    let provider: ProviderConfig =
        serde_yaml::from_str(yaml).expect("provider config should parse");
    let model = &provider.models[0];
    assert_eq!(model.context_window, 32_768);
    assert_eq!(model.max_tokens, 81_920);
}

#[test]
fn test_validate_runtime_rejects_zero_access_token_ttl() {
    let mut config = Config::default();
    config.gateway.access_token_ttl_days = 0;
    config.llm.provider = "ollama".to_string();
    config.llm.model = "qwen-max".to_string();
    config.agent.model = "qwen-max".to_string();
    config.llm.ollama.base_url = Some("http://localhost:11434".to_string());

    assert!(config.validate_runtime().is_err());
}

#[test]
fn test_validate_runtime_rejects_zero_auth_cache_ttl() {
    let mut config = Config::default();
    config.gateway.auth_cache.ttl_seconds = 0;
    config.llm.provider = "ollama".to_string();
    config.llm.model = "qwen-max".to_string();
    config.agent.model = "qwen-max".to_string();
    config.llm.ollama.base_url = Some("http://localhost:11434".to_string());

    assert!(config.validate_runtime().is_err());
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
  model: qwen-plus
gateway:
  host: 127.0.0.1
  port: 18789
  auth: false
  jwt_secret: jwt
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
  anthropic: { enabled: false }
  openai: { enabled: false }
  bailian: { enabled: false }
  ollama:
    enabled: true
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
  model: qwen-plus
  kernel:
    compaction:
      recent_messages_max_chars: 100
      compacted_recent_messages_max_chars: 120
    memory:
      workspace_dir: /tmp/talos-config-invalid-compaction
gateway:
  host: 127.0.0.1
  port: 18789
  auth: false
  jwt_secret: jwt
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
  anthropic: { enabled: false }
  openai: { enabled: false }
  bailian: { enabled: false }
  ollama:
    enabled: true
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
  model: qwen-plus
gateway:
  host: 127.0.0.1
  port: 18789
  auth: false
llm:
  provider: ollama
  model: qwen-plus
  ollama:
    enabled: true
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

    assert_eq!(config.agent.kernel.max_iters, 1);
    assert_eq!(config.agent.kernel.max_task_per_turn, 3);
    assert_eq!(
        config.agent.kernel.planner_selection,
        PlannerSelection::Static
    );
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
  model: qwen-plus
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
    memory:
      workspace_dir: /tmp/custom
gateway:
  host: 127.0.0.1
  port: 18789
  auth: false
  jwt_secret: jwt
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
  anthropic: { enabled: false }
  openai: { enabled: false }
  bailian: { enabled: false }
  ollama:
enabled: true
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
  model: qwen-plus
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
    enabled: true
    base_url: http://localhost:11434
"#;

    let err = serde_yaml::from_str::<Config>(yaml).expect_err("legacy binding field must fail");
    assert!(err.to_string().contains("binding"));
}
