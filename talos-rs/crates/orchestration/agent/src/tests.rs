#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio;
    use talos_tools::{ReadTool, WriteTool, ExecTool, WebSearchTool, WebFetchTool, HttpRequestTool, ToolRegistry};
    use talos_tools_policy::ToolPolicy;
    use serde_json::json;

    #[tokio::test]
    async fn test_process_tool_calls() {
        // Create an agent with a mock provider
        let mock_provider = MockLlmProvider {};
        let agent = AgentImpl::new(
            "test_agent".to_string(),
            "Test Agent".to_string(),
            "An agent for testing".to_string(),
            Arc::new(mock_provider),
            "gpt-4o".to_string(),
            Arc::new(ToolRegistry::new()),
            Default::default(),
        );

        // Create a tool call to test
        let tool_call = ToolCall {
            id: "call_123".to_string(),
            name: "web_search".to_string(),
            arguments: json!({"query": "test"}),
        };

        // Process the tool call
        let result = agent.process_tool_calls(vec![tool_call]).await;

        assert!(result.is_ok());
        let result_str = result.unwrap();
        assert!(result_str.contains("Tool 'web_search' executed successfully"));
    }

    #[tokio::test]
    async fn test_generate_system_prompt() {
        // Create an agent with a mock provider
        let mock_provider = MockLlmProvider {};
        let agent = AgentImpl::new(
            "test_agent".to_string(),
            "Test Agent".to_string(),
            "An agent for testing".to_string(),
            Arc::new(mock_provider),
            "gpt-4o".to_string(),
            Arc::new(ToolRegistry::new()),
            Default::default(),
        );

        // Add some tools to the registry
        agent.add_tool(Arc::new(WebSearchTool));
        agent.add_tool(Arc::new(ReadTool));

        // Generate system prompt
        let prompt = agent.generate_system_prompt();

        assert!(prompt.contains("Test Agent"));
        assert!(prompt.contains("An agent for testing"));
        assert!(prompt.contains("web_search"));
        assert!(prompt.contains("read"));
    }

    #[tokio::test]
    async fn test_add_tool() {
        // Create an agent with a mock provider
        let mock_provider = MockLlmProvider {};
        let agent = AgentImpl::new(
            "test_agent".to_string(),
            "Test Agent".to_string(),
            "An agent for testing".to_string(),
            Arc::new(mock_provider),
            "gpt-4o".to_string(),
            Arc::new(ToolRegistry::new()),
            Default::default(),
        );

        // Verify initial state
        assert_eq!(agent.tool_registry.list_tool_names().len(), 0);

        // Add a tool
        agent.add_tool(Arc::new(WebSearchTool));

        // Verify the tool was added
        let tools = agent.tool_registry.list_tool_names();
        assert!(tools.contains(&"web_search".to_string()));
    }

    #[tokio::test]
    async fn test_execute_tool() {
        // Create an agent with a mock provider
        let mock_provider = MockLlmProvider {};
        let agent = AgentImpl::new(
            "test_agent".to_string(),
            "Test Agent".to_string(),
            "An agent for testing".to_string(),
            Arc::new(mock_provider),
            "gpt-4o".to_string(),
            Arc::new(ToolRegistry::new()),
            Default::default(),
        );

        // Add a tool
        agent.add_tool(Arc::new(WebSearchTool));

        // Execute the tool
        let result = agent.execute_tool("web_search", json!({"query": "test"})).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.content.contains("Search results for 'test'"));
    }

    // Mock LLM provider for testing
    struct MockLlmProvider;

    #[async_trait::async_trait]
    impl LlmProvider for MockLlmProvider {
        fn name(&self) -> &str {
            "mock"
        }

        fn default_model(&self) -> &str {
            "mock-model"
        }

        async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
            Ok(ChatResponse {
                content: "Test response".to_string(),
                model: "mock-model".to_string(),
                usage: None,
                finish_reason: None,
            })
        }

        async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
            let (tx, rx) = tokio::sync::mpsc::channel(100);
            drop(tx); // Close the sender for mock
            Ok(ChatStream { rx })
        }
    }
}
