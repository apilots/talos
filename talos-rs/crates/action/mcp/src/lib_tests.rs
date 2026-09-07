use std::collections::BTreeMap;

use super::*;

#[test]
fn service_error_preserves_typed_code_without_exposing_remote_payload() {
    use std::error::Error;
    let error = map_service_error(ServiceError::McpError(rmcp::model::ErrorData::new(
        rmcp::model::ErrorCode(-32601),
        "SECRET_REMOTE_MESSAGE",
        Some(serde_json::json!({"token": "SECRET"})),
    )));
    assert_eq!(
        error.to_api_error().message,
        "The MCP server does not support this method."
    );
    assert!(
        matches!(error.source().unwrap().downcast_ref::<ServiceError>(), Some(ServiceError::McpError(remote)) if remote.code.0 == -32601 && remote.data.is_none())
    );
    assert!(!error.to_string().contains("SECRET"));
}

#[cfg(unix)]
#[tokio::test]
async fn stdio_command_does_not_inherit_parent_environment() {
    let mut command = build_stdio_command(
        "/usr/bin/env",
        &[],
        &BTreeMap::from([("MCP_EXPLICIT".to_string(), "visible".to_string())]),
        None,
    );
    let output = command
        .output()
        .await
        .expect("environment command should run");
    let stdout = String::from_utf8(output.stdout).expect("environment output should be utf-8");

    assert!(stdout.lines().any(|line| line == "MCP_EXPLICIT=visible"));
    if let Ok(path) = std::env::var("PATH") {
        assert!(stdout.lines().any(|line| line == format!("PATH={path}")));
    }
    assert!(!stdout.lines().any(|line| {
        let name = line.split_once('=').map(|(name, _)| name).unwrap_or(line);
        filter_inherited_tool_environment([(name, "value")]).is_empty()
    }));
}
