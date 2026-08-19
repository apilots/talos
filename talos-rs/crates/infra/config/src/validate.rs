use serde_json::Value;
use std::path::{Component, Path};

use talos_error::TalosResult;

use crate::{Config, DatabaseType};

const REQUIRED_CONFIG_PATHS: &[&str] = &[
    "agent.name",
    "gateway.host",
    "gateway.port",
    "gateway.auth",
    "llm.provider",
    "llm.model",
];

const FORBIDDEN_CONFIG_PATHS: &[&str] = &["logging.file", "storage.connection_string"];

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

    Err(TalosError::public(
        talos_error::ErrorCode::InvalidRequest,
        format!("missing required config key(s): {}", missing.join(", ")),
    ))
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

    Err(TalosError::public(
        talos_error::ErrorCode::InvalidRequest,
        format!(
            "config key(s) are derived from talos_home and cannot be configured directly: {}",
            present.join(", ")
        ),
    ))
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

    let profile_retention = &config.automation.profile_usage_retention;
    if profile_retention.cron.trim().is_empty() {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "automation.profile_usage_retention.cron must not be empty".to_string(),
        ));
    }
    let mut cron = croner::Cron::new(&profile_retention.cron);
    cron.with_seconds_required().with_dom_and_dow();
    cron.parse().map_err(|error| {
        TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            format!("automation.profile_usage_retention.cron is invalid: {error}"),
        )
    })?;
    if profile_retention.retention_days == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "automation.profile_usage_retention.retention_days must be > 0".to_string(),
        ));
    }
    if profile_retention.batch_size == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "automation.profile_usage_retention.batch_size must be > 0".to_string(),
        ));
    }
    if profile_retention.max_batches == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "automation.profile_usage_retention.max_batches must be > 0".to_string(),
        ));
    }

    let memory_maintenance = &config.automation.memory_daily_maintenance;
    if memory_maintenance.cron.trim().is_empty() {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "automation.memory_daily_maintenance.cron must not be empty".to_string(),
        ));
    }
    let mut cron = croner::Cron::new(&memory_maintenance.cron);
    cron.with_seconds_required().with_dom_and_dow();
    cron.parse().map_err(|error| {
        TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            format!("automation.memory_daily_maintenance.cron is invalid: {error}"),
        )
    })?;
    if memory_maintenance.batch_size == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "automation.memory_daily_maintenance.batch_size must be > 0".to_string(),
        ));
    }
    if memory_maintenance.max_batches == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "automation.memory_daily_maintenance.max_batches must be > 0".to_string(),
        ));
    }

    if config.agent.name.trim().is_empty() || is_unresolved_placeholder(&config.agent.name) {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.name is empty or unresolved placeholder".to_string(),
        ));
    }
    if config.agent.message_queue.max_messages_per_session == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.message_queue.max_messages_per_session must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.tool_parallelism == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.tool_parallelism must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.tool_result_max_bytes == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.tool_result_max_bytes must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.context_compaction_max_bytes == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.context_compaction_max_bytes must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.default_workspace_id.trim().is_empty()
        || is_unresolved_placeholder(&config.agent.kernel.default_workspace_id)
    {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.default_workspace_id is empty or unresolved placeholder".to_string(),
        ));
    }
    if config.agent.kernel.workspace_dir.trim().is_empty()
        || is_unresolved_placeholder(&config.agent.kernel.workspace_dir)
    {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.workspace_dir is empty or unresolved placeholder".to_string(),
        ));
    }
    if config.memory.file_list_default_limit == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "memory.file_list_default_limit must be > 0".to_string(),
        ));
    }
    if config.memory.file_list_max_limit == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "memory.file_list_max_limit must be > 0".to_string(),
        ));
    }
    if config.memory.file_list_default_limit >= config.memory.file_list_max_limit {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "memory.file_list_default_limit must be < memory.file_list_max_limit".to_string(),
        ));
    }
    if config.memory.root_memory_filename.trim() != "MEMORY.md" {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "memory.root_memory_filename must be MEMORY.md".to_string(),
        ));
    }
    let memory_dirname = config.memory.memory_dirname.as_str();
    let mut memory_components = Path::new(memory_dirname).components();
    if memory_dirname.trim() != memory_dirname
        || !matches!(memory_components.next(), Some(Component::Normal(_)))
        || memory_components.next().is_some()
    {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "memory.memory_dirname must be one relative directory name".to_string(),
        ));
    }
    if config.agent.kernel.prompt.bootstrap.max_chars_per_file == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.prompt.bootstrap.max_chars_per_file must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.prompt.window.compaction_buffer_tokens == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.prompt.window.compaction_buffer_tokens must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.compaction.trigger_ratio_percent == 0
        || config.agent.kernel.compaction.trigger_ratio_percent >= 100
    {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.trigger_ratio_percent must be in 1..100".to_string(),
        ));
    }
    if config.agent.kernel.compaction.hard_ratio_percent == 0
        || config.agent.kernel.compaction.hard_ratio_percent > 100
    {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.hard_ratio_percent must be in 1..=100".to_string(),
        ));
    }
    if config.agent.kernel.compaction.trigger_ratio_percent
        >= config.agent.kernel.compaction.hard_ratio_percent
    {
        return Err(TalosError::public(talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.trigger_ratio_percent must be < agent.kernel.compaction.hard_ratio_percent".to_string(),
        ));
    }
    if config.agent.kernel.compaction.recent_messages_keep == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.recent_messages_keep must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.compaction.recent_messages_max_chars == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.recent_messages_max_chars must be > 0".to_string(),
        ));
    }
    if config
        .agent
        .kernel
        .compaction
        .compacted_recent_messages_max_chars
        == 0
    {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.compacted_recent_messages_max_chars must be > 0".to_string(),
        ));
    }
    if config
        .agent
        .kernel
        .compaction
        .compacted_recent_messages_max_chars
        > config.agent.kernel.compaction.recent_messages_max_chars
    {
        return Err(TalosError::public(talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.compacted_recent_messages_max_chars must be <= agent.kernel.compaction.recent_messages_max_chars".to_string(),
        ));
    }
    if config
        .agent
        .kernel
        .compaction
        .compacted_recent_messages_keep
        == 0
    {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.compacted_recent_messages_keep must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.compaction.recent_summaries_keep == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.recent_summaries_keep must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.compaction.summary_max_chars == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.summary_max_chars must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.compaction.compacted_summary_max_chars == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.compacted_summary_max_chars must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.compaction.compacted_summary_max_chars
        > config.agent.kernel.compaction.summary_max_chars
    {
        return Err(TalosError::public(talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.compacted_summary_max_chars must be <= agent.kernel.compaction.summary_max_chars".to_string(),
        ));
    }
    if config.agent.kernel.compaction.source_recent_messages_limit == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.source_recent_messages_limit must be > 0".to_string(),
        ));
    }
    if config
        .agent
        .kernel
        .compaction
        .source_recent_tool_results_limit
        == 0
    {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.source_recent_tool_results_limit must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.compaction.tool_output_max_chars == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.tool_output_max_chars must be > 0".to_string(),
        ));
    }
    if config
        .agent
        .kernel
        .compaction
        .compacted_tool_output_max_chars
        == 0
    {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.compacted_tool_output_max_chars must be > 0".to_string(),
        ));
    }
    if config
        .agent
        .kernel
        .compaction
        .compacted_tool_output_max_chars
        > config.agent.kernel.compaction.tool_output_max_chars
    {
        return Err(TalosError::public(talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.compacted_tool_output_max_chars must be <= agent.kernel.compaction.tool_output_max_chars".to_string(),
        ));
    }
    if config
        .agent
        .kernel
        .compaction
        .source_recent_tool_exchanges_limit
        == 0
    {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.source_recent_tool_exchanges_limit must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.compaction.tool_exchange_max_chars == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.tool_exchange_max_chars must be > 0".to_string(),
        ));
    }
    if config
        .agent
        .kernel
        .compaction
        .compacted_tool_exchange_max_chars
        == 0
    {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.compacted_tool_exchange_max_chars must be > 0".to_string(),
        ));
    }
    if config
        .agent
        .kernel
        .compaction
        .compacted_tool_exchange_max_chars
        > config.agent.kernel.compaction.tool_exchange_max_chars
    {
        return Err(TalosError::public(talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.compacted_tool_exchange_max_chars must be <= agent.kernel.compaction.tool_exchange_max_chars".to_string(),
        ));
    }
    if config.agent.kernel.compaction.retrieved_memories_keep == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.retrieved_memories_keep must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.compaction.retrieved_memory_max_chars == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.retrieved_memory_max_chars must be > 0".to_string(),
        ));
    }
    if config
        .agent
        .kernel
        .compaction
        .compacted_retrieved_memory_max_chars
        == 0
    {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.compacted_retrieved_memory_max_chars must be > 0".to_string(),
        ));
    }
    if config
        .agent
        .kernel
        .compaction
        .compacted_retrieved_memory_max_chars
        > config.agent.kernel.compaction.retrieved_memory_max_chars
    {
        return Err(TalosError::public(talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.compaction.compacted_retrieved_memory_max_chars must be <= agent.kernel.compaction.retrieved_memory_max_chars".to_string(),
        ));
    }
    if let Some(model) = config.agent.kernel.compaction.model.as_ref() {
        if model.trim().is_empty() || is_unresolved_placeholder(model) {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "agent.kernel.compaction.model is empty or unresolved placeholder".to_string(),
            ));
        }
    }
    if config.agent.kernel.output.max_tokens == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.output.max_tokens must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.output.upper_limit == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.output.upper_limit must be > 0".to_string(),
        ));
    }
    if config.agent.kernel.output.max_tokens > config.agent.kernel.output.upper_limit {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent.kernel.output.max_tokens must be <= agent.kernel.output.upper_limit".to_string(),
        ));
    }
    match &config.agent.kernel.prompt.window.reserved_output_tokens {
        crate::ReservedOutputPolicy::UseModelDefault => {}
        crate::ReservedOutputPolicy::Fixed { tokens } => {
            if *tokens == 0 {
                return Err(TalosError::public(
                    talos_error::ErrorCode::InvalidRequest,
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
                return Err(TalosError::public(
                    talos_error::ErrorCode::InvalidRequest,
                    "agent.kernel.prompt.window.reserved_output_tokens.ratio must be > 0"
                        .to_string(),
                ));
            }
        }
    }
    if config.gateway.host.trim().is_empty() || is_unresolved_placeholder(&config.gateway.host) {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "gateway.host is empty or unresolved placeholder".to_string(),
        ));
    }
    if config.gateway.port == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "gateway.port must be > 0".to_string(),
        ));
    }
    if config.gateway.auth && config.cloud.api_base_url.trim().is_empty() {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "cloud.api_base_url must not be empty when gateway.auth=true".to_string(),
        ));
    }
    if config.gateway.auth && config.cloud.web_base_url.trim().is_empty() {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "cloud.web_base_url must not be empty when gateway.auth=true".to_string(),
        ));
    }
    if config.gateway.auth && config.cloud.auth.introspection_cache_ttl_seconds == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "cloud.auth.introspection_cache_ttl_seconds must be > 0".to_string(),
        ));
    }

    if config.logging.level.trim().is_empty() || is_unresolved_placeholder(&config.logging.level) {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "logging.level is empty or unresolved placeholder".to_string(),
        ));
    }
    if config.logging.format.trim().is_empty() || is_unresolved_placeholder(&config.logging.format)
    {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "logging.format is empty or unresolved placeholder".to_string(),
        ));
    }
    if config.logging.format != "text" && config.logging.format != "json" {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "logging.format must be one of: text, json".to_string(),
        ));
    }

    if config.storage.database_type != DatabaseType::Sqlite {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            format!(
                "unsupported storage.database_type: {}",
                config.storage.database_type
            ),
        ));
    }
    if config.storage.connection_string.trim().is_empty()
        || is_unresolved_placeholder(&config.storage.connection_string)
    {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "storage.connection_string is empty or unresolved placeholder".to_string(),
        ));
    }
    if config.storage.max_connections == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "storage.max_connections must be > 0".to_string(),
        ));
    }
    config.storage.files.validate()?;

    Ok(())
}
