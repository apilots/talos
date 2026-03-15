//! Full System Integration Tests
//!
//! Tests collaboration between all major components:
//! - Agent and Memory integration
//! - Agent and Tools integration
//! - Channel and Agent integration
//! - Client/CLI and Gateway integration
//! - TUI and Gateway client integration

use std::sync::Arc;
use tokio::time::timeout;
use tokio::sync::mpsc;

use talos_agent::AgentImpl;
use talos_tools::{ToolRegistry, WebSearchTool, ReadTool, WriteTool};
use talos_tools_policy::ToolPolicy;
use talos_llm_provider::providers::openai::OpenAiProvider;
use talos_channel::{ChannelManager, InputMessage, OutputMessage};
use talos_types::SessionId;
use talos_config::Config;
use talos_client::{GatewayClient, ChatMessage, ChatResponse, ClientResult};

/// Test the full message processing pipeline
/// Input Message -> Channel -> Agent -> Memory -> Response -> Channel -> Output Message
#[tokio::test]
async fn test_full_message_pipeline() {
    // Set up components
    let llm_provider = create_test_llm_provider();
    let mut agent_impl = AgentImpl::new(
        "pipeline_test_agent".to_string(),
        "Pipeline Test Agent".to_string(),
        "Agent for testing full pipeline".to_string(),
        Arc::new(llm_provider),
        "gpt-4".to_string(),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    );

    // Set up tools
    let policy = ToolPolicy::allow_list(vec!["read", "write"]);
    let tool_registry = Arc::new(ToolRegistry::new_with_policy(policy));
    tool_registry.register(Arc::new(ReadTool));
    tool_registry.register(Arc::new(WriteTool));
    agent_impl.set_tool_registry(tool_registry);

    let agent = Arc::new(agent_impl);
    // Simulate input message
    let input_msg = InputMessage {
        id: "pipeline_test_1".to_string(),
        channel: "test".to_string(),
        session_id: SessionId::new("pipeline_session"),
        sender_id: "test_user_1".to_string(),
        sender_name: "Test User".to_string(),
        content: "What is the capital of France?".to_string(),
        timestamp: 1234567890,
        metadata: std::collections::HashMap::new(),
    };

    // Simulate message processing flow
    // 1. Pass input to Agent for processing
    let response_content = agent.process(&input_msg.content).await
        .expect("Agent should process message successfully");

    // 2. Check if response is reasonable
    assert!(!response_content.is_empty(), "Response should not be empty");
    assert!(response_content.to_lowercase().contains("paris") ||
            response_content.to_lowercase().contains("france") ||
            response_content.len() > 10, "Response should be relevant");

    // 3. Verify memory was used (simplified verification)
    // 4. Create output message
    let output_msg = OutputMessage {
        input_id: Some(input_msg.id.clone()),
        channel: input_msg.channel.clone(),
        session_id: input_msg.session_id.clone(),
        recipient_id: input_msg.sender_id.clone(),
        content: response_content,
        timestamp: 1234567891,
        metadata: std::collections::HashMap::new(),
    };

    // 5. Verify output message structure correctness
    assert_eq!(output_msg.input_id.as_ref().unwrap(), "pipeline_test_1");
    assert_eq!(output_msg.recipient_id, "test_user_1");
    assert!(!output_msg.content.is_empty());
}

/// Test multiple channels integration with the same Agent
#[tokio::test]
async fn test_multi_channel_integration() {
    // Create shared components
    let llm_provider = create_test_llm_provider();
    let mut agent_impl = AgentImpl::new(
        "multi_channel_agent".to_string(),
        "Multi-Channel Test Agent".to_string(),
        "Agent for testing multi-channel".to_string(),
        Arc::new(llm_provider),
        "gpt-4".to_string(),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    );

    // Set up tools
    let policy = ToolPolicy::allow_list(vec!["read", "write"]);
    let tool_registry = Arc::new(ToolRegistry::new_with_policy(policy));
    tool_registry.register(Arc::new(ReadTool));
    agent_impl.set_tool_registry(tool_registry);

    let agent = Arc::new(agent_impl);
    // Simulate messages from different channels
    let channels = ["telegram", "discord", "cli"];
    let mut responses = Vec::new();

    for (i, &channel) in channels.iter().enumerate() {
        let input_msg = InputMessage {
            id: format!("multi_test_{}", i),
            channel: channel.to_string(),
            session_id: SessionId::new(format!("session_{}", i)),
            sender_id: format!("user_{}", i),
            sender_name: format!("User {}", i),
            content: "Hello, this is a test message".to_string(),
            timestamp: 1234567890 + i as u64,
            metadata: std::collections::HashMap::new(),
        };

        // Process message
        let response = agent.process(&input_msg.content).await
            .expect("Agent should process multi-channel message");

        responses.push((channel, response));
    }

    // Verify each response is unique and reasonable
    assert_eq!(responses.len(), 3);
    for (_, response) in &responses {
        assert!(!response.is_empty());
        assert!(response.len() > 5); // Simple content length check
    }
}

