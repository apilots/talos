//! Integration example showing how to use the prompt system with agents

use std::collections::HashMap;
use std::sync::Arc;
use talos_llm_provider::{ChatMessage, ChatRequest, LlmProvider};
use talos_prompt::{PromptManager, PromptTemplate};
use talos_tools::{Tool, ToolRegistry};

fn main() {}

// Example Agent that uses the prompt system
pub struct PromptAwareAgent {
    pub id: String,
    pub name: String,
    pub llm_provider: Box<dyn LlmProvider>,
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
            tool_registry: ToolRegistry::new(),
            prompt_manager,
        }
    }

    pub async fn run_text_request(
        &self,
        user_message: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        // 1. Prepare context for system prompt
        let mut context = HashMap::new();
        context.insert("agent_name".to_string(), self.name.clone());
        context.insert(
            "agent_purpose".to_string(),
            "A helpful AI assistant that can use tools to complete tasks".to_string(),
        );
        context.insert("conversation_context".to_string(), String::new());

        // 2. Render system prompt
        let system_prompt = self
            .prompt_manager
            .render_template("system_context", &context)?;

        // 3. Prepare messages for LLM
        let messages = vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(user_message),
        ];

        // 4. Call LLM
        let request_with_messages = ChatRequest::builder()
            .messages(messages)
            .model("gpt-4".to_string())
            .build();

        let response = self.llm_provider.chat(request_with_messages).await?;

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
#[path = "tests/integration_tests.rs"]
mod tests;
