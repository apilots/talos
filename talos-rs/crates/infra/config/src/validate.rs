use serde_json::Value;

use talos_error::TalosResult;

use crate::{Config, DatabaseType};

const REQUIRED_CONFIG_PATHS: &[&str] = &[
    "agent.name",
    "agent.model",
    "gateway.host",
    "gateway.port",
    "gateway.token",
    "gateway.auth",
    "llm.provider",
    "llm.model",
];

const FORBIDDEN_CONFIG_PATHS: &[&str] = &[
    "logging.file",
    "storage.connection_string",
    "agent.kernel.memory.workspace_dir",
];

pub(crate) fn validate_required_paths(root: &Value) -> TalosResult<()> {
    use talos_error::TalosError;

    let missing: Vec<&str> = REQUIRED_CONFIG_PATHS
        .iter()
        .copied()
        .filter(|path| !has_json_path(root, path))
        .collect();

    if missing.is_empty() {
        return Ok(());
    }

    Err(TalosError::InvalidConfiguration(format!(
        "missing required config key(s): {}",
        missing.join(", ")
    )))
}

pub(crate) fn validate_forbidden_paths(root: &Value) -> TalosResult<()> {
    use talos_error::TalosError;

    let present: Vec<&str> = FORBIDDEN_CONFIG_PATHS
        .iter()
        .copied()
        .filter(|path| has_json_path(root, path))
        .collect();

    if present.is_empty() {
        return Ok(());
    }

    Err(TalosError::InvalidConfiguration(format!(
        "config key(s) are derived from talos_home and cannot be configured directly: {}",
        present.join(", ")
    )))
}

fn has_json_path(root: &Value, path: &str) -> bool {
    let mut node = root;
    for seg in path.split('.') {
        match node {
            Value::Object(map) => {
                let Some(next) = map.get(seg) else {
                    return false;
                };
                node = next;
            }
            _ => return false,
        }
    }
    true
}

pub(crate) fn is_unresolved_placeholder(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with("${") && trimmed.ends_with('}')
}

