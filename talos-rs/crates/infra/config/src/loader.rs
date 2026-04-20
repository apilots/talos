use config::{Config as ExternalConfig, Environment, File};
use serde_json::Value;
use std::env;
use std::path::PathBuf;

use talos_error::TalosResult;

use crate::{validate, Config, TalosPaths};

pub(crate) fn load_config() -> TalosResult<Config> {
    load_config_from_path(None)
}

pub(crate) fn load_config_from_path(path: Option<&str>) -> TalosResult<Config> {
    use talos_error::TalosError;

    let paths = TalosPaths::resolve();
    paths.ensure_base_dirs()?;

    let config_path = match path {
        Some(path) => PathBuf::from(path),
        None => default_config_path(),
    };

    if !config_path.exists() {
        return Err(TalosError::InvalidConfiguration(format!(
            "no config file found at {}",
            config_path.display()
        )));
    }

    let builder = ExternalConfig::builder()
        .add_source(File::from(config_path).required(true))
        // Keep TALOS__ overrides for e2e/integration scenarios.
        .add_source(Environment::with_prefix("TALOS").separator("__"));

    // Build merged config values from file + TALOS__ environment overrides.
    let config = builder
        .build()
        .map_err(|e| TalosError::ConfigError(e.to_string()))?;

    // Require explicit top-level operational keys while allowing optional prompt knobs
    // to fall back to typed defaults when omitted from config files.
    let raw_value: Value = config
        .try_deserialize()
        .map_err(|e| TalosError::ConfigError(e.to_string()))?;
    validate::validate_required_paths(&raw_value)?;
    validate::validate_forbidden_paths(&raw_value)?;

    // Deserialize into typed config after strict key presence validation.
    let config: Config =
        serde_json::from_value(raw_value).map_err(|e| TalosError::ConfigError(e.to_string()))?;
    let mut config = resolve_env_placeholders(config)?;
    config.apply_derived_paths();
    config
        .talos_paths()
        .ensure_agent_workspace(&config.agent.name)?;

    Ok(config)
}

pub(crate) fn save_config(config: &Config, path: &str) -> TalosResult<()> {
    use talos_error::TalosError;

    let yaml = serde_yaml::to_string(config)
        .map_err(|e| TalosError::ConfigError(format!("Failed to serialize config: {}", e)))?;

    std::fs::write(path, yaml)
        .map_err(|e| TalosError::ConfigError(format!("Failed to write config file: {}", e)))?;

    Ok(())
}

pub(crate) fn default_config_path() -> PathBuf {
    TalosPaths::resolve().config_file
}

pub(crate) fn resolve_env_placeholders(config: Config) -> TalosResult<Config> {
    use talos_error::TalosError;

    let mut value = serde_json::to_value(config)
        .map_err(|e| TalosError::ConfigError(format!("Failed to encode config: {}", e)))?;
    resolve_value_placeholders(&mut value);
    serde_json::from_value(value)
        .map_err(|e| TalosError::ConfigError(format!("Failed to decode config: {}", e)))
}

pub(crate) fn resolve_value_placeholders(value: &mut Value) {
    match value {
        Value::String(text) => {
            *text = expand_env_in_string(text);
        }
        Value::Array(items) => {
            for item in items {
                resolve_value_placeholders(item);
            }
        }
        Value::Object(map) => {
            for v in map.values_mut() {
                resolve_value_placeholders(v);
            }
        }
        _ => {}
    }
}

pub(crate) fn expand_env_in_string(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut cursor = 0usize;
    while let Some(start_rel) = input[cursor..].find("${") {
        let start = cursor + start_rel;
        result.push_str(&input[cursor..start]);
        let after_start = start + 2;
        if let Some(end_rel) = input[after_start..].find('}') {
            let end = after_start + end_rel;
            let var_name = &input[after_start..end];
            let replacement = env::var(var_name).unwrap_or_else(|_| format!("${{{}}}", var_name));
            result.push_str(&replacement);
            cursor = end + 1;
        } else {
            result.push_str(&input[start..]);
            cursor = input.len();
        }
    }
    if cursor < input.len() {
        result.push_str(&input[cursor..]);
    }
    result
}

pub(crate) fn generate_default_config_content() -> String {
    r#"# Talos Configuration
# https://github.com/xiliu/talos

# Global timezone for user-visible time rendering
timezone: UTC

# Agent 配置
agent:
  name: jarvis
  model: claude-3-5-sonnet
  kernel:
    max_iters: 1
    max_task_per_turn: 3
    tool_parallelism: 1
    max_llm_calls: 8
    max_tool_rounds: 8
    tool_result_max_bytes: 65536
    context_compaction_max_bytes: 262144
    default_workspace_id: default
    planner_mode: built_in
    planner_selection: static
    execution_mode: single
    execution_children: 2
    execution_loop_iters: 3
    memory:
      hot_recent_messages: 64
      compaction_threshold_bytes: 262144
      root_memory_filename: MEMORY.md
      memory_dirname: memory
      workspace_memory_flush_enabled: true
      workspace_memory_flush_on_compaction: true
      flush_file_pattern: YYYY-MM-DD.md
      tool_search_enabled: true
      tool_get_enabled: true
      search_max_results: 6
      file_list_default_limit: 20
      file_list_max_limit: 90
      search_include_root_memory: true
      search_include_daily_memory: true
      search_include_sessions: false

# Gateway 配置
gateway:
  host: 127.0.0.1
  port: 18789
  token: ${GATEWAY_TOKEN}  # 环境变量
  jwt_secret: ${JWT_SECRET}
  auth: false
  access_token_ttl_days: 30
  allow_register: true

# 渠道配置
channels:
  whatsapp:
    enabled: false
    dm_policy: pairing
    allow_from: []
  
  telegram:
    enabled: false
    token: ${TELEGRAM_BOT_TOKEN}
    dm_policy: pairing
    dm_scope: per_channel_peer_topic
  
  discord:
    enabled: false
    token: ${DISCORD_BOT_TOKEN}
  
  feishu:
    enabled: false
    token: ${FEISHU_APP_ID}:${FEISHU_APP_SECRET}

# LLM 配置
llm:
  provider: anthropic
  model: claude-3-5-sonnet
  
  anthropic:
    api_key: ${ANTHROPIC_API_KEY}
    enabled: true
  
  openai:
    api_key: ${OPENAI_API_KEY}
    enabled: true

  bailian:
    api_key: ${BAILIAN_API_KEY}
    base_url: https://dashscope.aliyuncs.com/compatible-mode/v1
    enabled: true
    models:
      - id: qwen-max
        name: qwen-max
        context_window: 32768
        max_tokens: 8192
        reasoning: false
        input: [text]
        cost:
          input: 0
          output: 0
          cache_read: 0
          cache_write: 0
        compat: {}
      - id: qwen-plus
        name: qwen-plus
        context_window: 32768
        max_tokens: 8192
        reasoning: true
        input: [text]
        cost:
          input: 0
          output: 0
          cache_read: 0
          cache_write: 0
        compat:
          thinking_format: qwen
      - id: qwen-turbo
        name: qwen-turbo
        context_window: 32768
        max_tokens: 8192
        reasoning: false
        input: [text]
        cost:
          input: 0
          output: 0
          cache_read: 0
          cache_write: 0
        compat: {}
  
  ollama:
    base_url: http://localhost:11434
    enabled: true

# 日志配置
logging:
  level: info
  format: text  # json or text
  stdout: true

# Storage 配置
storage:
  database_type: sqlite
  max_connections: 5

"#
    .to_string()
}
