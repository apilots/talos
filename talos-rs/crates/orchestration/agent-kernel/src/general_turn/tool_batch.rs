use super::*;
use crate::execution::ReplayDynamicToolResult;
use crate::ToolExecutionOutput;
use talos_tools::ToolResultEnvelope;
use talos_types::{DynamicToolResultParams, UserActionParams};

pub(crate) fn parse_retrieved_memory_items(
    tool_name: &str,
    payload: &Value,
) -> TalosResult<Vec<talos_types::MemoryInfo>> {
    match tool_name {
        "MemorySearch" | "memory_search" => Ok(Vec::new()),
        "MemoryGet" | "memory_get" => {
            match serde_json::from_value::<MemoryGetResponse>(payload.clone()) {
                Ok(response) if !response.memory.body.trim().is_empty() => {
                    Ok(vec![response.memory])
                }
                Ok(_) => Ok(Vec::new()),
                Err(error) => Err(TalosError::SerializationError(format!(
                    "parse MemoryGet result during context rebuild: {error}"
                ))),
            }
        }
        _ => Ok(Vec::new()),
    }
}

pub(super) fn take_new_retrieved_memories(
    existing: &[talos_types::MemoryInfo],
    candidates: &[talos_types::MemoryInfo],
    max_items: usize,
) -> Vec<talos_types::MemoryInfo> {
    if candidates.is_empty() || existing.len() >= max_items || max_items == 0 {
        return Vec::new();
    }

    let mut accepted = Vec::new();
    for candidate in candidates {
        if existing.len() + accepted.len() >= max_items
            || existing
                .iter()
                .chain(accepted.iter())
                .any(|item| item.memory_id == candidate.memory_id)
        {
            continue;
        }
        accepted.push(candidate.clone());
    }
    accepted
}

fn build_tool_result_payload(
    tool_name: &str,
    outcome: &ToolDispatchOutcome,
) -> (Value, Vec<TalosContentItem>, bool, Option<Value>) {
    let envelope = outcome.envelope();
    let pending_arguments = envelope
        .pending
        .as_ref()
        .and_then(|pending| pending.arguments.clone());
    let next_turn_failure = envelope
        .next_turn_failure_payload()
        .and_then(|value| serde_json::to_value(value).ok());
    let semantic_events = envelope
        .semantic_events()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();

    match outcome {
        ToolDispatchOutcome::Pending { pending, .. } => {
            let error = match pending {
                ToolPending::Dynamic { .. } => "dynamic tool response is pending".to_string(),
                ToolPending::Question { .. } => "question response is pending".to_string(),
                ToolPending::SandboxApproval { .. } => "sandbox approval is pending".to_string(),
            };
            (
                serde_json::json!({
                    "ok": false,
                    "tool": tool_name,
                    "error": error,
                    "truncated": false,
                    "envelope": envelope,
                    "semantic_events": semantic_events,
                }),
                Vec::new(),
                true,
                pending_arguments,
            )
        }
        ToolDispatchOutcome::Executed { payload } => match payload {
            _ if payload.validate_canonical_result().is_err() => {
                let error = "canonical tool result requires content_items".to_string();
                (
                    serde_json::json!({
                        "ok": false,
                        "tool": tool_name,
                        "error": error,
                        "truncated": false,
                        "semantic_events": semantic_events,
                    }),
                    vec![TalosContentItem::Text { text: error }],
                    true,
                    pending_arguments,
                )
            }
            ToolOutputPayload::Function {
                content,
                success,
                structured_content,
                ..
            } => {
                let truncated = content.contains("[truncated]");
                if *success {
                    (
                        model_payload_from_envelope(&envelope),
                        envelope.content_items.clone(),
                        false,
                        pending_arguments,
                    )
                } else {
                    let error = envelope
                        .error
                        .as_ref()
                        .map(|item| item.message.clone())
                        .unwrap_or_else(|| "unknown error".to_string());
                    let mut value = serde_json::json!({
                        "ok": false,
                        "tool": tool_name,
                        "error": error,
                        "truncated": truncated,
                        "envelope": envelope,
                        "semantic_events": semantic_events,
                    });
                    if let Some(structured) = structured_content {
                        if let Some(object) = value.as_object_mut() {
                            object.insert("structured".to_string(), structured.clone());
                        }
                    }
                    if let Some(next_turn_failure) = next_turn_failure {
                        if let Some(object) = value.as_object_mut() {
                            object.insert("next_turn_failure".to_string(), next_turn_failure);
                        }
                    }
                    (
                        value,
                        envelope.content_items.clone(),
                        true,
                        pending_arguments,
                    )
                }
            }
            ToolOutputPayload::Resource { .. } => (
                model_payload_from_envelope(&envelope),
                envelope.content_items.clone(),
                false,
                pending_arguments,
            ),
            ToolOutputPayload::Mcp { content, is_error } => {
                if *is_error {
                    let mut value = serde_json::json!({
                        "ok": false,
                        "tool": tool_name,
                        "error": content,
                        "truncated": content.contains("[truncated]"),
                        "envelope": envelope,
                        "semantic_events": semantic_events,
                    });
                    if let Some(next_turn_failure) = next_turn_failure {
                        if let Some(object) = value.as_object_mut() {
                            object.insert("next_turn_failure".to_string(), next_turn_failure);
                        }
                    }
                    (
                        value,
                        envelope.content_items.clone(),
                        true,
                        pending_arguments,
                    )
                } else {
                    (
                        model_payload_from_envelope(&envelope),
                        envelope.content_items.clone(),
                        false,
                        pending_arguments,
                    )
                }
            }
        },
    }
}

