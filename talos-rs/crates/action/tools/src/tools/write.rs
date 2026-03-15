use async_trait::async_trait;
use serde_json::Value;
use std::fs;
use talos_error::TalosResult;

use crate::{Tool, ToolContext, ToolOutput};

/// Tool for writing content to a file
pub struct WriteTool;

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Writes content to a file. Input should contain 'path' and 'content' fields."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["path", "content"],
            "properties": {
                "path": {"type": "string"},
                "content": {"type": "string"}
            }
        }))
    }

    async fn execute(&self, input: Value, _context: ToolContext) -> TalosResult<ToolOutput> {
        // Extract path and content from input
        let path = if let Some(path_val) = input.get("path") {
            path_val.as_str().unwrap_or("")
        } else {
            return Ok(ToolOutput {
                success: false,
                output: "".to_string(),
                error: Some("Input must contain 'path' field".to_string()),
            });
        };

        let content = if let Some(content_val) = input.get("content") {
            content_val.as_str().unwrap_or("")
        } else {
            return Ok(ToolOutput {
                success: false,
                output: "".to_string(),
                error: Some("Input must contain 'content' field".to_string()),
            });
        };

        match fs::write(path, content) {
            Ok(_) => Ok(ToolOutput {
                success: true,
                output: format!(
                    "Successfully wrote {} characters to {}",
                    content.len(),
                    path
                ),
                error: None,
            }),
            Err(err) => Ok(ToolOutput {
                success: false,
                output: "".to_string(),
                error: Some(format!("Failed to write to file '{}': {}", path, err)),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_write_tool_success() {
        // Create a temporary file
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap().to_string();
        std::fs::remove_file(&path).unwrap(); // Remove the temp file to write to it

        let tool = WriteTool;
        let input = serde_json::json!({
            "path": path,
            "content": "Hello, World!"
        });
        let result = tool.execute(input, ToolContext::default()).await.unwrap();

        assert!(result.success);
        assert!(result.output.contains("Successfully wrote"));

        // Verify the content was written
        let written_content = fs::read_to_string(&path).unwrap();
        assert_eq!(written_content, "Hello, World!");
    }

    #[tokio::test]
    async fn test_write_tool_missing_path() {
        let tool = WriteTool;
        let input = serde_json::json!({
            "content": "Hello, World!"
        });
        let result = tool.execute(input, ToolContext::default()).await.unwrap();

        assert!(!result.success);
        assert!(result.error.is_some());
        assert!(result
            .error
            .as_ref()
            .unwrap()
            .contains("Input must contain 'path' field"));
    }

    #[tokio::test]
    async fn test_write_tool_missing_content() {
        let tool = WriteTool;
        let input = serde_json::json!({
            "path": "/tmp/test.txt"
        });
        let result = tool.execute(input, ToolContext::default()).await.unwrap();

        assert!(!result.success);
        assert!(result.error.is_some());
        assert!(result
            .error
            .as_ref()
            .unwrap()
            .contains("Input must contain 'content' field"));
    }
}
