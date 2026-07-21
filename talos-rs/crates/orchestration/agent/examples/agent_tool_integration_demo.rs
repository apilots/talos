//! Demo to validate Agent with Tool Integration

mod support;

use std::sync::Arc;
use support::NoopMemoryWriter;
use talos_agent::{
    empty_skill_roots_resolver, AgentHarness, AgentHarnessImpl, AgentHarnessParams, AgentRunConfig,
    AgentRunContext, AgentRunOptions, AgentRunParams, AgentRunSessionState,
};
use talos_error::TalosResult;
use talos_llm_provider::{
    ChatRequest, ChatResponse, ChatStream, LlmProvider, LlmProviderRouter, Usage,
};
use talos_tools::{ToolRegistry, WebSearchTool};
use talos_tools_policy::ToolPolicy;
use talos_types::talos::{TalosInputItem, TalosProvider, TalosSourceType};
use talos_types::{ModelRef, ModelSelection, SessionContext, SessionState, TalosSession};

/// Simple mock LLM provider for demo purposes
struct DemoLlmProvider;

struct StaticLlmProviderRouter {
    provider: Arc<dyn LlmProvider>,
}

impl LlmProviderRouter for StaticLlmProviderRouter {
    fn provider_for(&self, _model_ref: &ModelRef) -> TalosResult<Arc<dyn LlmProvider>> {
        Ok(self.provider.clone())
    }
}

fn agent_run_params_from_session(
    session: &TalosSession,
    turn_id: &str,
    input: Vec<TalosInputItem>,
) -> AgentRunParams {
    let context = AgentRunContext::from_session(session, turn_id.to_string());
    let config = AgentRunConfig::from_session(session);
    let session_state = AgentRunSessionState::from_session(session);
    let options = AgentRunOptions::from_config(&config);
    AgentRunParams::new(context, config, session_state, options, input)
}

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
            reasoning: talos_llm_provider::ReasoningOutput::default(),
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
    let agent = AgentHarnessImpl::new(AgentHarnessParams {
        id: "demo_agent_1".to_string(),
        name: "Demo Agent".to_string(),
        description: "An agent for demonstration purposes".to_string(),
        llm_provider_router: Arc::new(StaticLlmProviderRouter {
            provider: llm_provider,
        }),
        model_selection: ModelSelection::without_reasoning(ModelRef {
            provider: "openai".to_string(),
            id: "demo-model".to_string(),
        }),
        tool_registry,
        kernel_config: Default::default(),
        skill_roots_resolver: empty_skill_roots_resolver(),
        session_artifact_store: Arc::new(talos_store::session::FileSessionArtifactStore::default()),
        session_store: Arc::new(talos_store::session::MockSessionStore::new()),
        memory_writer: Arc::new(NoopMemoryWriter),
    })?;

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

    // Test the agent's message handling
    println!("\n3. Testing agent message handling:");
    let mut config = talos_config::Config::default();
    config.llm.openai.models = vec![talos_config::ProviderModelConfig {
        id: "demo-model".to_string(),
        name: "demo-model".to_string(),
        context_window: 32_768,
        reasoning: false,
        reasoning_effort: Default::default(),
        speed: Default::default(),
        input: Vec::new(),
        cost: Default::default(),
    }];
    let session = TalosSession {
        title: "Untitled Session".to_string(),
        user_id: "demo-user".to_string(),
        conf: talos_types::SessionConf {
            model_selection: Some(ModelSelection::without_reasoning(ModelRef {
                provider: "openai".to_string(),
                id: "demo-model".to_string(),
            })),
        },
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
        config,
        state: SessionState::default(),
    };
    match agent
        .run(agent_run_params_from_session(
            &session,
            "turn-demo-agent-tool",
            vec![TalosInputItem::Text {
                text: "Tell me about Rust programming language".to_string(),
            }],
        ))
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

    println!("\n4. Demonstrating the agent's system prompt generation:");
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
    println!("- System prompt generation with available tools");
    println!("- Tool execution result handling and memory storage");
    println!("- Memory integration for storing conversation history and tool results");
    println!("- Security policy enforcement through the tool registry");

    Ok(())
}