struct ReducedToolExecutionOutput {
    payload: Value,
    content_items: Vec<TalosContentItem>,
    is_error: bool,
    envelope: ToolResultEnvelope,
    semantic_events: Vec<String>,
    next_turn_failure: Option<Value>,
}

fn reduce_tool_execution_output(output: &ToolExecutionOutput) -> ReducedToolExecutionOutput {
    let envelope = output.outcome.envelope();
    let (payload, content_items, is_error, _) =
        build_tool_result_payload(&output.tool_name, &output.outcome);
    let semantic_events = envelope
        .semantic_events()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let next_turn_failure = envelope
        .next_turn_failure_payload()
        .and_then(|value| serde_json::to_value(value).ok());
    ReducedToolExecutionOutput {
        payload,
        content_items,
        is_error,
        envelope,
        semantic_events,
        next_turn_failure,
    }
}

pub(crate) async fn emit_tool_result_stream_event(
    stream_tx: Option<&mpsc::Sender<KernelStreamEvent>>,
    output: &ToolExecutionOutput,
) {
    if matches!(output.outcome, ToolDispatchOutcome::Pending { .. }) {
        return;
    }

    let reduced = reduce_tool_execution_output(output);
    emit_stream_event(
        stream_tx,
        KernelStreamEventType::ToolResult,
        KernelStreamEventData::ToolResult(ToolResultStreamData {
            call_id: output.call_id.clone(),
            tool_name: output.tool_name.clone(),
            content: reduced.content_items,
            is_error: reduced.is_error,
            semantic_events: reduced.semantic_events,
            envelope: serde_json::to_value(&reduced.envelope).ok(),
            next_turn_failure: reduced.next_turn_failure,
        }),
    )
    .await;
}

#[cfg(test)]
pub(crate) fn collect_general_turn_tool_batch(
    outputs: Vec<ToolExecutionOutput>,
) -> TalosResult<GeneralTurnToolBatch> {
    collect_general_turn_tool_batch_for_calls(outputs, &[])
}

