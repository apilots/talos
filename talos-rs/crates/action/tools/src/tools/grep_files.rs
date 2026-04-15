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
        "Search files with ripgrep to locate symbols, entrypoints, exports, module boundaries, routes, or call sites before reading files. This is the default search tool for repository exploration and source analysis. Prefer grep_files over shell grep or rg whenever the goal is to find code locations. After grep_files narrows the candidate files, use read_file on the most relevant paths instead of repeatedly searching with the same pattern. When you already have enough matches to choose the next file, stop searching and move to read_file."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["pattern"],
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Ripgrep pattern to search for, usually a regex or exact symbol text."
                },
                "include": {
                    "type": "string",
                    "description": "Optional file glob filter such as *.rs or src/**/*.ts to narrow searched files."
                },
                "path": {
                    "type": "string",
                    "description": "Optional absolute path or workspace-relative path to limit the search scope."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum number of matches to return."
                }
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
