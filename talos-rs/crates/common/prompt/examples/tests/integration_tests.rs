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