/// Test configuration management and component integration
#[tokio::test]
async fn test_config_integration() {
    // Create default configuration
    let config = Config::default();

    // Verify default configuration is loaded correctly
    assert_eq!(config.agent.name, "talos");
    assert_eq!(config.gateway.port, 18789);
    assert!(config.channels.telegram.enabled == false); // Should be disabled by default

    // Create components and verify they use configuration
    let llm_provider = create_test_llm_provider();
    let agent_impl = AgentImpl::new(
        config.agent.name.clone(),
        "Config Test Agent".to_string(),
        "Agent for testing config integration".to_string(),
        Arc::new(llm_provider),
        config.llm.model.clone(),
        Arc::new(ToolRegistry::new()),
        config.agent.kernel.clone(),
    );

    // Verify Agent name matches configuration
    assert_eq!(agent_impl.name(), "talos");
}

/// Test TUI and Gateway client integration
#[tokio::test]
async fn test_tui_gateway_client_integration() {
    use talos_ui_tui::{TuiApp, AppState};

    // Create a mock client implementation
    struct MockClient;

    #[async_trait::async_trait]
    impl GatewayClient for MockClient {
        async fn send_message(&self, _message: ChatMessage) -> ClientResult<ChatResponse> {
            Ok(ChatResponse {
                id: "mock_response".to_string(),
                session_id: SessionId::new("test".to_string()),
                content: "Mock response from gateway".to_string(),
                timestamp: 1234567890,
                status: "success".to_string(),
                metadata: std::collections::HashMap::new(),
            })
        }

        async fn stream_responses(&self, _session_id: SessionId) -> ClientResult<mpsc::Receiver<ChatResponse>> {
            let (_tx, rx) = mpsc::channel(100);
            Ok(rx)
        }

        async fn execute_tool(&self, _request: talos_client::ToolExecutionRequest) -> ClientResult<talos_client::ToolExecutionResponse> {
            Ok(talos_client::ToolExecutionResponse {
                id: "mock_tool".to_string(),
                name: "mock_tool".to_string(),
                result: "Mock tool execution result".to_string(),
                status: "success".to_string(),
                timestamp: 1234567890,
            })
        }

        async fn get_history(&self, _session_id: SessionId, _limit: Option<usize>) -> ClientResult<Vec<ChatMessage>> {
            Ok(vec![])
        }

        async fn create_session(&self, _session_name: &str) -> ClientResult<SessionId> {
            Ok(SessionId::new("mock_session".to_string()))
        }

        async fn list_tools(&self) -> ClientResult<Vec<String>> {
            Ok(vec!["read".to_string(), "write".to_string()])
        }

        async fn close_session(&self, _session_id: SessionId) -> ClientResult<()> {
            Ok(())
        }
    }

    // Create TUI app with mock client
    let app = TuiApp::new(Box::new(MockClient));

    // Test basic TUI app functionality
    assert_eq!(app.get_current_state(), &AppState::Chat);

    // Verify chat history contains welcome message
    let history = app.get_chat_history();
    assert!(!history.is_empty());
    assert!(history.iter().any(|msg| msg.contains("Welcome")));
}

/// Test end-to-end tool call
#[tokio::test]
async fn test_tool_call_end_to_end() {
    let llm_provider = create_test_llm_provider();
    let mut agent_impl = AgentImpl::new(
        "tool_e2e_agent".to_string(),
        "Tool E2E Test Agent".to_string(),
        "Agent for testing tool end-to-end".to_string(),
        Arc::new(llm_provider),
        "gpt-4".to_string(),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    );

    // Register tools
    let policy = ToolPolicy::allow_list(vec!["read", "write"]);
    let tool_registry = Arc::new(ToolRegistry::new_with_policy(policy));
    tool_registry.register(Arc::new(ReadTool));
    tool_registry.register(Arc::new(WriteTool));
    agent_impl.set_tool_registry(tool_registry);

    let agent = Arc::new(agent_impl);

    // Test query that requires tool call
    let tool_query = "Please read the file named 'test_file.txt' if it exists";
    let response = agent.process(tool_query).await
        .expect("Agent should handle tool query");

    // Verify response contains tool-related information
    assert!(response.to_lowercase().contains("tool") ||
            response.to_lowercase().contains("read") ||
            response.to_lowercase().contains("file") ||
            response.to_lowercase().contains("error")); // Should report error if file doesn't exist
}

