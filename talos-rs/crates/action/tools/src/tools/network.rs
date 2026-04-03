use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use talos_error::TalosResult;

use crate::{decode_tool_input, Tool, ToolContext, ToolOutput};

/// Tool for performing web searches
pub struct WebSearchTool;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum WebSearchInput {
    Query { query: String },
    Raw(String),
}

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
        let query = match decode_tool_input::<WebSearchInput>(input, self.name()) {
            Ok(input) => match input {
                WebSearchInput::Query { query } | WebSearchInput::Raw(query) => query,
            },
            Err(_) => {
                return Ok(ToolOutput {
                    success: false,
                    output: String::new(),
                    error: Some("Input must contain 'query' field".to_string()),
                });
            }
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

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum WebFetchInput {
    Url { url: String },
    Raw(String),
}

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
        let url = match decode_tool_input::<WebFetchInput>(input, self.name()) {
            Ok(input) => match input {
                WebFetchInput::Url { url } | WebFetchInput::Raw(url) => url,
            },
            Err(_) => {
                return Ok(ToolOutput {
                    success: false,
                    output: String::new(),
                    error: Some("Input must contain 'url' field".to_string()),
                });
            }
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

#[derive(Debug, Deserialize)]
struct HttpRequestInput {
    url: String,
    #[serde(default = "default_http_method")]
    method: String,
    #[serde(default)]
    headers: Option<Value>,
    #[serde(default)]
    body: Option<Value>,
}

fn default_http_method() -> String {
    "GET".to_string()
}

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
        let input: HttpRequestInput = match decode_tool_input(input, self.name()) {
            Ok(input) => input,
            Err(_) => {
                return Ok(ToolOutput {
                    success: false,
                    output: String::new(),
                    error: Some("Input must contain 'url' field".to_string()),
                });
            }
        };
        let header_summary = input
            .headers
            .as_ref()
            .map(Value::to_string)
            .unwrap_or_else(|| "{}".to_string());
        let body_summary = input
            .body
            .as_ref()
            .map(Value::to_string)
            .unwrap_or_else(|| "null".to_string());

        // Basic validation
        if !input.url.starts_with("http://") && !input.url.starts_with("https://") {
            return Ok(ToolOutput {
                success: false,
                output: "".to_string(),
                error: Some("URL must start with http:// or https://".to_string()),
            });
        }

        // For now, return a mock response
        let mock_response = format!(
            "HTTP {} request to '{}'\nRequest headers: {}\nRequest body: {}\nResponse status: 200 OK\nResponse headers: {{\"content-type\": \"application/json\"}}\nResponse body: {{\"message\": \"Success\"}}",
            input.method.to_uppercase(),
            input.url,
            header_summary,
            body_summary
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
