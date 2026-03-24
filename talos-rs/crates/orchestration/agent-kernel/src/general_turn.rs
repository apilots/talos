use crate::{GeneralTurnContext, KernelStreamEvent};
use chrono::Utc;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use talos_config::AgentKernelConfig;
use talos_error::TalosResult;
use talos_llm_provider::{
    ChatMessage, ChatRequest, FunctionCall, LlmProvider, ToolChoice, ToolStreamEvent,
};
use talos_memory::{
    CanonicalMessage, CanonicalMessageRole, CanonicalTurn, Memory, MemoryStore, RecordHeader,
    SummaryRecord, ToolCallRecord, ToolResultRecord,
};
use talos_prompt::{PromptBudget, PromptBuildConfig, PromptBuilder, PromptLoadPolicy, PromptPhase};
use talos_tools::ToolRegistry;
use talos_types::canonical::{CanonicalContentItem, CanonicalOutputItem, CanonicalUsage};
use tokio::sync::mpsc;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct GeneralTurnResult {
    pub output_text: String,
    pub output_items: Vec<CanonicalOutputItem>,
    pub stop_reason: String,
    pub usage: CanonicalUsage,
}

async fn compact_workspace_context_if_needed(
    session_id: &str,
    selected_model: &str,
    llm_provider: Arc<dyn LlmProvider>,
    memory_store: &MemoryStore,
    kernel_config: &AgentKernelConfig,
    metadata: Option<&Value>,
) -> TalosResult<()> {
    let threshold = metadata_u64(metadata, "memory_compaction_threshold_bytes")
        .or_else(|| metadata_u64(metadata, "context_compaction_max_bytes"))
        .unwrap_or(kernel_config.memory.compaction_threshold_bytes as u64);
    if threshold == 0 {
        return Ok(());
    }
    let session_bytes = memory_store.session_size_bytes(session_id).await?;
    if session_bytes < threshold {
        return Ok(());
    }

    let context = memory_store
        .build_prompt_context(session_id, 48, false, 0, 0)
        .await?;
    if context.recent_messages.is_empty() {
        return Ok(());
    }
    let transcript = context
        .recent_messages
        .iter()
        .map(|message| match message.role {
            CanonicalMessageRole::User => format!("User: {}", message.text.trim()),
            CanonicalMessageRole::Assistant => format!("Assistant: {}", message.text.trim()),
            CanonicalMessageRole::System => format!("System: {}", message.text.trim()),
        })
        .collect::<Vec<_>>()
        .join("\n");
    if transcript.trim().is_empty() {
        return Ok(());
    }

    let selected_model = selected_model.to_string();
    let llm_provider_for_summary = llm_provider.clone();
    let request = ChatRequest::builder()
        .model(selected_model)
        .messages(vec![
            ChatMessage::system(
                "Summarize the conversation into durable facts, user preferences, open tasks, and \
                 unresolved risks. Keep under 220 words.",
            ),
            ChatMessage::user(format!("Conversation transcript:\n{}", transcript)),
        ])
        .build();
    let response = llm_provider_for_summary.chat(request).await?;
    let summary = response.content.trim().to_string();
    if summary.is_empty() {
        return Ok(());
    }
    let latest_seq = memory_store.latest_seq(session_id).await?;
    let source_turn_count = context
        .recent_messages
        .iter()
        .filter_map(|item| item.header.turn_id.as_deref())
        .collect::<std::collections::HashSet<_>>()
        .len();
    memory_store
        .save_summary(&SummaryRecord {
            version: 2,
            header: RecordHeader {
                id: Uuid::new_v4().to_string(),
                session_id: session_id.to_string(),
                turn_id: None,
                timestamp: Utc::now(),
                metadata: HashMap::new(),
            },
            covers_until_seq: latest_seq,
            source_turn_count,
            content: summary,
        })
        .await?;

    Ok(())
}

fn build_reasoning_summary(tool_call_count: usize, tool_error_count: usize) -> String {
    if tool_call_count == 0 {
        "Kernel summary: answered directly without invoking tools.".to_string()
    } else if tool_error_count == 0 {
        format!(
            "Kernel summary: executed {} tool call(s) successfully before final response.",
            tool_call_count
        )
    } else {
        format!(
            "Kernel summary: executed {} tool call(s), {} call(s) returned errors; final response generated with available results.",
            tool_call_count, tool_error_count
        )
    }
}

