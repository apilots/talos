//! Demo to validate Agent with Tool Integration

use serde_json::json;
use std::sync::Arc;
use talos_agent::{Agent, AgentImpl, AgentOrchestrator, OrchestratorRequest};
use talos_error::TalosResult;
use talos_llm_provider::{ChatRequest, ChatResponse, ChatStream, LlmProvider, Usage};
use talos_tools::{ToolRegistry, WebSearchTool};
use talos_tools_policy::ToolPolicy;
use talos_types::talos::{TalosInputItem, TalosProvider, TalosSourceType};
use talos_types::{SessionContext, SessionState, TalosSession};

/// Simple mock LLM provider for demo purposes
struct DemoLlmProvider;

#[async_trait::async_trait]
impl LlmProvider for DemoLlmProvider {
    fn name(&self) -> &str {
        "demo"
    }

    fn default_model(&self) -> String {
        "demo-model".to_string()
    }

    async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
        Ok(ChatResponse {
            content: r#"{"content": "Hello! I've processed your request using available tools."}"#
                .to_string(),
            model: "demo-model".to_string(),
            usage: Usage {
                prompt_tokens: 10,
                completion_tokens: 20,
                total_tokens: 30,
                ..Usage::default()
            },
            reasoning: Vec::new(),
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        drop(tx); // Close the sender immediately for mock
        Ok(ChatStream { rx })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Validating Agent Tool Integration ===\n");

    // Create a tool registry with security policy
    let policy = ToolPolicy::allow_list(vec!["WebSearch"]);
    let tool_registry = Arc::new(ToolRegistry::new_with_policy(policy));

    // Register a web search tool
    tool_registry.register(Arc::new(WebSearchTool));

    // Create an agent with a mock provider
    let llm_provider = Arc::new(DemoLlmProvider {});
    let agent = AgentImpl::new(
        "demo_agent_1".to_string(),
        "Demo Agent".to_string(),
        "An agent for demonstration purposes".to_string(),
        llm_provider,
        "demo-model".to_string(),
        tool_registry,
        Default::default(),
    );

    println!("1. Agent created successfully");

    println!("2. Tool registry created and injected into agent");
    println!(
        "   Available tools: {:?}",
        agent.tool_registry.list_tool_names()
    );
    println!(
        "   Allowed tools: {:?}",
        agent.tool_registry.list_allowed_tool_names()
    );

    // Test tool execution directly through the agent
    println!("\n3. Testing tool execution via agent.execute_tool():");
    match agent
        .execute_tool("WebSearch", json!({"query": "Rust programming"}))
        .await
    {
        Ok(result) => {
            println!("   ✓ Tool executed successfully");
            println!(
                "   Result: {}",
                result.output_text.chars().take(100).collect::<String>()
            );
        }
        Err(e) => {
            println!("   ✗ Tool execution failed: {}", e);
        }
    }

    // Test the agent's message handling
    println!("\n4. Testing agent message handling:");
    match agent
        .run(OrchestratorRequest {
            session: TalosSession {
                title: "Untitled Session".to_string(),
                user_id: "demo-user".to_string(),
                conf: talos_types::SessionConf::default(),
                project_id: None,
                current_directory: None,
                context: SessionContext {
                    session_id: "demo-session".to_string(),
                    workspace_id: Some("default".to_string()),
                    project_id: None,
                    current_directory: None,
                    project_root: None,
                    source_type: TalosSourceType::System,
                    provider: TalosProvider::System,
                    user_id: "demo-user".to_string(),
                    message_id: "demo-message".to_string(),
                    trace_id: "demo-trace".to_string(),
                    channel_id: None,
                    thread_id: None,
                    environment: None,
                },
                config: talos_config::Config::default(),
                state: SessionState::default(),
            },
            turn_id: None,
            resume_context: None,
            input: vec![TalosInputItem::Text {
                text: "Tell me about Rust programming language".to_string(),
            }],
        })
        .await
    {
        Ok(response) => {
            println!("   ✓ Message handled successfully");
            println!(
                "   Response: {}",
                response.output_text.chars().take(100).collect::<String>()
            );
        }
        Err(e) => {
            println!("   ✗ Message handling failed: {}", e);
        }
    }

    println!("\n5. Demonstrating the agent's system prompt generation:");
    let system_prompt = agent.generate_system_prompt();
    println!(
        "   System prompt length: {} characters",
        system_prompt.len()
    );
    println!(
        "   Contains tool info: {}",
        system_prompt.contains("WebSearch")
    );

    println!("\n=== Agent Tool Integration Validation Complete ===");
    println!("\nThe agent has been enhanced with:");
    println!("- Tool call processing (process_tool_calls method)");
    println!("- System prompt generation with available tools");
    println!("- Tool execution result handling and memory storage");
    println!("- Memory integration for storing conversation history and tool results");
    println!("- Security policy enforcement through the tool registry");

    Ok(())
}
