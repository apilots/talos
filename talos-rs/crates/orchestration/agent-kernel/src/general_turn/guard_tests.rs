use super::*;
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use talos_llm_provider::{ChatResponse, ChatStream, Role, Usage};

#[test]
fn forced_finalization_prompt_requires_visible_answer_without_tools() {
    assert!(FORCED_FINALIZATION_INSTRUCTION.contains("final-answer phase"));
    assert!(FORCED_FINALIZATION_INSTRUCTION.contains("assistant response body"));
    assert!(FORCED_FINALIZATION_INSTRUCTION.contains("Do not call tools"));
    assert!(FORCED_FINALIZATION_INSTRUCTION.contains("tool-call syntax"));
    assert!(FORCED_FINALIZATION_INSTRUCTION.contains("Start directly with the answer"));
}

#[test]
fn salvage_finalization_prompt_rejects_planning_only_answers() {
    assert!(SALVAGE_FINALIZATION_INSTRUCTION.contains("user-visible final answer"));
    assert!(SALVAGE_FINALIZATION_INSTRUCTION.contains("assistant response body"));
    assert!(SALVAGE_FINALIZATION_INSTRUCTION.contains("Do not call tools"));
    assert!(SALVAGE_FINALIZATION_INSTRUCTION.contains("planning sentence"));
}

struct FinalizationReasoningProvider {
    calls: AtomicUsize,
}

#[async_trait]
impl LlmProvider for FinalizationReasoningProvider {
    fn name(&self) -> &str {
        "finalization-reasoning-provider"
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.chat_with_tools(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(
            request.reasoning_effort, None,
            "final-answer phase must use the visible response channel"
        );
        assert!(
            request.tools.as_ref().is_none_or(Vec::is_empty),
            "final-answer phase must not include tool definitions"
        );
        assert!(
            matches!(
                request.messages.last().map(|message| message.role),
                Some(Role::User)
            ),
            "final-answer phase instruction must remain the latest user turn"
        );
        Ok(ChatResponse {
            content: "Visible final answer from existing evidence.".to_string(),
            reasoning: ReasoningOutput::default(),
            model: request.model,
            usage: Usage::default(),
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<talos_llm_provider::ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    fn default_model(&self) -> String {
        "finalization-reasoning-provider".to_string()
    }
}

#[tokio::test]
async fn forced_finalization_disables_reasoning_effort_for_visible_answer_phase() {
    let provider = Arc::new(FinalizationReasoningProvider {
        calls: AtomicUsize::new(0),
    });

    let mut model_rounds = 0;
    let result = run_forced_finalization_step(
        vec![ChatMessage::user("Analyze the project.")],
        "mock-model".to_string(),
        provider.clone(),
        None,
        &mut model_rounds,
        ForcedFinalizationCause::RepeatedReadOnlyInvestigation,
        None,
    )
    .await
    .expect("forced finalization should succeed");

    assert_eq!(result.text, "Visible final answer from existing evidence.");
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}
