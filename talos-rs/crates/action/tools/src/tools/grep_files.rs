use async_trait::async_trait;
use serde_json::Value;
use talos_error::TalosResult;

use crate::runtimes::fs_search::{run, GrepFilesRequest};
use crate::{Tool, ToolContext, ToolOutput};

pub struct GrepFilesTool;

#[async_trait]
impl Tool for GrepFilesTool {
    fn name(&self) -> &str {
        "grep_files"
    }

    fn description(&self) -> &str {
        "Search the workspace or a provided path with ripgrep to locate entrypoints, symbols, modules, routes, or call sites before reading files. For a user-provided absolute path, verify the target first with list_dir or read_file."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["pattern"],
            "properties": {
                "pattern": { "type": "string" },
                "include": { "type": "string" },
                "path": { "type": "string" },
                "limit": { "type": "integer", "minimum": 1 }
            }
        }))
    }

    async fn execute(&self, input: Value, context: ToolContext) -> TalosResult<ToolOutput> {
        let request: GrepFilesRequest = serde_json::from_value(input).map_err(|err| {
            talos_error::TalosError::ToolError(format!("invalid grep_files arguments: {}", err))
        })?;
        let response = run(&request, &context).await?;
        Ok(ToolOutput {
            success: true,
            output: response.content,
            error: None,
        })
    }
}
