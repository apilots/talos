use super::{parse_yaml_root, YamlConfigStore};
use crate::RuntimeEnv;
use talos_error::{TalosError, TalosResult};
use talos_fs::FileSystem;

pub(super) fn load_servers(store: &YamlConfigStore) -> TalosResult<Vec<crate::McpServerConfig>> {
    let root = parse_yaml_root(&store.read_config_text()?)?;
    let Some(servers) = root
        .get("agent")
        .and_then(|value| value.get("kernel"))
        .and_then(|value| value.get("mcp"))
        .and_then(|value| value.get("servers"))
    else {
        return Ok(vec![]);
    };
    let mut value = serde_json::to_value(servers).map_err(|error| {
        TalosError::internal_with_source("The configured MCP definitions are invalid.", error)
    })?;
    crate::loader::resolve_value_placeholders(&mut value, &RuntimeEnv::load(&store.env_path)?);
    serde_json::from_value(value).map_err(|error| {
        TalosError::internal_with_source("The configured MCP definitions are invalid.", error)
    })
}

pub(super) fn save_server(
    store: &YamlConfigStore,
    previous: &crate::McpServerConfig,
    next: &crate::McpServerConfig,
) -> TalosResult<()> {
    let _lock = talos_fs::std_fs().with_file_lock(&store.lock_path())?;
    let mut root = parse_yaml_root(&store.read_config_text()?)?;
    let entries = root
        .get_mut("agent")
        .and_then(|value| value.get_mut("kernel"))
        .and_then(|value| value.get_mut("mcp"))
        .and_then(|value| value.get_mut("servers"))
        .and_then(serde_yaml::Value::as_sequence_mut)
        .ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::ResourceNotFound,
                "The configured MCP server was not found.",
            )
        })?;
    let mut matches = entries.iter_mut().filter(|entry| {
        entry.get("name").and_then(serde_yaml::Value::as_str) == Some(&previous.name)
    });
    let entry = matches.next().ok_or_else(|| {
        TalosError::new(
            talos_error::ErrorCode::ResourceNotFound,
            "The configured MCP server was not found.",
        )
    })?;
    if matches.next().is_some() || previous.name != next.name {
        return Err(TalosError::new(
            talos_error::ErrorCode::Conflict,
            "The configured MCP identity is ambiguous.",
        ));
    }
    let mut raw: crate::McpServerConfig =
        serde_yaml::from_value(entry.clone()).map_err(|error| {
            TalosError::internal_with_source("The configured MCP server is invalid.", error)
        })?;
    let mut resolved = serde_json::to_value(&raw).map_err(|error| {
        TalosError::internal_with_source("Failed to encode MCP configuration.", error)
    })?;
    crate::loader::resolve_value_placeholders(&mut resolved, &RuntimeEnv::load(&store.env_path)?);
    let current: crate::McpServerConfig = serde_json::from_value(resolved).map_err(|error| {
        TalosError::internal_with_source("The configured MCP server is invalid.", error)
    })?;
    if current != *previous {
        return Err(TalosError::new(
            talos_error::ErrorCode::Conflict,
            "The MCP configuration changed on disk. Reload it before updating credentials.",
        ));
    }
    raw.credentials = next.credentials.clone();
    // Preserve raw command, URL and environment placeholders. Credential changes only remove
    // migrated environment keys or the legacy inline bearer token.
    match (&mut raw.transport, &next.transport) {
        (
            crate::McpTransportConfig::Stdio { env, .. },
            crate::McpTransportConfig::Stdio { env: remaining, .. },
        ) => env.retain(|key, _| remaining.contains_key(key)),
        (
            crate::McpTransportConfig::Http { bearer_token, .. },
            crate::McpTransportConfig::Http { .. },
        ) => *bearer_token = None,
        _ => {
            return Err(TalosError::new(
                talos_error::ErrorCode::Conflict,
                "MCP credential updates cannot change transport.",
            ))
        }
    }
    *entry = serde_yaml::to_value(raw).map_err(|error| {
        TalosError::internal_with_source("Failed to encode the MCP configuration.", error)
    })?;
    let text = serde_yaml::to_string(&root).map_err(|error| {
        TalosError::internal_with_source("Failed to encode the configuration.", error)
    })?;
    talos_fs::std_fs().write_file(&store.config_path, &text)
}
