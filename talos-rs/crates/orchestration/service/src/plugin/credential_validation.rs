use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{
    McpAuthentication, McpCredentialValues, McpServerDefinition, McpTransportDefinition,
};

pub(super) fn validate(
    server: &McpServerDefinition,
    values: &McpCredentialValues,
) -> TalosResult<()> {
    if values.token.as_deref() == Some("********")
        || values
            .env
            .iter()
            .chain(values.headers.iter())
            .flat_map(|map| map.values())
            .any(|value| value == "********")
    {
        return Err(invalid(
            "A redacted placeholder cannot be used as an MCP credential.",
        ));
    }
    match (&values.authentication, &values.token) {
        (McpAuthentication::None, None) => {}
        (McpAuthentication::Bearer | McpAuthentication::Header { .. }, Some(token))
            if !token.is_empty() => {}
        _ => return Err(invalid("MCP authentication and token values do not match.")),
    }
    match &server.transport {
        McpTransportDefinition::Stdio { env, .. } => {
            if !matches!(values.authentication, McpAuthentication::None)
                || values
                    .headers
                    .as_ref()
                    .is_some_and(|headers| !headers.is_empty())
            {
                return Err(invalid(
                    "Stdio MCP credentials only support secret environment values.",
                ));
            }
            if values.env.as_ref().is_some_and(|secrets| {
                secrets.iter().any(|(key, value)| {
                    key.is_empty()
                        || key.contains(['=', '\0'])
                        || value.contains('\0')
                        || env.contains_key(key)
                })
            }) {
                return Err(invalid(
                    "Secret MCP environment names are invalid or duplicate public values.",
                ));
            }
        }
        McpTransportDefinition::StreamableHttp { url, headers } => {
            if values.env.as_ref().is_some_and(|env| !env.is_empty()) {
                return Err(invalid(
                    "HTTP MCP credentials cannot contain environment values.",
                ));
            }
            let url = url::Url::parse(url).map_err(|error| {
                invalid("The MCP credential target URL is invalid.").with_source(error)
            })?;
            if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
                return Err(invalid(
                    "MCP credential targets cannot include user information or URL fragments.",
                ));
            }
            let loopback = url.host_str().is_some_and(|host| {
                host == "localhost"
                    || host
                        .trim_matches(['[', ']'])
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())
            });
            if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
                return Err(invalid(
                    "MCP credentials require HTTPS, except for loopback test servers.",
                ));
            }
            let mut names = std::collections::HashSet::new();
            for name in headers.keys() {
                insert_header(&mut names, name, false)?;
            }
            if let Some(secrets) = &values.headers {
                for (name, value) in secrets {
                    insert_header(&mut names, name, false)?;
                    validate_value(value)?;
                }
            }
            match &values.authentication {
                McpAuthentication::None => {}
                McpAuthentication::OAuth { .. } => {
                    return Err(invalid("Use the MCP OAuth authorization endpoint."))
                }
                McpAuthentication::Bearer => {
                    insert_header(&mut names, "authorization", true)?;
                }
                McpAuthentication::Header { name } => {
                    insert_header(&mut names, name, false)?;
                }
            }
            if let Some(token) = &values.token {
                validate_value(token)?;
            }
        }
    }
    Ok(())
}

fn insert_header(
    names: &mut std::collections::HashSet<http::HeaderName>,
    name: &str,
    authorization: bool,
) -> TalosResult<()> {
    let name = http::HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
        invalid("The MCP credential header name is invalid.").with_source(error)
    })?;
    if name.as_str().starts_with("mcp-")
        || matches!(
            name.as_str(),
            "host"
                | "content-length"
                | "content-type"
                | "accept"
                | "connection"
                | "transfer-encoding"
        )
        || (!authorization && name == http::header::AUTHORIZATION)
        || !names.insert(name)
    {
        return Err(invalid(
            "MCP credentials conflict with another header or a protocol-owned header.",
        ));
    }
    Ok(())
}
fn validate_value(value: &str) -> TalosResult<()> {
    http::HeaderValue::from_str(value)
        .map(|_| ())
        .map_err(|error| invalid("The MCP credential header value is invalid.").with_source(error))
}
fn invalid(message: &'static str) -> TalosError {
    TalosError::new(ErrorCode::InvalidRequest, message)
}
