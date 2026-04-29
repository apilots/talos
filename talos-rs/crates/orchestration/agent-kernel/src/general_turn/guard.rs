use super::*;
use serde_json::{Map, Value};

const FORCED_FINALIZATION_INSTRUCTION: &str = r#"Tool investigation must stop now.

You are in the final-answer phase. No more information can be requested. Produce the best user-visible final answer using only verified observations, tool results, and explicit failures already present in the conversation.

Response contract:
- Do not call tools, request tools, describe a planned tool call, or emit any tool-call syntax.
- Do not output JSON/function-call/XML/tool tags/placeholders.
- Put the complete answer in the assistant response body, not only in private reasoning or thinking.
- Start directly with the answer; do not say you will analyze, inspect, continue, or call tools.
- If evidence is incomplete, state the limitation briefly and still summarize what is known."#;
const SALVAGE_FINALIZATION_INSTRUCTION: &str = r#"The previous finalization attempt did not produce a usable user-visible final answer.

Produce a concise final answer now using only verified observations, tool results, and explicit failures already in the conversation.

Response contract:
- Do not call tools or emit tool-call syntax of any form.
- Do not place the answer only in private reasoning/thinking; write it in the assistant response body.
- Do not answer with a planning sentence such as "I will analyze..." or "I will continue...".
- If tool evidence is incomplete, say what is incomplete and give the best next step."#;
const FALLBACK_SALVAGE_RESPONSE: &str = "I could not produce a full final answer after tool execution. Based on the available tool results, at least one required step failed or returned insufficient evidence. Review the tool errors and continue with a narrower follow-up step.";
const LOW_GAIN_READ_ONLY_NUDGE: &str = "You are repeating a similar read-only investigation. Prefer answering now with verified evidence already collected. If one more read is essential, read only the single most relevant file; otherwise produce the final answer in the assistant response body.";

pub(crate) fn is_read_only_tool_name(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "read"
            | "Read"
            | "Glob"
            | "Grep"
            | "MemorySearch"
            | "MemoryGet"
            | "WebSearch"
            | "WebFetch"
            | "list_mcp_resources"
            | "list_mcp_resource_templates"
            | "web_search"
            | "web_fetch"
    )
}

pub(crate) fn is_read_only_tool_round(step: &GeneralTurnStepResult) -> bool {
    !step.tool_calls.is_empty()
        && step
            .tool_calls
            .iter()
            .all(|call| is_read_only_tool_name(&call.name))
}

pub(crate) fn should_force_finalization_for_step(
    guard: &mut ToolLoopGuardState,
    step: &GeneralTurnStepResult,
) -> Option<ForcedFinalizationCause> {
    if !is_read_only_tool_round(step) {
        guard.last_read_only_round = None;
        guard.consecutive_equivalent_read_only_rounds = 0;
        return None;
    }

    let fingerprint = fingerprint_read_only_round(step);
    if guard.last_read_only_round.as_ref() == Some(&fingerprint) {
        guard.consecutive_equivalent_read_only_rounds = guard
            .consecutive_equivalent_read_only_rounds
            .saturating_add(1);
    } else {
        guard.last_read_only_round = Some(fingerprint);
        guard.consecutive_equivalent_read_only_rounds = 0;
    }

    (guard.consecutive_equivalent_read_only_rounds >= 2)
        .then_some(ForcedFinalizationCause::RepeatedReadOnlyInvestigation)
}

pub(crate) fn should_inject_read_only_nudge(guard: &ToolLoopGuardState) -> bool {
    guard.consecutive_equivalent_read_only_rounds == 1
}

pub(crate) fn build_read_only_nudge_message() -> ChatMessage {
    ChatMessage::system(LOW_GAIN_READ_ONLY_NUDGE)
}

pub(crate) fn stop_reason_for_forced_finalization(_cause: ForcedFinalizationCause) -> StopReason {
    match _cause {
        ForcedFinalizationCause::EmptyFinalResponseAfterTools => StopReason::EndTurn,
        ForcedFinalizationCause::ToolRoundBudget
        | ForcedFinalizationCause::RepeatedReadOnlyInvestigation => StopReason::ToolRoundLimit,
    }
}

pub(crate) fn should_force_finalization_for_empty_response(
    prior_tool_call_count: usize,
    step: &GeneralTurnStepResult,
) -> bool {
    prior_tool_call_count > 0
        && matches!(step.disposition, GeneralTurnStepDisposition::FinalResponse)
        && step.text.trim().is_empty()
}

pub(crate) async fn run_forced_finalization_step(
    mut messages: Vec<ChatMessage>,
    model: String,
    llm_provider: Arc<dyn LlmProvider>,
    _native_thinking: Option<NativeThinkingConfig>,
    cause: ForcedFinalizationCause,
) -> TalosResult<GeneralTurnStepResult> {
    let cause_note = match cause {
        ForcedFinalizationCause::ToolRoundBudget => {
            "Reason: the tool round budget has been exhausted."
        }
        ForcedFinalizationCause::RepeatedReadOnlyInvestigation => {
            "Reason: the model repeated an equivalent read-only investigation."
        }
        ForcedFinalizationCause::EmptyFinalResponseAfterTools => {
            "Reason: the model returned an empty final response after using tools."
        }
    };
    messages.push(ChatMessage::user(format!(
        "{}\n{}",
        FORCED_FINALIZATION_INSTRUCTION, cause_note
    )));

    let step = execute_general_turn_step(
        GeneralTurnStepRequest {
            round: 0,
            model: model.clone(),
            max_tokens: None,
            messages: messages.clone(),
            tool_definitions: Vec::new(),
            parallel_tool_calls: false,
            native_thinking: None,
            stream_tx: None,
            streaming_tool_execution: None,
        },
        llm_provider.clone(),
    )
    .await?;

    if !step.text.trim().is_empty() {
        return Ok(step);
    }

    run_salvage_finalization_step(messages, step, model, llm_provider).await
}

