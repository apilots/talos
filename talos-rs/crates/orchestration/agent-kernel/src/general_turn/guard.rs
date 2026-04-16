use super::*;
use serde_json::{Map, Value};

const FORCED_FINALIZATION_INSTRUCTION: &str = "Tool investigation must stop now. Do not call any more tools. Based only on the verified observations and tool results already present in the conversation, provide the best final answer for the user. If the evidence is incomplete, say so briefly and summarize what is known.";
const SALVAGE_FINALIZATION_INSTRUCTION: &str = "Your previous attempt did not produce a usable final answer. Do not call any tools. Using only the verified observations, tool results, and explicit failures already in the conversation, produce a concise final answer. If the tools failed or evidence is incomplete, explain that clearly and give the best next step.";
const FALLBACK_SALVAGE_RESPONSE: &str = "I could not produce a full final answer after tool execution. Based on the available tool results, at least one required step failed or returned insufficient evidence. Review the tool errors and continue with a narrower follow-up step.";
const LOW_GAIN_READ_ONLY_NUDGE: &str = "You are repeating a similar read-only investigation. Unless the next step materially narrows scope, stop searching and either read the most relevant file or answer with the verified evidence already collected.";

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

    let step = execute_general_turn_step(
        GeneralTurnStepRequest {
            round: 0,
            model: model.clone(),
            messages: messages.clone(),
            tool_definitions: Vec::new(),
            parallel_tool_calls: false,
            native_thinking: native_thinking.clone(),
            stream_tx: None,
            streaming_tool_execution: None,
        },
        llm_provider.clone(),
    )
    .await?;

    if !step.text.trim().is_empty() {
        return Ok(step);
    }

    run_salvage_finalization_step(messages, step, model, llm_provider, native_thinking).await
}

async fn run_salvage_finalization_step(
    mut messages: Vec<ChatMessage>,
    prior_step: GeneralTurnStepResult,
    model: String,
    llm_provider: Arc<dyn LlmProvider>,
    native_thinking: Option<NativeThinkingConfig>,
) -> TalosResult<GeneralTurnStepResult> {
    messages.push(ChatMessage::system(SALVAGE_FINALIZATION_INSTRUCTION));
    if !prior_step.text.trim().is_empty() {
        messages.push(ChatMessage::assistant(prior_step.text.clone()));
    }

    let mut salvage_step = execute_general_turn_step(
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
        "grep_files" => {
            map.remove("limit");
        }
        "list_dir" => {
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
