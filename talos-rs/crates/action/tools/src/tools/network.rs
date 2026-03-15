use async_trait::async_trait;
use serde_json::Value;
use talos_error::TalosResult;

use crate::{Tool, ToolContext, ToolOutput};

/// Tool for performing web searches
pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Performs a web search. Input should contain 'query' field."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": {"type": "string"}
            }
        }))
    }

    async fn execute(&self, input: Value, _context: ToolContext) -> TalosResult<ToolOutput> {
        // Extract query from input
        let query = if let Some(query_val) = input.get("query") {
            query_val.as_str().unwrap_or("")
        } else if input.is_string() {
            input.as_str().unwrap()
        } else {
            return Ok(ToolOutput {
                success: false,
                output: "".to_string(),
                error: Some("Input must contain 'query' field or be a string".to_string()),
            });
        };

        // For now, return a mock response since we don't have a real search API integrated
        // In a real implementation, we would call a search API like SerpAPI, Tavily, etc.
        let mock_results = format!(
            "Search results for '{}':\n- Result 1: Sample search result\n- Result 2: Another result\n- Result 3: Third result",
            query
        );

        Ok(ToolOutput {
            success: true,
            output: mock_results,
            error: None,
        })
    }
}

/// Tool for fetching web content
pub struct WebFetchTool;

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetches content from a web URL. Input should contain 'url' field."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["url"],
            "properties": {
                "url": {"type": "string"}
            }
        }))
    }

    async fn execute(&self, input: Value, _context: ToolContext) -> TalosResult<ToolOutput> {
        // Extract URL from input
        let url = if let Some(url_val) = input.get("url") {
            url_val.as_str().unwrap_or("")
        } else if input.is_string() {
            input.as_str().unwrap()
        } else {
            return Ok(ToolOutput {
                success: false,
                output: "".to_string(),
                error: Some("Input must contain 'url' field or be a string".to_string()),
            });
        };

        // Basic URL validation
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Ok(ToolOutput {
                success: false,
                output: "".to_string(),
                error: Some("URL must start with http:// or https://".to_string()),
            });
        }

        // For now, return a mock response
        // In a real implementation, we would fetch the actual content
        let mock_content = format!(
            "Content from URL '{}':\n<HTML>\n  <head><title>Sample Page</title></head>\n  <body>\n    <h1>Sample Web Page</h1>\n    <p>This is sample content from the web page.</p>\n  </body>\n</HTML>",
            url
        );

        Ok(ToolOutput {
            success: true,
            output: mock_content,
            error: None,
        })
    }
}

/// Tool for making HTTP requests
pub struct HttpRequestTool;

#[async_trait]
impl Tool for HttpRequestTool {
    fn name(&self) -> &str {
        "http_request"
    }

    fn description(&self) -> &str {
        "Makes an HTTP request to a URL. Input should contain 'url', 'method', and optional 'headers' and 'body' fields."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["url"],
            "properties": {
                "url": {"type": "string"},
                "method": {"type": "string"},
                "headers": {"type": "object"},
                "body": {}
            }
        }))
    }

    async fn execute(&self, input: Value, _context: ToolContext) -> TalosResult<ToolOutput> {
        let url = match input.get("url") {
            Some(url_val) => url_val.as_str().unwrap_or(""),
            None => {
                return Ok(ToolOutput {
                    success: false,
                    output: "".to_string(),
                    error: Some("Input must contain 'url' field".to_string()),
                });
            }
        };

        let method = match input.get("method") {
            Some(method_val) => method_val.as_str().unwrap_or("GET"),
            None => "GET", // Default to GET
        };

        // Basic validation
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Ok(ToolOutput {
                success: false,
                output: "".to_string(),
                error: Some("URL must start with http:// or https://".to_string()),
            });
        }

        // For now, return a mock response
        let mock_response = format!(
            "HTTP {} request to '{}'\nResponse status: 200 OK\nResponse headers: {{\"content-type\": \"application/json\"}}\nResponse body: {{\"message\": \"Success\"}}",
            method.to_uppercase(),
            url
        );

        Ok(ToolOutput {
            success: true,
            output: mock_response,
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_web_search_tool_success() {
        let tool = WebSearchTool;
        let input = json!({"query": "test search"});
        let result = tool.execute(input, ToolContext::default()).await.unwrap();

        assert!(result.success);
        assert!(result.output.contains("test search"));
    }

    #[tokio::test]
    async fn test_web_search_tool_failure() {
        let tool = WebSearchTool;
        let input = json!({"invalid_field": "test"});
        let result = tool.execute(input, ToolContext::default()).await.unwrap();

        assert!(!result.success);
        assert!(result.error.is_some());
        assert!(result
            .error
            .as_ref()
            .unwrap()
            .contains("Input must contain 'query' field"));
    }

    #[tokio::test]
    async fn test_web_fetch_tool_success() {
        let tool = WebFetchTool;
        let input = json!({"url": "https://example.com"});
        let result = tool.execute(input, ToolContext::default()).await.unwrap();

        assert!(result.success);
        assert!(result.output.contains("https://example.com"));
    }

    #[tokio::test]
    async fn test_web_fetch_tool_invalid_url() {
        let tool = WebFetchTool;
        let input = json!({"url": "invalid-url"});
        let result = tool.execute(input, ToolContext::default()).await.unwrap();

        assert!(!result.success);
        assert!(result.error.is_some());
        assert!(result
            .error
            .as_ref()
            .unwrap()
            .contains("URL must start with http:// or https://"));
    }

    #[tokio::test]
    async fn test_http_request_tool_success() {
        let tool = HttpRequestTool;
        let input = json!({
            "url": "https://api.example.com/data",
            "method": "GET"
        });
        let result = tool.execute(input, ToolContext::default()).await.unwrap();

        assert!(result.success);
        assert!(result.output.contains("GET"));
        assert!(result.output.contains("https://api.example.com/data"));
    }
}