#[derive(Debug, Default)]
struct PendingToolCall {
    id: Option<String>,
    name: Option<String>,
    arguments_buffer: String,
    final_arguments: Option<Value>,
}

#[derive(Debug, Default)]
struct StreamRound {
    content: String,
    reasoning: String,
    tool_calls: Vec<FunctionCall>,
    finish_reason: Option<String>,
    saw_output_delta: bool,
}

#[derive(Debug, Clone)]
struct ToolResultLog {
    call_id: String,
    tool: String,
    payload: Value,
    is_error: bool,
    truncated: bool,
    size_bytes: usize,
}

fn metadata_u64(metadata: Option<&Value>, key: &str) -> Option<u64> {
    metadata
        .and_then(|m| m.get(key))
        .and_then(|value| value.as_u64())
}

fn metadata_str(metadata: Option<&Value>, key: &str) -> Option<String> {
    metadata
        .and_then(|m| m.get(key))
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
}

async fn emit_stream_event(
    stream_tx: Option<&mpsc::Sender<KernelStreamEvent>>,
    event_type: &str,
    data: Value,
) {
    if let Some(tx) = stream_tx {
        let _ = tx.send(KernelStreamEvent::new(event_type, data)).await;
    }
}

fn finalize_tool_calls(
    round: u32,
    pending_calls: BTreeMap<usize, PendingToolCall>,
) -> Vec<FunctionCall> {
    let mut calls = Vec::new();

    for (index, call) in pending_calls {
        let Some(name) = call.name.filter(|value| !value.trim().is_empty()) else {
            continue;
        };

        let id = call
            .id
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("tool-{}-{}", round, index));
        let arguments = if let Some(value) = call.final_arguments {
            value
        } else if call.arguments_buffer.trim().is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str::<Value>(&call.arguments_buffer)
                .unwrap_or_else(|_| Value::String(call.arguments_buffer))
        };

        calls.push(FunctionCall::new(id, name, arguments));
    }

    calls
}

async fn stream_round(
    llm_provider: Arc<dyn LlmProvider>,
    request: ChatRequest,
    round: u32,
    stream_tx: Option<&mpsc::Sender<KernelStreamEvent>>,
) -> TalosResult<StreamRound> {
    let mut stream = llm_provider.stream_with_tools(request).await?;
    let mut content = String::new();
    let mut reasoning = String::new();
    let mut finish_reason = None;
    let mut pending_calls: BTreeMap<usize, PendingToolCall> = BTreeMap::new();
    let mut saw_output_delta = false;

    while let Some(event) = stream.next().await {
        match event? {
            ToolStreamEvent::TextDelta { delta } => {
                content.push_str(&delta);
                saw_output_delta = true;
                emit_stream_event(
                    stream_tx,
                    "output_delta",
                    json!({
                        "delta": delta,
                        "round": round,
                    }),
                )
                .await;
            }
            ToolStreamEvent::ReasoningDelta { delta } => {
                reasoning.push_str(&delta);
                emit_stream_event(
                    stream_tx,
                    "reasoning_delta",
                    json!({
                        "delta": delta,
                        "source": "provider",
                        "round": round,
                    }),
                )
                .await;
            }
            ToolStreamEvent::ToolCallStart { index, id, name } => {
                let entry = pending_calls.entry(index).or_default();
                if id.is_some() {
                    entry.id = id;
                }
                if name.is_some() {
                    entry.name = name;
                }
            }
            ToolStreamEvent::ToolCallDelta {
                index,
                arguments_delta,
            } => {
                let entry = pending_calls.entry(index).or_default();
                entry.arguments_buffer.push_str(&arguments_delta);
            }
            ToolStreamEvent::ToolCallEnd {
                index,
                id,
                name,
                arguments,
            } => {
                let entry = pending_calls.entry(index).or_default();
                if id.is_some() {
                    entry.id = id;
                }
                if name.is_some() {
                    entry.name = name;
                }
                let arguments_copy = arguments.clone();
                entry.final_arguments = Some(arguments);
                emit_stream_event(
                    stream_tx,
                    "tool_call_start",
                    json!({
                        "index": index,
                        "id": entry.id.clone(),
                        "name": entry.name.clone(),
                        "arguments": arguments_copy,
                        "round": round,
                    }),
                )
                .await;
            }
            ToolStreamEvent::Done {
                finish_reason: final_reason,
            } => {
                finish_reason = final_reason;
                break;
            }
        }
    }

    Ok(StreamRound {
        content,
        reasoning,
        tool_calls: finalize_tool_calls(round, pending_calls),
        finish_reason,
        saw_output_delta,
    })
}

