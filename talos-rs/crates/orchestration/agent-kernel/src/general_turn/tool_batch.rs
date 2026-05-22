use super::*;
use crate::execution::ReplayDynamicToolResult;
use crate::ToolExecutionOutput;
use talos_tools::ToolResultEnvelope;

pub(crate) fn parse_retrieved_memory_items(
    tool_name: &str,
    payload: &Value,
) -> Vec<RetrievedMemoryItem> {
    match tool_name {
        "MemorySearch" | "memory_search" => {
            match serde_json::from_value::<MemorySearchResponse>(payload.clone()) {
                Ok(response) => response.into_retrieved_items(),
                Err(err) => {
                    warn!(
                        tool_name = %tool_name,
                        error = %err,
                        payload = %payload,
                        "failed to parse MemorySearch payload"
                    );
                    Vec::new()
                }
            }
        }
        "MemoryGet" | "memory_get" => {
            match serde_json::from_value::<MemoryGetResult>(payload.clone()) {
                Ok(result) if !result.text.trim().is_empty() => vec![RetrievedMemoryItem {
                    path: result.path,
                    snippet: result.text.trim().to_string(),
                }],
                Ok(_) => Vec::new(),
                Err(err) => {
                    warn!(
                        tool_name = %tool_name,
                        error = %err,
                        payload = %payload,
                        "failed to parse MemoryGet payload"
                    );
                    Vec::new()
                }
            }
        }
        _ => Vec::new(),
    }
}

fn build_retrieved_memories_message(
    items: &[RetrievedMemoryItem],
    max_items: usize,
    max_chars: usize,
) -> Option<ChatMessage> {
    if items.is_empty() || max_items == 0 || max_chars == 0 {
        return None;
    }
    let mut content = String::new();
    for item in items.iter().take(max_items) {
        let line = format!("- {}: {}", item.path, item.snippet.trim());
        if content.len() + line.len() + 1 > max_chars {
            break;
        }
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str(&line);
    }
    if content.is_empty() {
        None
    } else {
        Some(ChatMessage::system(format!(
            "Retrieved memories:\n{}",
            content
        )))
    }
}

fn build_tool_result_payload(
    tool_name: &str,
    outcome: &ToolDispatchOutcome,
) -> (Value, bool, Option<Value>) {
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
                true,
                pending_arguments,
            )
        }
        ToolDispatchOutcome::Executed { payload } => match payload {
            ToolOutputPayload::Function {
                content,
                success,
                structured_content,
            } => {
                let truncated = content.contains("[truncated]");
                if *success {
                    (
                        envelope
                            .structured_content
                            .clone()
                            .unwrap_or(serde_json::Value::Null),
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
                    (value, true, pending_arguments)
                }
            }
            ToolOutputPayload::Resource { .. } => (
                envelope
                    .structured_content
                    .clone()
                    .unwrap_or(serde_json::Value::Null),
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
                    (value, true, pending_arguments)
                } else {
                    (
                        envelope
                            .structured_content
                            .clone()
                            .unwrap_or(serde_json::Value::Null),
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
    is_error: bool,
    pending_arguments: Option<Value>,
    memory_payload: Value,
    envelope: ToolResultEnvelope,
    semantic_events: Vec<String>,
    next_turn_failure: Option<Value>,
}

fn reduce_tool_execution_output(output: &ToolExecutionOutput) -> ReducedToolExecutionOutput {
    let envelope = output.outcome.envelope();
    let (payload, is_error, pending_arguments) =
        build_tool_result_payload(&output.tool_name, &output.outcome);
    let memory_payload = output
        .outcome
        .payload()
        .map(|payload| payload.envelope())
        .and_then(|envelope| envelope.structured_content)
        .unwrap_or(serde_json::Value::Null);
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
        is_error,
        pending_arguments,
        memory_payload,
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
            content: vec![TalosContentItem::Json {
                value: reduced.payload.into(),
            }],
            is_error: reduced.is_error,
            semantic_events: reduced.semantic_events,
            envelope: serde_json::to_value(&reduced.envelope).ok(),
            next_turn_failure: reduced.next_turn_failure,
        }),
    )
    .await;
}

