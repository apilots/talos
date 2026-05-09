//! Integration example showing how to use the prompt system with agents

use std::collections::HashMap;
use std::sync::Arc;
use talos_llm_provider::{ChatMessage, ChatRequest, LlmProvider};
use talos_memory::{Memory, MemoryStore, SessionPromptContextConfig, TalosMessageRole, TalosTurn};
use talos_prompt::{PromptManager, PromptTemplate};
use talos_tools::{Tool, ToolRegistry};
use talos_types::talos::{StopReason, TalosOutputItem, TalosUsage};

const DEFAULT_SESSION_ID: &str = "default";

fn main() {}

// Example Agent that uses the prompt system
pub struct PromptAwareAgent {
    pub id: String,
    pub name: String,
    pub llm_provider: Box<dyn LlmProvider>,
    pub memory: MemoryStore,
    pub tool_registry: ToolRegistry,
    pub prompt_manager: PromptManager,
}

impl PromptAwareAgent {
    pub fn new(id: String, name: String, llm_provider: Box<dyn LlmProvider>) -> Self {
        let mut prompt_manager = PromptManager::new();

        // Register common agent prompts
        prompt_manager.register_template(
            "system_context",
            PromptTemplate::new(
                r#"You are {{agent_name}}, an AI assistant.
{{agent_purpose}}

Current conversation context:
{{conversation_context}}

Respond appropriately to the user's message."#
                    .to_string(),
                "System context for agent responses".to_string(),
            ),
        );

        prompt_manager.register_template(
            "tool_decision",
            PromptTemplate::new(
                r#"Consider the user's request: {{user_request}}

Available tools:
{{available_tools}}

Based on the request and available tools, decide if a tool should be used. If yes, respond with the tool to use and arguments."#.to_string(),
                "Prompt for tool usage decision".to_string(),
            ),
        );

        Self {
            id,
            name,
            llm_provider,
            memory: MemoryStore::new(
                std::env::temp_dir().join(format!("talos-prompt-example-{}", uuid::Uuid::new_v4())),
            ),
            tool_registry: ToolRegistry::new(),
            prompt_manager,
        }
    }

    pub async fn run_text_request(
        &self,
        user_message: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        // 1. Retrieve conversation context from memory
        let context_snapshot = self
            .memory
            .build_session_prompt_context(
                DEFAULT_SESSION_ID,
                &SessionPromptContextConfig {
                    recent_messages_limit: 5,
                    recent_tool_results_limit: 0,
                    tool_output_max_chars: 0,
                    recent_tool_exchanges_limit: 0,
                    tool_exchange_max_chars: 0,
                    include_summary: false,
                    summary_recent_count: 0,
                    summary_max_chars: 0,
                },
            )
            .await?;
        let conversation_context = context_snapshot
            .recent_messages
            .iter()
            .map(|message| {
                format!(
                    "{}: {}",
                    match message.role {
                        TalosMessageRole::User => "user",
                        TalosMessageRole::Assistant => "assistant",
                        TalosMessageRole::System => "system",
                    },
                    message.text
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        // 2. Prepare context for system prompt
        let mut context = HashMap::new();
        context.insert("agent_name".to_string(), self.name.clone());
        context.insert(
            "agent_purpose".to_string(),
            "A helpful AI assistant that can use tools to complete tasks".to_string(),
        );
        context.insert("conversation_context".to_string(), conversation_context);

        // 3. Render system prompt
        let system_prompt = self
            .prompt_manager
            .render_template("system_context", &context)?;

        // 4. Prepare messages for LLM
        let messages = vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(user_message),
        ];

        // 5. Call LLM
        let request_with_messages = ChatRequest::builder()
            .messages(messages)
            .model("gpt-4".to_string())
            .build();

        let response = self.llm_provider.chat(request_with_messages).await?;

        // 6. Store in memory
        let turn_id = uuid::Uuid::new_v4().to_string();
        let seq = self.memory.next_turn_seq(DEFAULT_SESSION_ID).await?;
        let timestamp = chrono::Utc::now();
        let talos_turn = TalosTurn {
            turn_id: turn_id.clone(),
            message_id: "msg-1".to_string(),
            session_id: DEFAULT_SESSION_ID.to_string(),
            seq,
            timestamp,
            items: vec![
                TalosOutputItem::UserInput {
                    text: user_message.to_string(),
                },
                TalosOutputItem::OutputText {
                    text: response.content.clone(),
                },
            ],
            usage: TalosUsage::default(),
            stop_reason: StopReason::EndTurn,
        };
        self.memory.append_turn(&talos_turn).await?;

        Ok(response.content)
    }

    pub fn register_tool(&mut self, tool: Arc<dyn Tool>) {
        self.tool_registry.register(tool);
    }

    pub fn get_available_tools(&self) -> String {
        self.tool_registry.list_tool_names().join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use talos_llm_provider::{ChatResponse, LlmProvider, Usage};
    use tokio::sync::mpsc;

    // Mock LLM Provider for testing
    struct MockLlmProvider;

    #[async_trait]
    impl LlmProvider for MockLlmProvider {
        fn name(&self) -> &str {
            "mock"
        }

        async fn chat(
            &self,
            _request: talos_llm_provider::ChatRequest,
        ) -> Result<talos_llm_provider::ChatResponse, talos_error::TalosError> {
            Ok(ChatResponse {
                content: "This is a mock response from the LLM".to_string(),
                model: "mock-model".to_string(),
                usage: Usage::default(),
                finish_reason: Some("stop".to_string()),
                tool_calls: None,
            })
        }

        async fn stream(
            &self,
            _request: talos_llm_provider::ChatRequest,
        ) -> Result<talos_llm_provider::ChatStream, talos_error::TalosError> {
            let (tx, rx) = mpsc::channel(1);
            drop(tx); // Close the channel immediately for mock
            Ok(talos_llm_provider::ChatStream::new(rx))
        }

        fn default_model(&self) -> String {
            "mock-model".to_string()
        }
    }

    #[tokio::test]
    async fn test_prompt_aware_agent() {
        let agent = PromptAwareAgent::new(
            "test-agent".to_string(),
            "Test Assistant".to_string(),
            Box::new(MockLlmProvider),
        );

        let response = agent.run_text_request("Hello, world!").await;
        assert!(response.is_ok());
        println!("Agent response: {}", response.unwrap());
    }

    #[test]
    fn test_prompt_templates() {
        let mut manager = PromptManager::new();

        manager.register_template(
            "test_template",
            PromptTemplate::new(
                "Hello, {{name}}! You have {{count}} items.".to_string(),
                "Test template".to_string(),
            ),
        );

        let mut context = HashMap::new();
        context.insert("name".to_string(), "Alice".to_string());
        context.insert("count".to_string(), "5".to_string());

        let result = manager.render_template("test_template", &context).unwrap();
        assert_eq!(result, "Hello, Alice! You have 5 items.");
    }
}
