use super::*;
use crate::ToolExecutionOutput;

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
    match outcome {
        ToolDispatchOutcome::Pending { pending, .. } => match pending {
            ToolPending::Approval { prompt, arguments } => (
                serde_json::json!({
                    "ok": false,
                    "tool": tool_name,
                    "error": prompt,
                    "truncated": false,
                }),
                true,
                Some(arguments.clone()),
            ),
            ToolPending::Dynamic { arguments } => (
                serde_json::json!({
                    "ok": false,
                    "tool": tool_name,
                    "error": "dynamic tool response is pending",
                    "truncated": false,
                }),
                true,
                Some(arguments.clone()),
            ),
        },
        ToolDispatchOutcome::Executed { payload } => match payload {
            ToolOutputPayload::Function {
                content,
                success,
                structured_content,
            } => {
                let truncated = content.contains("[truncated]");
                if *success {
                    if let Some(structured) = structured_content {
                        (structured.clone(), false, None)
                    } else {
                        (
                            serde_json::json!({
                                "ok": true,
                                "tool": tool_name,
                                "result": content,
                                "truncated": truncated,
                            }),
                            false,
                            None,
                        )
                    }
                } else {
                    let error = structured_content
                        .as_ref()
                        .and_then(|value| value.get("error"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| {
                            if content.trim().is_empty() {
                                None
                            } else {
                                Some(content.clone())
                            }
                        })
                        .unwrap_or_else(|| "unknown error".to_string());
                    let mut value = serde_json::json!({
                        "ok": false,
                        "tool": tool_name,
                        "error": error,
                        "truncated": truncated,
                    });
                    if let Some(structured) = structured_content {
                        if let Some(object) = value.as_object_mut() {
                            object.insert("structured".to_string(), structured.clone());
                        }
                    }
                    (value, true, None)
                }
            }
            ToolOutputPayload::Resource {
                uri,
                mime_type,
                content,
            } => (
                serde_json::json!({
                    "ok": true,
                    "tool": tool_name,
                    "uri": uri,
                    "mime_type": mime_type,
                    "content": content,
                    "truncated": content.contains("[truncated]"),
                }),
                false,
                None,
            ),
            ToolOutputPayload::Mcp { content, is_error } => {
                if *is_error {
                    (
                        serde_json::json!({
                            "ok": false,
                            "tool": tool_name,
                            "error": content,
                            "truncated": content.contains("[truncated]"),
                        }),
                        true,
                        None,
                    )
                } else {
                    (
                        serde_json::json!({
                            "ok": true,
                            "tool": tool_name,
                            "result": content,
                            "truncated": content.contains("[truncated]"),
                        }),
                        false,
                        None,
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
}

fn reduce_tool_execution_output(output: &ToolExecutionOutput) -> ReducedToolExecutionOutput {
    let (payload, is_error, pending_arguments) =
        build_tool_result_payload(&output.tool_name, &output.outcome);
    let memory_payload = output
        .outcome
        .payload()
        .and_then(|payload| payload.structured_content())
        .cloned()
        .unwrap_or_else(|| payload.clone());
    ReducedToolExecutionOutput {
        payload,
        is_error,
        pending_arguments,
        memory_payload,
    }
}

pub(crate) async fn emit_tool_result_stream_event(
    stream_tx: Option<&mpsc::Sender<KernelStreamEvent>>,
    output: &ToolExecutionOutput,
) {
    let reduced = reduce_tool_execution_output(output);
    emit_stream_event(
        stream_tx,
        KernelStreamEventType::ToolResult,
        KernelStreamEventData::ToolResult(ToolResultStreamData {
            call_id: output.call_id.clone(),
            tool_name: output.tool_name.clone(),
            content: vec![TalosContentItem::Json {
                value: reduced.payload,
            }],
            is_error: reduced.is_error,
        }),
    )
    .await;
}

pub(crate) fn collect_general_turn_tool_batch(
    outputs: Vec<ToolExecutionOutput>,
) -> GeneralTurnToolBatch {
    let mut output_items = Vec::new();
    let mut tool_messages = Vec::new();
    let mut pending_confirmations = Vec::new();
    let mut pending_dynamic_requests = Vec::new();
    let mut retrieved_memories = Vec::new();
    let mut logs = Vec::new();
    let mut error_count = 0_usize;

    for output in outputs {
        let reduced = reduce_tool_execution_output(&output);
        if reduced.is_error {
            error_count += 1;
        }
        output_items.push(TalosOutputItem::ToolResult {
            call_id: output.call_id.clone(),
            tool_name: output.tool_name.clone(),
            content: vec![TalosContentItem::Json {
                value: reduced.payload.clone(),
            }],
            is_error: reduced.is_error,
        });
        logs.push(ToolResultLog {
            call_id: output.call_id.clone(),
            tool: output.tool_name.clone(),
            payload: reduced.payload.clone(),
            is_error: reduced.is_error,
            truncated: reduced
                .payload
                .get("truncated")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            size_bytes: serde_json::to_vec(&reduced.payload)
                .map(|buf| buf.len())
                .unwrap_or(0),
        });
        retrieved_memories.extend(parse_retrieved_memory_items(
            &output.tool_name,
            &reduced.memory_payload,
        ));
        if let ToolDispatchOutcome::Pending {
            pending: ToolPending::Approval { prompt, .. },
            ..
        } = &output.outcome
        {
            pending_confirmations.push(PendingToolConfirmation {
                tool_name: output.tool_name.clone(),
                call_id: output.call_id.clone(),
                prompt: prompt.clone(),
                arguments: reduced.pending_arguments.unwrap_or(Value::Null),
            });
        } else if let ToolDispatchOutcome::Pending {
            pending: ToolPending::Dynamic { .. },
            ..
        } = &output.outcome
        {
            pending_dynamic_requests.push(PendingDynamicToolRequest {
                tool_name: output.tool_name.clone(),
                call_id: output.call_id.clone(),
                arguments: reduced.pending_arguments.unwrap_or(Value::Null),
            });
        }
        tool_messages.push(ChatMessage::tool(
            output.call_id,
            output.tool_name,
            reduced.payload.to_string(),
        ));
    }

    GeneralTurnToolBatch {
        output_items,
        tool_messages,
        pending_confirmations,
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
    pre_approved_call_ids: std::collections::HashSet<String>,
    replay_dynamic_tool_results: std::collections::HashMap<String, Value>,
    stream_tx: Option<&mpsc::Sender<KernelStreamEvent>>,
) -> TalosResult<GeneralTurnToolBatch> {
    let execution_context = ToolExecutionContext {
        session_id: prepared.session_id.clone(),
        user_id: user_id.to_string(),
        project_root: prepared.project_root.clone(),
        kernel_config,
        memory_store,
        pre_approved_call_ids,
        dynamic_tool_results: replay_dynamic_tool_results,
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
    state
        .retrieved_memories
        .extend(batch.retrieved_memories.clone());

    if kernel_config.prompt.include_retrieved_memories {
        if let Some(message) = build_retrieved_memories_message(
            &state.retrieved_memories,
            kernel_config.prompt.retrieved_memory_max_items,
            kernel_config.prompt.retrieved_memory_max_chars,
        ) {
            if !state.messages.iter().rev().take(4).any(|item| {
                item.role == talos_llm_provider::Role::System && item.content == message.content
            }) {
                state.messages.push(message);
            }
        }
    }

    state.round += 1;
}
