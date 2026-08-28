use std::collections::BTreeMap;

use super::*;

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