pub(crate) fn collect_general_turn_tool_batch(
    outputs: Vec<ToolExecutionOutput>,
) -> GeneralTurnToolBatch {
    let mut output_items = Vec::new();
    let mut tool_messages = Vec::new();
    let mut pending_dynamic_requests = Vec::new();
    let mut retrieved_memories = Vec::new();
    let mut logs = Vec::new();
    let mut error_count = 0_usize;

    for output in outputs {
        let reduced = reduce_tool_execution_output(&output);
        if let ToolDispatchOutcome::Pending {
            pending: ToolPending::Dynamic { .. },
            ..
        } = &output.outcome
        {
            pending_dynamic_requests.push(PendingDynamicToolRequest {
                tool_name: output.tool_name.clone(),
                call_id: output.call_id.clone(),
                arguments: reduced.pending_arguments.unwrap_or(Value::Null),
            });
            continue;
        }

        if reduced.is_error {
            error_count += 1;
        }
        output_items.push(TalosOutputItem::ToolResult {
            call_id: output.call_id.clone(),
            tool_name: output.tool_name.clone(),
            content: vec![TalosContentItem::Json {
                value: reduced.payload.clone().into(),
            }],
            is_error: reduced.is_error,
        });
        logs.push(ToolResultLog {
            call_id: output.call_id.clone(),
            tool: output.tool_name.clone(),
            payload: reduced.payload.clone(),
            envelope: serde_json::to_value(&reduced.envelope).unwrap_or(Value::Null),
            is_error: reduced.is_error,
            truncated: reduced
                .payload
                .get("truncated")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            size_bytes: serde_json::to_vec(&reduced.payload)
                .map(|buf| buf.len())
                .unwrap_or(0),
            semantic_events: reduced.semantic_events.clone(),
            next_turn_failure: reduced.next_turn_failure.clone(),
        });
        retrieved_memories.extend(parse_retrieved_memory_items(
            &output.tool_name,
            &reduced.memory_payload,
        ));
        tool_messages.push(ChatMessage::tool(
            output.call_id,
            output.tool_name,
            reduced.payload.to_string(),
        ));
    }

    GeneralTurnToolBatch {
        output_items,
        tool_messages,
        pending_dynamic_requests,
        retrieved_memories,
        logs,
        error_count,
    }
}

pub async fn execute_general_turn_tool_batch(
    prepared: &PreparedGeneralTurn,
    tool_executor: Arc<dyn ToolExecutor>,
    tool_registry: Arc<dyn ToolExecutorPort>,
    memory_store: Arc<MemoryStore>,
    kernel_config: Arc<AgentKernelConfig>,
    user_id: &str,
    tool_calls: &[FunctionCall],
    replay_dynamic_tool_results: std::collections::HashMap<String, ReplayDynamicToolResult>,
    stream_tx: Option<&mpsc::Sender<KernelStreamEvent>>,
) -> TalosResult<GeneralTurnToolBatch> {
    let execution_context = ToolExecutionContext {
        session_id: prepared.session_id.clone(),
        user_id: user_id.to_string(),
        project_root: prepared.project_root.clone(),
        kernel_config,
        memory_store,
        dynamic_tool_results: replay_dynamic_tool_results,
    };
    let inputs = tool_calls
        .iter()
        .map(|call| ToolExecutionInput {
            call_id: call.id.clone(),
            tool_name: call.name.clone(),
            arguments: call.arguments.clone().into(),
        })
        .collect::<Vec<_>>();
    let outputs = tool_executor
        .execute(&execution_context, tool_registry, &inputs)
        .await?;

    for output in &outputs {
        emit_tool_result_stream_event(stream_tx, output).await;
    }

    Ok(collect_general_turn_tool_batch(outputs))
}

