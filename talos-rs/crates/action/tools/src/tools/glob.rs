use async_trait::async_trait;
use globset::Glob;
use serde::Deserialize;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use walkdir::WalkDir;

use crate::runtimes::fs_read::resolve_tool_path;
use crate::spec::{ToolBehaviorFlags, ToolResultPolicy};
use crate::{Tool, ToolContext, ToolOutput};

pub struct GlobTool;

#[derive(Debug, Deserialize)]
struct GlobInput {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "Glob"
    }

    fn internal_name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Find files by glob pattern within the workspace."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["pattern"],
            "properties": {
                "pattern": { "type": "string" },
                "path": { "type": "string" },
                "limit": { "type": "integer", "minimum": 1 }
            }
        }))
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
            max_inline_bytes: 32 * 1024,
            persist_large_output: false,
            preview_lines: 100,
        }
    }

    async fn execute(&self, input: Value, context: ToolContext) -> TalosResult<ToolOutput> {
        let input: GlobInput = serde_json::from_value(input)
            .map_err(|err| TalosError::ToolError(format!("invalid Glob arguments: {}", err)))?;
        let base = if let Some(path) = input.path.as_deref() {
            resolve_tool_path(path, &context)?
        } else {
            crate::runtimes::resolve_base_dir(&context)?
        };
        let matcher = Glob::new(&input.pattern)
            .map_err(|err| TalosError::ToolError(format!("invalid glob pattern: {}", err)))?
            .compile_matcher();
        let limit = input.limit.unwrap_or(200).min(2_000);
        let mut results = Vec::new();
        for entry in WalkDir::new(&base).into_iter().filter_map(Result::ok) {
            if !entry.file_type().is_file() {
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(&base)
                .unwrap_or(entry.path())
                .to_path_buf();
            if matcher.is_match(&relative) || matcher.is_match(entry.path()) {
                results.push(relative.display().to_string());
                if results.len() >= limit {
                    break;
                }
            }
        }

        Ok(ToolOutput {
            success: true,
            output: serde_json::to_string(&results)
                .map_err(|err| TalosError::ToolError(format!("serialize glob results: {}", err)))?,
            error: None,
        })
    }
}