pub async fn run_general_turn(ctx: &GeneralTurnContext) -> TalosResult<GeneralTurnResult> {
    // Extract fields from context for local use
    let session_id = &ctx.session_id;
    let agent_id = &ctx.agent_id;
    let agent_name = &ctx.agent_name;
    let agent_description = &ctx.agent_description;
    let input = &ctx.input;
    let selected_model = &ctx.selected_model;
    let llm_provider = ctx.llm_provider.clone();
    let tool_registry = ctx.tool_registry.clone();
    let memory_store = ctx.memory_store.clone();
    let kernel_config = &ctx.kernel_config;
    let metadata = ctx.metadata.as_ref();
    let stream_tx = ctx.stream_tx.clone();

    let phase = if metadata_str(metadata, "planning_instruction").is_some() {
        PromptPhase::Planning
    } else {
        PromptPhase::Execution
    };

    let mut prompt_builder = PromptBuilder::new()
        .agent(agent_name.to_string(), agent_description.to_string())
        .phase(phase)
        .config(PromptBuildConfig {
            budget: PromptBudget {
                max_static_tokens: kernel_config.prompt.max_static_tokens,
                max_dynamic_tokens: kernel_config.prompt.max_dynamic_tokens,
            },
            load_policy: PromptLoadPolicy {
                include_workspace_summary: kernel_config.prompt.include_workspace_summary,
                include_recent_messages: kernel_config.prompt.include_recent_messages,
                workspace_recent_limit: kernel_config.memory.context_target_messages,
                unified_recent_limit: 10,
            },
        })
        .tools(tool_registry.as_ref())
        .memory_store(session_id, memory_store.as_ref())
        .user_input(input.to_string());
    if let Some(metadata_value) = metadata {
        prompt_builder = prompt_builder.metadata(metadata_value);
    }
    let mut messages = prompt_builder.build().await?;

    let tool_definitions = tool_registry.get_tool_definitions();
    let configured_rounds = metadata_u64(metadata, "max_tool_rounds")
        .unwrap_or(kernel_config.max_tool_rounds as u64) as u32;
    let max_tool_rounds = configured_rounds.max(1);

    let mut tool_context_metadata = metadata
        .and_then(Value::as_object)
        .map(|object| {
            object
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    if !tool_context_metadata.contains_key("tool_output_max_bytes") {
        tool_context_metadata.insert(
            "tool_output_max_bytes".to_string(),
            json!(kernel_config.tool_result_max_bytes),
        );
    }

    let mut round = 0_u32;
    let mut output_items: Vec<CanonicalOutputItem> = Vec::new();
    let usage = CanonicalUsage::default();
    let mut tool_call_count = 0_usize;
    let mut tool_error_count = 0_usize;
    let mut reasoning_log: Vec<String> = Vec::new();
    let mut tool_call_log: Vec<ToolCallRecord> = Vec::new();
    let mut tool_result_log: Vec<ToolResultLog> = Vec::new();
    let mut saw_output_delta = false;

    let (mut final_text, stop_reason) = loop {
        let mut builder = ChatRequest::builder()
            .messages(messages.clone())
            .model(selected_model.to_string());

        if !tool_definitions.is_empty() {
            builder = builder
                .tools(tool_definitions.clone())
                .tool_choice(ToolChoice::Auto)
                .parallel_tool_calls(true);
        }

        let round_response = stream_round(
            llm_provider.clone(),
            builder.build(),
            round,
            stream_tx.as_ref(),
        )
        .await?;
        saw_output_delta = saw_output_delta || round_response.saw_output_delta;

        if !round_response.reasoning.trim().is_empty() {
            let reasoning = round_response.reasoning.trim().to_string();
            output_items.push(CanonicalOutputItem::Reasoning {
                text: reasoning.clone(),
            });
            reasoning_log.push(reasoning);
        }

        if !round_response.tool_calls.is_empty() && !tool_definitions.is_empty() {
            let calls = round_response.tool_calls;
            for call in &calls {
                output_items.push(CanonicalOutputItem::ToolCall {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                });
                tool_call_count += 1;
                tool_call_log.push(ToolCallRecord {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                });
            }

            messages.push(ChatMessage::assistant_with_tool_calls(
                round_response.content.clone(),
                calls.clone(),
            ));

            let context = tool_registry.build_context(
                format!("agent-{}", agent_id),
                agent_name.to_string(),
                tool_context_metadata.clone(),
            );

            let mut tool_messages = Vec::new();
            for call in calls {
                info!(
                    agent_id = %agent_id,
                    tool_name = %call.name,
                    call_id = %call.id,
                    arguments = %call.arguments,
                    "executing tool call: {}",
                    call.name
                );

                match tool_registry
                    .execute_with_context(&call.name, call.arguments.clone(), context.clone())
                    .await
                {
                    Ok(tool_result) => {
                        let output_truncated = tool_result.output.contains("[truncated]");
                        let payload = if tool_result.success {
                            serde_json::json!({
                                "ok": true,
                                "tool": call.name,
                                "result": tool_result.output,
                                "truncated": output_truncated,
                            })
                        } else {
                            tool_error_count += 1;
                            let error = tool_result
                                .error
                                .unwrap_or_else(|| "unknown error".to_string());
                            serde_json::json!({
                                "ok": false,
                                "tool": call.name,
                                "error": error.clone(),
                                "truncated": error.contains("[truncated]"),
                            })
                        };

                        let is_error = payload
                            .get("ok")
                            .and_then(|v| v.as_bool())
                            .map(|ok| !ok)
                            .unwrap_or(true);
                        output_items.push(CanonicalOutputItem::ToolResult {
                            call_id: call.id.clone(),
                            content: vec![CanonicalContentItem::Json {
                                value: payload.clone(),
                            }],
                            is_error,
                        });
                        emit_stream_event(
                            stream_tx.as_ref(),
                            "tool_result",
                            json!({
                                "call_id": call.id.clone(),
                                "tool": call.name.clone(),
                                "payload": payload.clone(),
                                "is_error": is_error,
                            }),
                        )
                        .await;

                        tool_result_log.push(ToolResultLog {
                            call_id: call.id.clone(),
                            tool: call.name.clone(),
                            payload: payload.clone(),
                            is_error,
                            truncated: payload
                                .get("truncated")
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                            size_bytes: serde_json::to_vec(&payload)
                                .map(|buf| buf.len())
                                .unwrap_or(0),
                        });

                        tool_messages.push(ChatMessage::tool(
                            call.id.clone(),
                            call.name.clone(),
                            payload.to_string(),
                        ));
                        info!(
                            agent_id = %agent_id,
                            tool_name = %call.name,
                            call_id = %call.id,
                            ok = payload.get("ok").and_then(|v| v.as_bool()).unwrap_or(false),
                            "tool call completed: {}",
                            call.name
                        );
                    }
                    Err(e) => {
                        tool_error_count += 1;
                        let payload = serde_json::json!({
                            "ok": false,
                            "tool": call.name,
                            "error": e.to_string(),
                        });
                        output_items.push(CanonicalOutputItem::ToolResult {
                            call_id: call.id.clone(),
                            content: vec![CanonicalContentItem::Json {
                                value: payload.clone(),
                            }],
                            is_error: true,
                        });
                        emit_stream_event(
                            stream_tx.as_ref(),
                            "tool_result",
                            json!({
                                "call_id": call.id.clone(),
                                "tool": call.name.clone(),
                                "payload": payload.clone(),
                                "is_error": true,
                            }),
                        )
                        .await;
                        tool_result_log.push(ToolResultLog {
                            call_id: call.id.clone(),
                            tool: call.name.clone(),
                            payload: payload.clone(),
                            is_error: true,
                            truncated: false,
                            size_bytes: serde_json::to_vec(&payload)
                                .map(|buf| buf.len())
                                .unwrap_or(0),
                        });
                        tool_messages.push(ChatMessage::tool(
                            call.id.clone(),
                            call.name.clone(),
                            payload.to_string(),
                        ));
                        info!(
                            agent_id = %agent_id,
                            tool_name = %call.name,
                            call_id = %call.id,
                            error = %e,
                            "tool call failed: {}",
                            call.name
                        );
                    }
                }
            }

            messages.extend(tool_messages);
            round += 1;
            if round >= max_tool_rounds {
                break (
                    "[tool-call rounds exceeded]".to_string(),
                    "tool_round_limit".to_string(),
                );
            }
            continue;
        }

        let final_text = round_response.content.clone();
        let stop_reason = round_response
            .finish_reason
            .clone()
            .unwrap_or_else(|| "end_turn".to_string());
        messages.push(ChatMessage::assistant(round_response.content.clone()));
        break (final_text, stop_reason);
    };

    if final_text.trim().is_empty() {
        final_text = if tool_call_count > 0 {
            "Tool execution completed.".to_string()
        } else {
            "No response generated.".to_string()
        };
    }

    if !saw_output_delta && !final_text.trim().is_empty() {
        emit_stream_event(
            stream_tx.as_ref(),
            "output_delta",
            json!({
                "delta": final_text.clone(),
                "source": "kernel_fallback",
            }),
        )
        .await;
    }

    let reasoning_summary = build_reasoning_summary(tool_call_count, tool_error_count);
    emit_stream_event(
        stream_tx.as_ref(),
        "reasoning_delta",
        json!({
            "delta": reasoning_summary.clone(),
            "source": "kernel",
        }),
    )
    .await;
    output_items.push(CanonicalOutputItem::Reasoning {
        text: reasoning_summary.clone(),
    });
    output_items.push(CanonicalOutputItem::OutputText {
        text: final_text.clone(),
    });

    let seq = memory_store.next_turn_seq(session_id).await?;
    let turn_id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let canonical_turn = CanonicalTurn {
        turn_id: turn_id.clone(),
        session_id: session_id.to_string(),
        seq,
        timestamp: now,
        messages: vec![
            CanonicalMessage {
                header: RecordHeader {
                    id: format!("{}:u", turn_id),
                    session_id: session_id.to_string(),
                    turn_id: Some(turn_id.clone()),
                    timestamp: now,
                    metadata: HashMap::new(),
                },
                role: CanonicalMessageRole::User,
                text: input.to_string(),
            },
            CanonicalMessage {
                header: RecordHeader {
                    id: format!("{}:a", turn_id),
                    session_id: session_id.to_string(),
                    turn_id: Some(turn_id.clone()),
                    timestamp: now,
                    metadata: HashMap::new(),
                },
                role: CanonicalMessageRole::Assistant,
                text: final_text.clone(),
            },
        ],
        reasoning: {
            let mut values = reasoning_log;
            values.push(reasoning_summary.clone());
            values
        },
        tool_calls: tool_call_log,
        tool_results: tool_result_log
            .iter()
            .map(|item| ToolResultRecord {
                header: RecordHeader {
                    id: format!("{}:tool:{}", turn_id, item.call_id),
                    session_id: session_id.to_string(),
                    turn_id: Some(turn_id.clone()),
                    timestamp: now,
                    metadata: HashMap::new(),
                },
                call_id: item.call_id.clone(),
                tool: item.tool.clone(),
                payload: item.payload.clone(),
                is_error: item.is_error,
                truncated: item.truncated,
                size_bytes: item.size_bytes,
            })
            .collect(),
    };
    if let Err(err) = memory_store.append_turn(&canonical_turn).await {
        warn!(
            session_id = %session_id,
            error = %err,
            "failed to append memory store turn"
        );
    }

    if let Err(err) = compact_workspace_context_if_needed(
        session_id,
        selected_model,
        llm_provider.clone(),
        memory_store.as_ref(),
        kernel_config,
        metadata,
    )
    .await
    {
        warn!(
            session_id = %session_id,
            error = %err,
            "failed to compact workspace memory context"
        );
    }

    Ok(GeneralTurnResult {
        output_text: final_text,
        output_items,
        stop_reason,
        usage,
    })
}
