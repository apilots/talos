use super::*;
use serde_json::{Map, Value};

const FORCED_FINALIZATION_INSTRUCTION: &str = "Tool investigation must stop now. Do not call any more tools. Based only on the verified observations and tool results already present in the conversation, provide the best final answer for the user. If the evidence is incomplete, say so briefly and summarize what is known.";

pub(crate) fn is_read_only_tool_name(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "read"
            | "read_file"
            | "list_dir"
            | "grep_files"
            | "memory_search"
            | "memory_get"
            | "web_search"
            | "web_fetch"
            | "http_request"
            | "list_mcp_resources"
            | "list_mcp_resource_templates"
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

    (guard.consecutive_equivalent_read_only_rounds >= 1)
        .then_some(ForcedFinalizationCause::RepeatedReadOnlyInvestigation)
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
    native_thinking: Option<NativeThinkingConfig>,
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
    messages.push(ChatMessage::system(format!(
        "{}\n{}",
        FORCED_FINALIZATION_INSTRUCTION, cause_note
    )));

    execute_general_turn_step(
        GeneralTurnStepRequest {
            round: 0,
            model,
            messages,
            tool_definitions: Vec::new(),
            parallel_tool_calls: false,
            native_thinking,
            stream_tx: None,
            streaming_tool_execution: None,
        },
        llm_provider,
    )
    .await
}

fn fingerprint_read_only_round(step: &GeneralTurnStepResult) -> ReadOnlyRoundFingerprint {
    ReadOnlyRoundFingerprint {
        calls: step
            .tool_calls
            .iter()
            .map(|call| ToolCallFingerprint {
                tool_name: call.name.clone(),
                canonical_arguments: canonical_json_string(&call.arguments),
            })
            .collect(),
    }
}

fn canonical_json_string(value: &Value) -> String {
    canonicalize_value(value).to_string()
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
