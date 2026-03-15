use async_trait::async_trait;
use serde_json::Value;
use std::fs;
use talos_error::TalosResult;

use crate::{Tool, ToolContext, ToolOutput};

/// Tool for reading file contents
pub struct ReadTool;

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Reads the contents of a file. Input should contain 'path' field."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": {"type": "string"}
            }
        }))
    }

    async fn execute(&self, input: Value, _context: ToolContext) -> TalosResult<ToolOutput> {
        // Extract path from input
        let path = if let Some(path_val) = input.get("path") {
            path_val.as_str().unwrap_or("")
        } else if input.is_string() {
            input.as_str().unwrap()
        } else {
            return Ok(ToolOutput {
                success: false,
                output: "".to_string(),
                error: Some("Input must contain 'path' field or be a string".to_string()),
            });
        };

        match fs::read_to_string(path) {
            Ok(content) => Ok(ToolOutput {
                success: true,
                output: content,
                error: None,
            }),
            Err(err) => Ok(ToolOutput {
                success: false,
                output: "".to_string(),
                error: Some(format!("Failed to read file '{}': {}", path, err)),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_read_tool_success() {
        // Create a temporary file
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "Hello, World!").unwrap();
        let path = temp_file.path().to_str().unwrap().to_string();

        let tool = ReadTool;
        let input = serde_json::json!({"path": path});
        let result = tool.execute(input, ToolContext::default()).await.unwrap();

        assert!(result.success);
        assert!(result.output.contains("Hello, World!"));
    }

    #[tokio::test]
    async fn test_read_tool_failure() {
        let tool = ReadTool;
        let input = serde_json::json!({"path": "/nonexistent/file.txt"});
        let result = tool.execute(input, ToolContext::default()).await.unwrap();

        assert!(!result.success);
        assert!(result.error.is_some());
        assert!(result
            .error
            .as_ref()
            .unwrap()
            .contains("Failed to read file"));
    }
}