pub(crate) fn validate_runtime_config(config: &Config) -> TalosResult<()> {
    use talos_error::TalosError;

    crate::parse_timezone(&config.timezone)?;

    if config.agent.name.trim().is_empty() || is_unresolved_placeholder(&config.agent.name) {
        return Err(TalosError::InvalidConfiguration(
            "agent.name is empty or unresolved placeholder".to_string(),
        ));
    }
    if config.agent.model.trim().is_empty() || is_unresolved_placeholder(&config.agent.model) {
        return Err(TalosError::InvalidConfiguration(
            "agent.model is empty or unresolved placeholder".to_string(),
        ));
    }
    if config.agent.kernel.max_iters == 0 {
        return Err(TalosError::InvalidConfiguration(
            "agent.kernel.max_iters must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.max_task_per_turn == 0 {
        return Err(TalosError::InvalidConfiguration(
            "agent.kernel.max_task_per_turn must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.tool_parallelism == 0 {
        return Err(TalosError::InvalidConfiguration(
            "agent.kernel.tool_parallelism must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.max_llm_calls == 0 {
        return Err(TalosError::InvalidConfiguration(
            "agent.kernel.max_llm_calls must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.max_tool_rounds == 0 {
        return Err(TalosError::InvalidConfiguration(
            "agent.kernel.max_tool_rounds must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.tool_result_max_bytes == 0 {
        return Err(TalosError::InvalidConfiguration(
            "agent.kernel.tool_result_max_bytes must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.context_compaction_max_bytes == 0 {
        return Err(TalosError::InvalidConfiguration(
            "agent.kernel.context_compaction_max_bytes must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.default_workspace_id.trim().is_empty()
        || is_unresolved_placeholder(&config.agent.kernel.default_workspace_id)
    {
        return Err(TalosError::InvalidConfiguration(
            "agent.kernel.default_workspace_id is empty or unresolved placeholder".to_string(),
        ));
    }
    if config.agent.kernel.execution_children == 0 {
        return Err(TalosError::InvalidConfiguration(
            "agent.kernel.execution_children must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.execution_loop_iters == 0 {
        return Err(TalosError::InvalidConfiguration(
            "agent.kernel.execution_loop_iters must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.memory.workspace_dir.trim().is_empty()
        || is_unresolved_placeholder(&config.agent.kernel.memory.workspace_dir)
    {
        return Err(TalosError::InvalidConfiguration(
            "agent.kernel.memory.workspace_dir is empty or unresolved placeholder".to_string(),
        ));
    }
    if config.agent.kernel.memory.hot_recent_messages == 0 {
        return Err(TalosError::InvalidConfiguration(
            "agent.kernel.memory.hot_recent_messages must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.prompt.session_recent_messages_limit == 0 {
        return Err(TalosError::InvalidConfiguration(
            "agent.kernel.prompt.session_recent_messages_limit must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.prompt.session_summary_recent_count == 0 {
        return Err(TalosError::InvalidConfiguration(
            "agent.kernel.prompt.session_summary_recent_count must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.prompt.session_summary_max_chars == 0 {
        return Err(TalosError::InvalidConfiguration(
            "agent.kernel.prompt.session_summary_max_chars must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.memory.compaction_threshold_bytes == 0 {
        return Err(TalosError::InvalidConfiguration(
            "agent.kernel.memory.compaction_threshold_bytes must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.memory.file_list_default_limit == 0 {
        return Err(TalosError::InvalidConfiguration(
            "agent.kernel.memory.file_list_default_limit must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.memory.file_list_max_limit == 0 {
        return Err(TalosError::InvalidConfiguration(
            "agent.kernel.memory.file_list_max_limit must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.memory.file_list_default_limit
        >= config.agent.kernel.memory.file_list_max_limit
    {
        return Err(TalosError::InvalidConfiguration(
            "agent.kernel.memory.file_list_default_limit must be < agent.kernel.memory.file_list_max_limit".to_string(),
        ));
    }
    if config.agent.kernel.memory.root_memory_filename.trim() != "MEMORY.md" {
        return Err(TalosError::InvalidConfiguration(
            "agent.kernel.memory.root_memory_filename must be MEMORY.md".to_string(),
        ));
    }
    if config.agent.kernel.prompt.bootstrap.max_chars_per_file == 0 {
        return Err(TalosError::InvalidConfiguration(
            "agent.kernel.prompt.bootstrap.max_chars_per_file must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.prompt.bootstrap.max_chars_total == 0 {
        return Err(TalosError::InvalidConfiguration(
            "agent.kernel.prompt.bootstrap.max_chars_total must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.prompt.bootstrap.max_chars_total
        < config.agent.kernel.prompt.bootstrap.max_chars_per_file
    {
        return Err(TalosError::InvalidConfiguration(
            "agent.kernel.prompt.bootstrap.max_chars_total must be >= agent.kernel.prompt.bootstrap.max_chars_per_file".to_string(),
        ));
    }
    if config.agent.kernel.prompt.window.compaction_buffer_tokens == 0 {
        return Err(TalosError::InvalidConfiguration(
            "agent.kernel.prompt.window.compaction_buffer_tokens must be > 0".to_string(),
        ));
    }
    match &config.agent.kernel.prompt.window.reserved_output_tokens {
        crate::ReservedOutputPolicy::UseModelDefault => {}
        crate::ReservedOutputPolicy::Fixed { tokens } => {
            if *tokens == 0 {
                return Err(TalosError::InvalidConfiguration(
                    "agent.kernel.prompt.window.reserved_output_tokens.fixed.tokens must be > 0"
                        .to_string(),
                ));
            }
        }
        crate::ReservedOutputPolicy::Ratio {
            numerator,
            denominator,
        } => {
            if *numerator == 0 || *denominator == 0 {
                return Err(TalosError::InvalidConfiguration(
                    "agent.kernel.prompt.window.reserved_output_tokens.ratio must be > 0"
                        .to_string(),
                ));
            }
        }
    }
    if config.gateway.host.trim().is_empty() || is_unresolved_placeholder(&config.gateway.host) {
        return Err(TalosError::InvalidConfiguration(
            "gateway.host is empty or unresolved placeholder".to_string(),
        ));
    }
    if config.gateway.port == 0 {
        return Err(TalosError::InvalidConfiguration(
            "gateway.port must be > 0".to_string(),
        ));
    }
    let Some(token) = config.gateway.token.as_ref() else {
        return Err(TalosError::InvalidConfiguration(
            "gateway.token is required".to_string(),
        ));
    };
    if token.trim().is_empty() || is_unresolved_placeholder(token) {
        return Err(TalosError::InvalidConfiguration(
            "gateway.token is empty or unresolved placeholder".to_string(),
        ));
    }

    if config.gateway.auth {
        let Some(secret) = config.gateway.jwt_secret.as_ref() else {
            return Err(TalosError::InvalidConfiguration(
                "gateway.jwt_secret is required when gateway.auth=true".to_string(),
            ));
        };
        if secret.trim().is_empty() || is_unresolved_placeholder(secret) {
            return Err(TalosError::InvalidConfiguration(
                "gateway.jwt_secret is empty or unresolved placeholder".to_string(),
            ));
        }
    }

    if config.logging.level.trim().is_empty() || is_unresolved_placeholder(&config.logging.level) {
        return Err(TalosError::InvalidConfiguration(
            "logging.level is empty or unresolved placeholder".to_string(),
        ));
    }
    if config.logging.format.trim().is_empty() || is_unresolved_placeholder(&config.logging.format)
    {
        return Err(TalosError::InvalidConfiguration(
            "logging.format is empty or unresolved placeholder".to_string(),
        ));
    }
    if config.logging.format != "text" && config.logging.format != "json" {
        return Err(TalosError::InvalidConfiguration(
            "logging.format must be one of: text, json".to_string(),
        ));
    }

    if config.storage.database_type != DatabaseType::Sqlite {
        return Err(TalosError::InvalidConfiguration(format!(
            "unsupported storage.database_type: {}",
            config.storage.database_type
        )));
    }
    if config.storage.connection_string.trim().is_empty()
        || is_unresolved_placeholder(&config.storage.connection_string)
    {
        return Err(TalosError::InvalidConfiguration(
            "storage.connection_string is empty or unresolved placeholder".to_string(),
        ));
    }
    if config.storage.max_connections == 0 {
        return Err(TalosError::InvalidConfiguration(
            "storage.max_connections must be > 0".to_string(),
        ));
    }

    let provider_name = config.llm.provider.trim().to_ascii_lowercase();
    let provider = match provider_name.as_str() {
        "anthropic" => ("anthropic", &config.llm.anthropic),
        "openai" => ("openai", &config.llm.openai),
        "bailian" => ("bailian", &config.llm.bailian),
        "ollama" => ("ollama", &config.llm.ollama),
        other => {
            return Err(TalosError::InvalidConfiguration(format!(
                "unsupported llm.provider: {}",
                other
            )));
        }
    };

    if !provider.1.enabled {
        return Err(TalosError::InvalidConfiguration(format!(
            "llm provider '{}' is disabled",
            provider.0
        )));
    }

    if provider.0 != "ollama" {
        let Some(api_key) = provider.1.api_key.as_ref() else {
            return Err(TalosError::InvalidConfiguration(format!(
                "llm.{}.api_key is required",
                provider.0
            )));
        };
        if api_key.trim().is_empty() || is_unresolved_placeholder(api_key) {
            return Err(TalosError::InvalidConfiguration(format!(
                "llm.{}.api_key is empty or unresolved placeholder",
                provider.0
            )));
        }
    }

    if let Some(base_url) = provider.1.base_url.as_ref() {
        if base_url.trim().is_empty() || is_unresolved_placeholder(base_url) {
            return Err(TalosError::InvalidConfiguration(format!(
                "llm.{}.base_url is empty or unresolved placeholder",
                provider.0
            )));
        }
    } else if provider.0 == "bailian" || provider.0 == "ollama" {
        return Err(TalosError::InvalidConfiguration(format!(
            "llm.{}.base_url is required",
            provider.0
        )));
    }

    if config.llm.model.trim().is_empty() || is_unresolved_placeholder(&config.llm.model) {
        return Err(TalosError::InvalidConfiguration(
            "llm.model is empty or unresolved placeholder".to_string(),
        ));
    }

    if !provider.1.models.is_empty()
        && !provider
            .1
            .models
            .iter()
            .any(|model| model.id == config.llm.model)
    {
        return Err(TalosError::InvalidConfiguration(format!(
            "llm.model '{}' is not configured under llm.{}.models",
            config.llm.model, provider.0
        )));
    }

    Ok(())
}
