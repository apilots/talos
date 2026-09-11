use std::collections::BTreeMap;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{
    McpAuthentication, McpCredentialValues, McpServerDefinition, McpTransportDefinition,
};

/// Known credential fields are extracted before Plugin receives a persistable definition.
/// Explicit secret maps cover service-specific names without per-field reference objects.
pub(super) fn extract_inline(
    server: &mut McpServerDefinition,
    credentials: &mut Option<McpCredentialValues>,
) -> TalosResult<()> {
    let mut values = credentials.take().unwrap_or(McpCredentialValues {
        authentication: McpAuthentication::None,
        token: None,
        env: None,
        headers: None,
    });
    match &mut server.transport {
        McpTransportDefinition::Stdio { env, .. } => {
            let names: Vec<_> = env
                .keys()
                .filter(|name| sensitive_name(name))
                .cloned()
                .collect();
            for name in names {
                insert(
                    values.env.get_or_insert_with(BTreeMap::new),
                    name.clone(),
                    env.remove(&name).expect("selected environment name"),
                )?;
            }
        }
        McpTransportDefinition::StreamableHttp { headers, .. } => {
            let names: Vec<_> = headers
                .keys()
                .filter(|name| sensitive_name(name))
                .cloned()
                .collect();
            for name in names {
                let value = headers.remove(&name).expect("selected header name");
                if name.eq_ignore_ascii_case("authorization") {
                    if !matches!(values.authentication, McpAuthentication::None)
                        || values.token.is_some()
                    {
                        return Err(invalid());
                    }
                    let (scheme, token) = value.split_once(' ').ok_or_else(invalid)?;
                    if !scheme.eq_ignore_ascii_case("bearer") || token.is_empty() {
                        return Err(TalosError::new(ErrorCode::InvalidRequest, "Only bearer Authorization headers can be imported. Use explicit header credentials for a supported custom header."));
                    }
                    values.authentication = McpAuthentication::Bearer;
                    values.token = Some(token.into());
                } else {
                    insert(
                        values.headers.get_or_insert_with(BTreeMap::new),
                        name,
                        value,
                    )?;
                }
            }
        }
    }
    if values.token.is_some()
        || !matches!(values.authentication, McpAuthentication::None)
        || values.env.as_ref().is_some_and(|env| !env.is_empty())
        || values
            .headers
            .as_ref()
            .is_some_and(|headers| !headers.is_empty())
    {
        *credentials = Some(values);
    }
    Ok(())
}

fn insert(map: &mut BTreeMap<String, String>, name: String, value: String) -> TalosResult<()> {
    if map
        .keys()
        .any(|existing| existing.eq_ignore_ascii_case(&name))
    {
        return Err(invalid());
    }
    map.insert(name, value);
    Ok(())
}
fn invalid() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "MCP credential inputs contain conflicting values.",
    )
}
pub(super) fn sensitive_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        "authorization",
        "token",
        "secret",
        "password",
        "api_key",
        "api-key",
        "apikey",
    ]
    .iter()
    .any(|part| name.contains(part))
}
