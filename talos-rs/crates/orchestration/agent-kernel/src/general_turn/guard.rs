use super::*;
use serde_json::{Map, Value};

const FORCED_FINALIZATION_INSTRUCTION: &str = r#"Tool investigation must stop now.

You are in the final-answer phase. No more information can be requested. Produce the best user-visible final answer using only verified observations, tool results, and explicit failures already present in the conversation.

Response contract:
- Do not call tools, request tools, describe a planned tool call, or emit any tool-call syntax.
- Do not output JSON/function-call/XML/tool tags/placeholders.
- Put the complete answer in the assistant response body, not only in private reasoning or thinking.
- Start directly with the answer; do not say you will analyze, inspect, continue, or call tools.
- If the user requested an exact phrase, marker, or acknowledgement token, output that exact text without paraphrase or extra explanation.
- If a previous visible assistant response already satisfies an exact-output request, repeat that response exactly.
- If evidence is incomplete, state the limitation briefly and still summarize what is known."#;
const SALVAGE_FINALIZATION_INSTRUCTION: &str = r#"The previous finalization attempt did not produce a usable user-visible final answer.

Produce a concise final answer now using only verified observations, tool results, and explicit failures already in the conversation.

Response contract:
- Do not call tools or emit tool-call syntax of any form.
- Do not place the answer only in private reasoning/thinking; write it in the assistant response body.
- Do not answer with a planning sentence such as "I will analyze..." or "I will continue...".
- Preserve exact requested phrases, markers, or acknowledgement tokens without paraphrase.
- If tool evidence is incomplete, say what is incomplete and give the best next step."#;
const FALLBACK_SALVAGE_RESPONSE: &str = "I could not produce a full final answer after tool execution. Based on the available tool results, at least one required step failed or returned insufficient evidence. Review the tool errors and continue with a narrower follow-up step.";
const LOW_GAIN_READ_ONLY_NUDGE: &str = "You are repeating a similar read-only investigation. Prefer answering now with verified evidence already collected. If one more read is essential, read only the single most relevant file; otherwise produce the final answer in the assistant response body.";
const TOOL_FAILURE_RECOVERY_NUDGE: &str = "The previous tool batch included failed commands or failed file reads. Before calling another tool, revise the next step using only verified paths and successful outputs already observed. Do not retry the same missing path, command, or arguments unless a prior successful result proves that exact target exists. If the failure blocks the task, explain the limitation in the final answer instead of repeating the failed lookup.";

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
            | "ListMcpResources"
            | "ListMcpResourceTemplates"
            | "ReadMcpResource"
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

pub(crate) fn build_tool_failure_recovery_nudge_message(
    batch: &GeneralTurnToolBatch,
) -> Option<ChatMessage> {
    batch_has_recoverable_tool_failure(batch)
        .then(|| ChatMessage::system(TOOL_FAILURE_RECOVERY_NUDGE))
}

fn batch_has_recoverable_tool_failure(batch: &GeneralTurnToolBatch) -> bool {
    batch.logs.iter().any(|log| {
        log.is_error
            || log
                .payload
                .pointer("/exit_code")
                .and_then(Value::as_i64)
                .is_some_and(|code| code != 0)
            || log
                .payload
                .pointer("/value/exit_code")
                .and_then(Value::as_i64)
                .is_some_and(|code| code != 0)
            || log
                .payload
                .pointer("/ok")
                .and_then(Value::as_bool)
                .is_some_and(|ok| !ok)
    })
}

pub(crate) fn stop_reason_for_forced_finalization(_cause: ForcedFinalizationCause) -> StopReason {
    match _cause {
        ForcedFinalizationCause::EmptyFinalResponseAfterTools
        | ForcedFinalizationCause::RepeatedReadOnlyInvestigation => StopReason::EndTurn,
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
    stream_tx: Option<mpsc::Sender<AgentKernelEvent>>,
    pre_final_execution_summary: Option<String>,
    cause: ForcedFinalizationCause,
) -> TalosResult<GeneralTurnStepResult> {
    let cause_note = match cause {
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
            call_mode: KernelModelCallMode::Finalization,
            model: model.clone(),
            max_tokens: None,
            messages: messages.clone(),
            tool_definitions: Vec::new(),
            parallel_tool_calls: false,
            reasoning_effort: None,
            speed: Default::default(),
            pre_final_execution_summary: pre_final_execution_summary.clone(),
            stream_tx: stream_tx.clone(),
            streaming_tool_execution: None,
        },
        llm_provider.clone(),
    )
    .await?;

    if !step.text.trim().is_empty() {
        return Ok(step);
    }

    run_salvage_finalization_step(
        messages,
        step,
        model,
        llm_provider,
        stream_tx,
        pre_final_execution_summary,
    )
    .await
}

async fn run_salvage_finalization_step(
    mut messages: Vec<ChatMessage>,
    prior_step: GeneralTurnStepResult,
    model: String,
    llm_provider: Arc<dyn LlmProvider>,
    stream_tx: Option<mpsc::Sender<AgentKernelEvent>>,
    pre_final_execution_summary: Option<String>,
) -> TalosResult<GeneralTurnStepResult> {
    messages.push(ChatMessage::user(SALVAGE_FINALIZATION_INSTRUCTION));
    if !prior_step.text.trim().is_empty() {
        messages.push(ChatMessage::assistant(prior_step.text.clone()));
    }

    let mut salvage_step = execute_general_turn_step(
        GeneralTurnStepRequest {
            round: 0,
            call_mode: KernelModelCallMode::Finalization,
            model,
            max_tokens: None,
            messages,
            tool_definitions: Vec::new(),
            parallel_tool_calls: false,
            reasoning_effort: None,
            speed: Default::default(),
            pre_final_execution_summary: if prior_step.execution_summary_streamed {
                None
            } else {
                pre_final_execution_summary
            },
            stream_tx,
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
#[path = "guard_tests.rs"]
mod tests;
