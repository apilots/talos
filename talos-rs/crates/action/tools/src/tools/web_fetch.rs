use async_trait::async_trait;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};

use crate::runtimes::web_fetch::{run, WebFetchRequest};
use crate::spec::{ToolBehaviorFlags, ToolResultPolicy};
use crate::{Tool, ToolContext, ToolOutputPayload};

pub struct WebFetchTool;

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "WebFetch"
    }

    fn internal_name(&self) -> &str {
        "web_fetch"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["web_fetch"]
    }

    fn description(&self) -> &str {
        "Fetch a public web page, extract readable content, and return a structured summary."
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
            max_inline_bytes: 64 * 1024,
            persist_large_output: true,
            preview_lines: 80,
        }
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["url"],
            "properties": {
                "url": {
                    "type": "string",
                    "description": "HTTP or HTTPS URL to fetch."
                },
                "prompt": {
                    "type": "string",
                    "description": "Optional caller hint describing what to focus on in the fetched page."
                },
                "max_chars": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum number of extracted characters to return."
                }
            }
        }))
    }

    async fn execute_payload(
        &self,
        input: Value,
        _context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let request: WebFetchRequest = serde_json::from_value(input)
            .map_err(|err| TalosError::ToolError(format!("invalid web_fetch arguments: {err}")))?;
        let response = run(&request).await?;
        ToolOutputPayload::success_json(&response, "serialize web_fetch response")
    }
}
