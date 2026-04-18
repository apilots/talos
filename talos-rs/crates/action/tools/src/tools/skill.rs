use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use talos_error::{TalosError, TalosResult};

use crate::spec::{ToolBehaviorFlags, ToolResultPolicy, ToolSource, ToolVisibility};
use crate::state::CatalogToolEntry;
use crate::{Tool, ToolContext, ToolOutput};

#[derive(Debug, Clone, Deserialize)]
pub struct SkillRequest {
    #[serde(default)]
    pub query: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SkillResponse {
    pub skills: Vec<CatalogToolEntry>,
}

pub struct SkillTool;

#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &str {
        "Skill"
    }

    fn internal_name(&self) -> &str {
        "skill"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["skill"]
    }

    fn description(&self) -> &str {
        "Enumerate discoverable skill tools from the current catalog snapshot."
    }

    fn source(&self) -> ToolSource {
        ToolSource::Skill
    }

    fn visibility(&self) -> ToolVisibility {
        ToolVisibility::Deferred
    }

    fn behavior_flags(&self) -> ToolBehaviorFlags {
        ToolBehaviorFlags {
            read_only: true,
            destructive: false,
            concurrency_safe: true,
            requires_user_interaction: false,
        }
    }

    fn result_policy(&self) -> ToolResultPolicy {
        ToolResultPolicy {
            max_inline_bytes: 16 * 1024,
            persist_large_output: false,
            preview_lines: 40,
        }
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Optional keyword filter applied to skill name and description."
                }
            }
        }))
    }

    async fn execute(&self, input: Value, context: ToolContext) -> TalosResult<ToolOutput> {
        let request: SkillRequest = serde_json::from_value(input)
            .map_err(|err| TalosError::ToolError(format!("invalid skill arguments: {err}")))?;
        let query = request
            .query
            .as_ref()
            .map(|value| value.trim().to_ascii_lowercase());

        let skills = context
            .tool_catalog_state()
            .list()
            .into_iter()
            .filter(is_skill_entry)
            .filter(|entry| match query.as_deref() {
                Some(query) if query.is_empty() => true,
                Some(query) => {
                    entry.public_name.to_ascii_lowercase().contains(query)
                        || entry.description.to_ascii_lowercase().contains(query)
                }
                None => true,
            })
            .collect::<Vec<_>>();
        let response = SkillResponse { skills };

        Ok(ToolOutput {
            success: true,
            output: serde_json::to_string(&response)
                .map_err(|err| TalosError::ToolError(format!("serialize skill response: {err}")))?,
            error: None,
        })
    }
}

fn is_skill_entry(entry: &CatalogToolEntry) -> bool {
    entry.public_name.starts_with("skill_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn skill_tool_filters_skill_entries() {
        let session_id = format!("session-{}", Uuid::new_v4());
        let context = ToolContext::new(session_id, "user-1").with_catalog_snapshot(vec![
            CatalogToolEntry {
                public_name: "skill_write_spec".to_string(),
                description: "Write specification drafts".to_string(),
                deferred: true,
                available: true,
                unavailable_reason: None,
            },
            CatalogToolEntry {
                public_name: "web_search".to_string(),
                description: "Search the web".to_string(),
                deferred: false,
                available: true,
                unavailable_reason: None,
            },
        ]);

        let tool = SkillTool;
        let result = tool
            .execute(serde_json::json!({}), context)
            .await
            .expect("skill tool should succeed");

        assert!(result.output.contains("skill_write_spec"));
        assert!(!result.output.contains("web_search"));
    }
}