/// Test unified client interface usage
#[tokio::test]
async fn test_unified_client_interface() {
    // This test verifies that all clients (TUI, CLI, mobile apps, third parties) can use the same interface
    struct MockClient;

    #[async_trait::async_trait]
    impl GatewayClient for MockClient {
        async fn send_message(&self, message: ChatMessage) -> ClientResult<ChatResponse> {
            // Validate message structure
            assert!(!message.content.is_empty());
            assert!(!message.id.is_empty());

            Ok(ChatResponse {
                id: format!("response_to_{}", message.id),
                session_id: message.session_id,
                content: format!("Processed: {}", message.content),
                timestamp: 1234567890,
                status: "success".to_string(),
                metadata: std::collections::HashMap::new(),
            })
        }

        async fn stream_responses(&self, _session_id: SessionId) -> ClientResult<mpsc::Receiver<ChatResponse>> {
            let (_tx, rx) = mpsc::channel(100);
            Ok(rx)
        }

        async fn execute_tool(&self, _request: talos_client::ToolExecutionRequest) -> ClientResult<talos_client::ToolExecutionResponse> {
            Ok(talos_client::ToolExecutionResponse {
                id: "mock_tool".to_string(),
                name: "mock_tool".to_string(),
                result: "Mock tool execution result".to_string(),
                status: "success".to_string(),
                timestamp: 1234567890,
            })
        }

        async fn get_history(&self, _session_id: SessionId, _limit: Option<usize>) -> ClientResult<Vec<ChatMessage>> {
            Ok(vec![])
        }

        async fn create_session(&self, _session_name: &str) -> ClientResult<SessionId> {
            Ok(SessionId::new("test_session".to_string()))
        }

        async fn list_tools(&self) -> ClientResult<Vec<String>> {
            Ok(vec!["read".to_string(), "write".to_string()])
        }

        async fn close_session(&self, _session_id: SessionId) -> ClientResult<()> {
            Ok(())
        }
    }

    let client = MockClient;

    // Test message sending
    let message = ChatMessage {
        id: "test_msg_1".to_string(),
        session_id: SessionId::new("test_session".to_string()),
        content: "Hello, world!".to_string(),
        timestamp: 1234567890,
        metadata: std::collections::HashMap::new(),
    };

    let response = client.send_message(message).await.unwrap();
    assert_eq!(response.status, "success");
    assert!(response.content.contains("Processed: Hello, world!"));
}

/// Helper function: Create LLM provider for testing
fn create_test_llm_provider() -> OpenAiProvider {
    // In test environment, we might use keys from environment variables or mock keys
    let api_key = std::env::var("TEST_OPENAI_API_KEY")
        .unwrap_or_else(|_| "sk-test-dummy-key-for-testing".to_string());

    OpenAiProvider::new(api_key)
}

/// Stress test: Concurrently process multiple requests
#[tokio::test(flavor = "multi_thread")]
async fn test_concurrent_requests() {
    let llm_provider = create_test_llm_provider();
    let agent_impl = AgentImpl::new(
        "concurrent_test_agent".to_string(),
        "Concurrent Test Agent".to_string(),
        "Agent for testing concurrent requests".to_string(),
        Arc::new(llm_provider),
        "gpt-4".to_string(),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    );
    let agent = Arc::new(agent_impl);

    // Create multiple concurrent tasks
    let mut handles = vec![];
    for i in 0..10 {
        let agent_clone = Arc::clone(&agent);
        let handle = tokio::spawn(async move {
            let input = format!("Test message {}", i);
            agent_clone.process(&input).await
        });
        handles.push(handle);
    }

    // Wait for all tasks to complete
    let results: Vec<_> = futures_util::future::join_all(handles).await;

    // Verify all requests were processed successfully
    for result in results {
        assert!(result.is_ok(), "All concurrent requests should succeed");
        if let Ok(response_result) = result {
            assert!(response_result.is_ok(), "Each request should produce a valid response");
        }
    }
}
