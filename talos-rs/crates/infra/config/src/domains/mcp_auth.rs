use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Public configuration contains only an opaque reference to one private credential bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct McpCredentialBinding {
    pub reference: String,
    pub authentication: McpAuthentication,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum McpAuthentication {
    None,
    #[serde(rename = "oauth")]
    OAuth {
        config: Box<McpOAuthConfig>,
    },
    Bearer,
    Header {
        name: String,
    },
}

/// Local authorization configuration. Confidential client secrets remain in private storage.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpOAuthConfig {
    #[ts(optional)]
    pub client_id: Option<String>,
    #[ts(optional)]
    pub client_metadata_url: Option<String>,
    #[ts(optional)]
    pub callback_port: Option<u16>,
    #[serde(default)]
    pub scopes: Vec<String>,
}