pub fn apply_general_turn_tool_batch(
    state: &mut GeneralTurnLoopState,
    batch: &GeneralTurnToolBatch,
    kernel_config: &AgentKernelConfig,
) {
    state.messages.extend(batch.tool_messages.clone());
    state.output_items.extend(batch.output_items.clone());
    state.tool_result_log.extend(batch.logs.clone());
    state.tool_error_count += batch.error_count;
    state.pending_dynamic_requests = batch.pending_dynamic_requests.clone();
    state
        .retrieved_memories
        .extend(batch.retrieved_memories.clone());

    if let Some(message) = build_retrieved_memories_message(
        &state.retrieved_memories,
        kernel_config.compaction.retrieved_memories_keep,
        kernel_config.compaction.retrieved_memory_max_chars,
    ) {
        if !state.messages.iter().rev().take(4).any(|item| {
            item.role == talos_llm_provider::Role::System && item.content == message.content
        }) {
            state.messages.push(message);
        }
    }

    state.round += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduce_tool_execution_output_includes_next_turn_failure_payload() {
        let output = ToolExecutionOutput {
            call_id: "call-1".to_string(),
            tool_name: "Read".to_string(),
            outcome: ToolDispatchOutcome::executed(ToolOutputPayload::Function {
                content: String::new(),
                success: false,
                structured_content: Some(serde_json::json!({
                    "error": "path is required",
                    "error_kind": "validation_failure",
                    "retryable": true,
                    "suggested_action": "fix the arguments and retry",
                    "gate_source": "tool_semantic"
                })),
            }),
            is_error: true,
        };

        let reduced = reduce_tool_execution_output(&output);
        assert!(reduced.is_error);
        assert_eq!(
            reduced.envelope.status,
            talos_tools::ToolResultStatus::Failure
        );
        assert!(reduced
            .semantic_events
            .contains(&"tool_use_error".to_string()));
        let next_turn_failure = reduced
            .next_turn_failure
            .expect("next_turn_failure should be present");
        assert_eq!(
            next_turn_failure.get("strategy").and_then(Value::as_str),
            Some("retry")
        );
        assert_eq!(
            next_turn_failure.get("gate_source").and_then(Value::as_str),
            Some("tool_semantic")
        );
    }

    #[tokio::test]
    async fn emit_tool_result_stream_event_includes_envelope_and_semantic_events() {
        let output = ToolExecutionOutput {
            call_id: "call-2".to_string(),
            tool_name: "Bash".to_string(),
            outcome: ToolDispatchOutcome::executed(ToolOutputPayload::Function {
                content: String::new(),
                success: false,
                structured_content: Some(serde_json::json!({
                    "error": "authorization denied",
                    "error_kind": "permission_rejection",
                    "retryable": false,
                    "suggested_action": "choose a less privileged tool or provide a dynamic tool result",
                    "gate_source": "allow_list"
                })),
            }),
            is_error: true,
        };
        let (tx, mut rx) = mpsc::channel(4);

        emit_tool_result_stream_event(Some(&tx), &output).await;
        let event = rx.recv().await.expect("tool result event should exist");

        match event.data {
            KernelStreamEventData::ToolResult(data) => {
                assert!(data.is_error);
                assert_eq!(
                    data.semantic_events,
                    vec![
                        "tool_use_rejected".to_string(),
                        "tool_use_complete".to_string()
                    ]
                );
                assert!(data.envelope.is_some());
                assert_eq!(
                    data.next_turn_failure
                        .as_ref()
                        .and_then(|value| value.get("strategy"))
                        .and_then(Value::as_str),
                    Some("reject")
                );
            }
            other => panic!("expected tool result event, got {other:?}"),
        }
    }

    #[test]
    fn reduce_tool_execution_output_uses_canonical_success_payload() {
        let output = ToolExecutionOutput {
            call_id: "call-3".to_string(),
            tool_name: "Read".to_string(),
            outcome: ToolDispatchOutcome::executed(ToolOutputPayload::success_text(
                "plain text result",
            )),
            is_error: false,
        };

        let reduced = reduce_tool_execution_output(&output);
        assert!(!reduced.is_error);
        assert_eq!(
            reduced.payload,
            serde_json::json!({"result": "plain text result"})
        );
        assert_eq!(
            reduced.memory_payload,
            serde_json::json!({"result": "plain text result"})
        );
    }
}