async fn run_salvage_finalization_step(
    mut messages: Vec<ChatMessage>,
    prior_step: GeneralTurnStepResult,
    model: String,
    llm_provider: Arc<dyn LlmProvider>,
) -> TalosResult<GeneralTurnStepResult> {
    messages.push(ChatMessage::user(SALVAGE_FINALIZATION_INSTRUCTION));
    if !prior_step.text.trim().is_empty() {
        messages.push(ChatMessage::assistant(prior_step.text.clone()));
    }

    let mut salvage_step = execute_general_turn_step(
        GeneralTurnStepRequest {
            round: 0,
            model,
            max_tokens: None,
            messages,
            tool_definitions: Vec::new(),
            parallel_tool_calls: false,
            native_thinking: None,
            stream_tx: None,
            streaming_tool_execution: None,
        },
        llm_provider,
    )
    .await?;

    if salvage_step.text.trim().is_empty() {
        salvage_step.text = FALLBACK_SALVAGE_RESPONSE.to_string();
        salvage_step.disposition = GeneralTurnStepDisposition::FinalResponse;
    }

    Ok(salvage_step)
}

fn fingerprint_read_only_round(step: &GeneralTurnStepResult) -> ReadOnlyRoundFingerprint {
    ReadOnlyRoundFingerprint {
        calls: step
            .tool_calls
            .iter()
            .map(|call| ToolCallFingerprint {
                tool_name: call.name.clone(),
                canonical_arguments: canonical_read_only_arguments(&call.name, &call.arguments),
            })
            .collect(),
    }
}

fn canonical_read_only_arguments(tool_name: &str, value: &Value) -> String {
    canonical_json_string(&canonicalize_read_only_arguments(tool_name, value))
}

fn canonical_json_string(value: &Value) -> String {
    canonicalize_value(value).to_string()
}

fn canonicalize_read_only_arguments(tool_name: &str, value: &Value) -> Value {
    let mut normalized = canonicalize_value(value);
    let Value::Object(map) = &mut normalized else {
        return normalized;
    };

    match tool_name {
        "Grep" | "grep" => {
            map.remove("limit");
        }
        "Glob" | "glob" => {
            map.remove("limit");
            map.remove("offset");
        }
        _ => {}
    }

    Value::Object(map.clone())
}

fn canonicalize_value(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonicalize_value).collect()),
        Value::Object(map) => {
            let mut sorted = std::collections::BTreeMap::new();
            for (key, value) in map {
                sorted.insert(key.clone(), canonicalize_value(value));
            }
            let mut normalized = Map::new();
            for (key, value) in sorted {
                normalized.insert(key, value);
            }
            Value::Object(normalized)
        }
        _ => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use talos_llm_provider::{ChatResponse, ChatStream, NativeThinkingEffort, Role, Usage};

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

    #[test]
    fn read_only_nudge_prefers_answering_with_existing_evidence() {
        assert!(LOW_GAIN_READ_ONLY_NUDGE.contains("verified evidence"));
        assert!(LOW_GAIN_READ_ONLY_NUDGE.contains("single most relevant file"));
        assert!(LOW_GAIN_READ_ONLY_NUDGE.contains("assistant response body"));
    }

    struct FinalizationNativeThinkingProvider {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl LlmProvider for FinalizationNativeThinkingProvider {
        fn name(&self) -> &str {
            "finalization-native-thinking-provider"
        }

        async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
            self.chat_with_tools(request).await
        }

        async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert!(
                request.native_thinking.is_none(),
                "final-answer phase must use the visible response channel"
            );
            assert!(
                request.tools.as_ref().map_or(true, Vec::is_empty),
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
                reasoning: Vec::new(),
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

        fn default_model(&self) -> &str {
            "finalization-native-thinking-provider"
        }
    }

    #[tokio::test]
    async fn forced_finalization_disables_native_thinking_for_visible_answer_phase() {
        let provider = Arc::new(FinalizationNativeThinkingProvider {
            calls: AtomicUsize::new(0),
        });

        let result = run_forced_finalization_step(
            vec![ChatMessage::user("Analyze the project.")],
            "mock-model".to_string(),
            provider.clone(),
            Some(NativeThinkingConfig {
                effort: NativeThinkingEffort::High,
                budget_tokens: Some(1024),
            }),
            ForcedFinalizationCause::ToolRoundBudget,
        )
        .await
        .expect("forced finalization should succeed");

        assert_eq!(result.text, "Visible final answer from existing evidence.");
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    }
}