pub(crate) fn collect_general_turn_tool_batch_for_calls(
    outputs: Vec<ToolExecutionOutput>,
    calls: &[FunctionCall],
) -> TalosResult<GeneralTurnToolBatch> {
    let completed_call_ids = outputs
        .iter()
        .map(|output| output.call_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let deferred_tool_calls = calls
        .iter()
        .filter(|call| !completed_call_ids.contains(call.id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let mut output_items = Vec::new();
    let mut tool_messages = Vec::new();
    let mut pending_user_action = None;
    let mut logs = Vec::new();
    let mut error_count = 0_usize;

    for output in outputs {
        let reduced = reduce_tool_execution_output(&output);
        if let ToolDispatchOutcome::Pending { pending, .. } = &output.outcome {
            let params = match pending {
                ToolPending::Dynamic { .. } => {
                    UserActionParams::DynamicToolResult(DynamicToolResultParams {
                        tool_name: output.tool_name.clone(),
                    })
                }
                ToolPending::Question { params } => UserActionParams::Question(params.clone()),
                ToolPending::SandboxApproval { params, .. } => {
                    UserActionParams::SandboxApproval(params.clone())
                }
            };
            let sandbox_request_fingerprint = match pending {
                ToolPending::SandboxApproval {
                    request_fingerprint,
                    ..
                } => Some(request_fingerprint.clone()),
                ToolPending::Dynamic { .. } | ToolPending::Question { .. } => None,
            };
            let action = PendingUserActionRequest {
                action_id: format!("ua_{}", uuid::Uuid::now_v7().simple()),
                tool_name: output.tool_name.clone(),
                call_id: output.call_id.clone(),
                arguments: output.arguments.clone(),
                params,
                sandbox_request_fingerprint,
                reasoning_continuation: None,
                tool_calls: calls.to_vec(),
                tool_messages: Vec::new(),
                deferred_tool_calls: deferred_tool_calls.clone(),
            };
            if pending_user_action.replace(action).is_some() {
                return Err(TalosError::InternalError(
                    "tool batch produced multiple pending user actions".to_string(),
                ));
            }
            continue;
        }

        if reduced.is_error {
            error_count += 1;
        }
        output_items.push(RunItem::ToolResult {
            call_id: output.call_id.clone(),
            tool_name: output.tool_name.clone(),
            content: reduced.content_items.clone(),
            is_error: reduced.is_error,
        });
        logs.push(ToolResultLog {
            call_id: output.call_id.clone(),
            tool: output.tool_name.clone(),
            payload: reduced.payload.clone(),
            is_error: reduced.is_error,
        });
        tool_messages.push(ChatMessage::tool(
            output.call_id,
            output.tool_name.clone(),
            build_model_tool_result_text(&reduced),
        ));
    }

    if let Some(pending) = &mut pending_user_action {
        pending.tool_messages = tool_messages.clone();
    }

    Ok(GeneralTurnToolBatch {
        output_items,
        tool_messages,
        pending_user_action,
        logs,
        error_count,
    })
}

fn model_payload_from_envelope(envelope: &ToolResultEnvelope) -> Value {
    if envelope.content_items.len() == 1 {
        let mut value =
            serde_json::to_value(&envelope.content_items[0]).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "summary".to_string(),
                envelope
                    .compact_summary
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
        }
        return value;
    }
    serde_json::json!({
        "summary": envelope.compact_summary.clone(),
        "content_items": envelope.content_items.clone(),
    })
}

fn build_model_tool_result_text(reduced: &ReducedToolExecutionOutput) -> String {
    if reduced.is_error {
        return reduced.payload.to_string();
    }
    model_payload_from_envelope(&reduced.envelope).to_string()
}

#[allow(clippy::too_many_arguments)]
pub async fn execute_general_turn_tool_batch(
    prepared: &PreparedGeneralTurn,
    tool_executor: Arc<dyn ToolExecutor>,
    tool_registry: Arc<dyn ToolExecutorPort>,
    kernel_config: Arc<AgentKernelConfig>,
    user_id: &str,
    tool_calls: &[FunctionCall],
    replay_dynamic_tool_results: std::collections::HashMap<String, ReplayDynamicToolResult>,
    user_action_responses: std::collections::HashMap<String, talos_types::UserActionResponse>,
    sandbox_approval_fingerprints: std::collections::HashMap<String, String>,
    command_risk_assessor: Arc<dyn talos_tools::CommandRiskAssessor>,
    stream_tx: Option<&mpsc::Sender<KernelStreamEvent>>,
) -> TalosResult<GeneralTurnToolBatch> {
    let execution_context = ToolExecutionContext {
        session_id: prepared.session_id.clone(),
        turn_id: prepared.invocation_id.clone(),
        agent_id: prepared.agent_id.clone(),
        user_id: user_id.to_string(),
        project_root: prepared.project_root.clone(),
        kernel_config,
        dynamic_tool_results: replay_dynamic_tool_results,
        user_action_responses,
        sandbox_approval_fingerprints,
        command_risk_assessor: Some(command_risk_assessor),
    };
    let inputs = tool_calls
        .iter()
        .map(|call| ToolExecutionInput {
            call_id: call.id.clone(),
            tool_name: call.name.clone(),
            arguments: call.arguments.clone(),
        })
        .collect::<Vec<_>>();
    let outputs = tool_executor
        .execute(&execution_context, tool_registry, &inputs)
        .await?;

    for output in &outputs {
        emit_tool_result_stream_event(stream_tx, output).await;
    }

    collect_general_turn_tool_batch_for_calls(outputs, tool_calls)
}

pub fn apply_general_turn_tool_batch(
    state: &mut GeneralTurnLoopState,
    batch: &GeneralTurnToolBatch,
    _kernel_config: &AgentKernelConfig,
) {
    state.messages.extend(batch.tool_messages.clone());
    state.output_items.extend(batch.output_items.clone());
    state.tool_result_log.extend(batch.logs.clone());
    state.tool_result_count += batch.logs.len();
    state.tool_error_count += batch.error_count;
    state.pending_user_action = batch.pending_user_action.clone();
    state.round += 1;
}

#[cfg(test)]
#[path = "tool_batch_tests.rs"]
mod tests;
