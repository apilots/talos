use std::time::Duration;

mod cleanup;
mod lifecycle;
mod oauth;
mod process;
pub use oauth::{
    OAuthAuthorization, OAuthCredentialSource, OAuthCredentialStore, OAuthCredentials,
};
mod protocol;
mod session;
pub use lifecycle::{McpConnection, McpConnectionInput, PreparedMcpUpdate, TalosMcpRuntime};
const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_TOOL_TIMEOUT: Duration = Duration::from_secs(120);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

pub fn mcp_tool_name(server: &str, tool: &str) -> String {
    format!("mcp__{server}__{tool}")
}

pub fn split_mcp_tool_name(name: &str) -> Option<(String, String)> {
    let rest = name.strip_prefix("mcp__")?;
    let (server, tool) = rest.split_once("__")?;
    if server.is_empty() || tool.is_empty() {
        return None;
    }
    Some((server.to_string(), tool.to_string()))
}

#[cfg(all(test, unix))]
#[path = "lifecycle_tests.rs"]
mod lifecycle_tests;

/// Private connection inputs supplied by the application credential owner.
#[derive(Clone, PartialEq)]
pub enum McpConnectionCredentials {
    Token(talos_types::McpCredentialValues),
    OAuth(OAuthCredentialSource),
}
