use crate::{McpAuthentication, PluginInstallId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

/// Stable single-instance definition identity, independent of display names and endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum McpDefinitionRef {
    Plugin { install_id: PluginInstallId },
    Config { name: String },
}

/// Write-only values for a complete credential replacement. Omitted maps mean no secret entries.
/// These values must never be returned as a definition, status, event or tool result.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpCredentialValues {
    pub authentication: McpAuthentication,
    #[ts(optional)]
    pub token: Option<String>,
    #[ts(optional)]
    pub env: Option<BTreeMap<String, String>>,
    #[ts(optional)]
    pub headers: Option<BTreeMap<String, String>>,
}

impl std::fmt::Debug for McpCredentialValues {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpCredentialValues")
            .field("authentication", &self.authentication)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpCredentialsUpdateRequest {
    pub server: McpDefinitionRef,
    pub change: McpCredentialsChange,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum McpCredentialsChange {
    Replace { credentials: McpCredentialValues },
    Clear,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpAuthenticationReadRequest {
    pub server: McpDefinitionRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct McpAuthenticationResponse {
    pub server: McpDefinitionRef,
    pub authentication: McpAuthentication,
    pub state: McpAuthenticationState,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum McpAuthenticationState {
    Authorizing {
        #[serde(rename = "attemptId")]
        attempt_id: String,
        #[serde(rename = "expiresAt")]
        expires_at: i64,
    },
    Authenticated,
    Expired,
    Unconfigured,
    CredentialsStored,
    Missing,
    Failed {
        error: talos_error::ApiError,
    },
}

/// Imports one server object or one-entry mcpServers object. A bare unnamed object needs name.
#[derive(Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpServerImportRequest {
    pub json: String,
    #[ts(optional)]
    pub name: Option<String>,
    #[ts(optional)]
    pub enable: Option<bool>,
    pub conflict_policy: crate::PluginInstallConflictPolicy,
}

impl std::fmt::Debug for McpServerImportRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpServerImportRequest")
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpAuthenticationStartRequest {
    pub server: McpDefinitionRef,
    pub config: crate::McpOAuthConfig,
    #[ts(optional)]
    pub client_secret: Option<String>,
}
impl std::fmt::Debug for McpAuthenticationStartRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpAuthenticationStartRequest")
            .field("server", &self.server)
            .finish_non_exhaustive()
    }
}
#[derive(Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct McpAuthenticationStartResponse {
    pub server: McpDefinitionRef,
    pub attempt_id: String,
    pub authorization_url: String,
    /// Unix timestamp in seconds.
    pub expires_at: i64,
}
impl std::fmt::Debug for McpAuthenticationStartResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpAuthenticationStartResponse")
            .field("server", &self.server)
            .field("attempt_id", &self.attempt_id)
            .finish_non_exhaustive()
    }
}
