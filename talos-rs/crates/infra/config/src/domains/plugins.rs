use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginsRuntimeConfig {
    #[serde(default = "default_sources", deserialize_with = "deserialize_sources")]
    pub sources: Vec<PluginSourceConfig>,

    #[serde(default = "default_roots")]
    pub roots: Vec<PluginRootConfig>,
}

impl Default for PluginsRuntimeConfig {
    fn default() -> Self {
        Self {
            sources: default_sources(),
            roots: default_roots(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginSourceConfig {
    #[serde(default = "default_source_type")]
    pub source_type: PluginSourceType,
    pub display_name: String,
    pub kind: PluginSourceKind,
    pub url: String,
    pub reference: String,
    #[serde(default)]
    pub sparse_paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginSourceType {
    Official,
    User,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginSourceKind {
    Git,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginRootConfig {
    pub kind: PluginRootKind,
    pub path: String,
    #[serde(default = "default_true")]
    pub read_only: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginRootKind {
    Skill,
    McpServer,
    Package,
    Feature,
}

fn default_sources() -> Vec<PluginSourceConfig> {
    vec![
        PluginSourceConfig {
            source_type: PluginSourceType::Official,
            display_name: "Apilots Skills".to_string(),
            kind: PluginSourceKind::Git,
            url: "https://github.com/apilots/skills".to_string(),
            reference: "main".to_string(),
            sparse_paths: vec!["skills".to_string()],
        },
        official_mcp_source(),
    ]
}

fn deserialize_sources<'de, D>(deserializer: D) -> Result<Vec<PluginSourceConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let mut sources = Vec::<PluginSourceConfig>::deserialize(deserializer)?;
    for official in [official_mcp_source()] {
        if !sources.iter().any(|source| {
            source.url == official.url
                && source.reference == official.reference
                && source.sparse_paths == official.sparse_paths
        }) {
            sources.push(official);
        }
    }
    Ok(sources)
}

/// Product-owned discovery source. A user-supplied source label grants no host authority.
pub fn official_mcp_source() -> PluginSourceConfig {
    PluginSourceConfig {
        source_type: PluginSourceType::Official,
        display_name: "Apilots MCPs".to_string(),
        kind: PluginSourceKind::Git,
        url: "https://github.com/apilots/mcps".to_string(),
        reference: "main".to_string(),
        sparse_paths: vec!["mcp".to_string()],
    }
}

fn default_roots() -> Vec<PluginRootConfig> {
    vec![PluginRootConfig {
        kind: PluginRootKind::Skill,
        path: "~/.agents/skills".to_string(),
        read_only: true,
    }]
}

fn default_source_type() -> PluginSourceType {
    PluginSourceType::User
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
#[path = "plugins_tests.rs"]
mod tests;
