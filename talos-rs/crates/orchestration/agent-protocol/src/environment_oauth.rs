//! Trusted callback delivery to the existing Worker OAuth attempt, never a credential store.
use talos_types::{McpAuthenticationStartRequest, McpDefinitionRef};

pub enum McpOAuthCallbackTarget {
    Loopback,
    ControlPlaneRelay { uri: String },
}

pub struct McpAuthenticationStartParams {
    pub request: McpAuthenticationStartRequest,
    pub callback_target: McpOAuthCallbackTarget,
}

pub struct McpAuthenticationCallbackParams {
    pub server: McpDefinitionRef,
    pub attempt_id: String,
    pub state: String,
    pub issuer: Option<String>,
    pub outcome: McpOAuthCallbackOutcome,
}

pub enum McpOAuthCallbackOutcome {
    AuthorizationCode(String),
    ProviderErrorCode(String),
}
