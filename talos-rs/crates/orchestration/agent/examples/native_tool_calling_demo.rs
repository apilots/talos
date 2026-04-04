use std::sync::Arc;

use serde_json::json;
use talos_agent::{AgentImpl, AgentOrchestrator, OrchestratorRequest};
use talos_error::TalosResult;
use talos_llm_provider::{
    ChatRequest, ChatResponse, ChatStream, FunctionCall, LlmProvider, ToolStream, Usage,
};
use talos_tools::{ToolRegistry, WebSearchTool};
use talos_tools_policy::ToolPolicy;
use talos_types::talos::{TalosInputItem, TalosProvider, TalosSourceType};
use talos_types::{SessionContext, SessionState, TalosSession};

struct NativeToolMockProvider;

#[async_trait::async_trait]
impl LlmProvider for NativeToolMockProvider {
    fn name(&self) -> &str {
        "native-tool-mock"
    }

    fn default_model(&self) -> &str {
        "mock-model"
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.chat_with_tools(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let has_tool_result = request
            .messages
            .iter()
            .any(|m| m.role == talos_llm_provider::Role::Tool);

        if has_tool_result {
            return Ok(ChatResponse {
                content: "I used the tool result and produced the final answer.".to_string(),
                model: "mock-model".to_string(),
                usage: Usage::default(),
                finish_reason: Some("stop".to_string()),
                tool_calls: None,
            });
        }

        Ok(ChatResponse {
            content: "Let me call web_search first.".to_string(),
            model: "mock-model".to_string(),
            usage: Usage::default(),
            finish_reason: Some("tool_calls".to_string()),
            tool_calls: Some(vec![FunctionCall::new(
                "call_1",
                "web_search",
                json!({"query": "Rust async patterns"}),
            )]),
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (tx, rx) = tokio::sync::mpsc::channel(10);
        drop(tx);
        Ok(ChatStream::new(rx))
    }

    async fn stream_with_tools(&self, _request: ChatRequest) -> TalosResult<ToolStream> {
        let (tx, rx) = tokio::sync::mpsc::channel(10);
        drop(tx);
        Ok(ToolStream::new(rx))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let provider = Arc::new(NativeToolMockProvider);
    let policy = ToolPolicy::allow_list(vec!["web_search"]);
    let registry = Arc::new(ToolRegistry::new_with_policy(policy));
    registry.register(Arc::new(WebSearchTool));

    let agent = AgentImpl::new(
        "demo-native-tool-agent".to_string(),
        "NativeToolAgent".to_string(),
        "Demonstrates native tool calling flow".to_string(),
        provider,
        "mock-model".to_string(),
        registry,
        Default::default(),
    );

    let response = agent
        .run(OrchestratorRequest {
            session: TalosSession {
                context: SessionContext {
                    session_id: "demo-session".to_string(),
                    workspace_id: Some("default".to_string()),
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
            input: vec![TalosInputItem::Text {
                text: "Find latest Rust async best practices".to_string(),
            }],
        })
        .await?;

    println!("Final response: {}", response.output_text);
    Ok(())
}
