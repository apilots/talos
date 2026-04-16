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
        "Search file contents with ripgrep to locate symbols, entrypoints, exports, module boundaries, routes, or call sites before reading files.\n\nUsage:\n- Use grep_files for content search tasks instead of shell grep or rg.\n- Use grep_files first when you need to narrow candidate files for code analysis.\n- After grep_files identifies the best file, switch to read_file on that file.\n- When the path question is about directory shape rather than content search, use list_dir instead.\n- Do not keep repeating near-identical searches once you already have enough matches to choose the next file."
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
