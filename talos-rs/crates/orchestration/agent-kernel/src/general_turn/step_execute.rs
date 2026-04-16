use super::*;
use crate::{ToolArgsDeltaStreamData, ToolExecutionOutput, ToolStreamKey};

pub(crate) async fn emit_stream_event(
    stream_tx: Option<&mpsc::Sender<KernelStreamEvent>>,
    event_type: KernelStreamEventType,
    data: KernelStreamEventData,
) {
    if let Some(tx) = stream_tx {
        let _ = tx.send(KernelStreamEvent::new(event_type, data)).await;
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
pub(crate) struct StreamRound {
    pub(crate) content: String,
    pub(crate) reasoning: String,
    pub(crate) tool_calls: Vec<FunctionCall>,
    pub(crate) streamed_tool_outputs: Vec<ToolExecutionOutput>,
    pub(crate) finish_reason: Option<String>,
    pub(crate) saw_output_delta: bool,
    pub(crate) usage: TalosUsage,
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

        let id = normalized_tool_call_id(round, index, call.id);
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

fn normalized_tool_call_id(round: u32, index: usize, id: Option<String>) -> String {
    id.filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("tool-{}-{}", round, index))
}

fn talos_usage_from_provider_usage(usage: &talos_llm_provider::Usage) -> TalosUsage {
    TalosUsage {
        input_tokens: usage.prompt_tokens,
        output_tokens: usage.completion_tokens,
        total_tokens: usage.total_tokens,
        cache_creation_input_tokens: usage.cache_creation_input_tokens,
        cache_read_input_tokens: usage.cache_read_input_tokens,
    }
}

pub(crate) async fn stream_round(
    llm_provider: Arc<dyn LlmProvider>,
    request: ChatRequest,
    round: u32,
    stream_tx: Option<&mpsc::Sender<KernelStreamEvent>>,
    streaming_tool_execution: Option<StreamingToolExecutionConfig>,
) -> TalosResult<StreamRound> {
    let mut stream = llm_provider.stream_with_tools(request).await?;
    let mut content = String::new();
    let mut reasoning = String::new();
    let mut finish_reason = None;
    let mut pending_calls: BTreeMap<usize, PendingToolCall> = BTreeMap::new();
    let mut streamed_tool_outputs = Vec::new();
    let mut saw_output_delta = false;
    let mut usage = TalosUsage::default();
    let mut scheduler = streaming_tool_execution.map(|config| {
        crate::tool_execution::StreamingToolScheduler::new(
            config.execution_mode,
            config.execution_context,
            config.tool_registry,
            stream_tx.cloned(),
        )
    });

    while let Some(event) = stream.next().await {
        match event? {
            ToolStreamEvent::TextDelta { delta } => {
                content.push_str(&delta);
                saw_output_delta = true;
                emit_stream_event(
                    stream_tx,
                    KernelStreamEventType::OutputDelta,
                    KernelStreamEventData::OutputDelta(OutputDeltaStreamData {
                        delta,
                        round: Some(round),
                        source: Some(KernelOutputDeltaSource::Provider),
                    }),
                )
                .await;
            }
            ToolStreamEvent::ReasoningSignature { .. } => {
                // thinking block integrity signature — no action needed at kernel level
            }
            ToolStreamEvent::ReasoningDelta { delta } => {
                reasoning.push_str(&delta);
                emit_stream_event(
                    stream_tx,
                    KernelStreamEventType::ThinkingDelta,
                    KernelStreamEventData::ThinkingDelta(ThinkingDeltaStreamData {
                        delta,
                        source: KernelThinkingSource::Provider,
                        stage: KernelThinkingStage::ModelReasoning,
                        round: Some(round),
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
                emit_stream_event(
                    stream_tx,
                    KernelStreamEventType::ToolArgsDelta,
                    KernelStreamEventData::ToolArgsDelta(ToolArgsDeltaStreamData {
                        stream_key: ToolStreamKey { round, index },
                        call_id: entry.id.clone(),
                        tool_name: entry.name.clone(),
                        delta: arguments_delta,
                    }),
                )
                .await;
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
                let stable_call_id = normalized_tool_call_id(round, index, entry.id.clone());
                entry.id = Some(stable_call_id.clone());
                entry.final_arguments = Some(arguments);
                emit_stream_event(
                    stream_tx,
                    KernelStreamEventType::ToolCallStart,
                    KernelStreamEventData::ToolCallStart(ToolCallStartStreamData {
                        index,
                        call_id: Some(stable_call_id.clone()),
                        tool_name: entry.name.clone(),
                        arguments: arguments_copy.clone(),
                        round,
                    }),
                )
                .await;
                if let Some(scheduler) = scheduler.as_mut() {
                    scheduler
                        .push_call(
                            ToolStreamKey { round, index },
                            ToolExecutionInput {
                                call_id: stable_call_id,
                                tool_name: entry
                                    .name
                                    .clone()
                                    .unwrap_or_else(|| "unknown".to_string()),
                                arguments: arguments_copy,
                            },
                        )
                        .await?;
                }
            }
            ToolStreamEvent::Done {
                finish_reason: final_reason,
                usage: final_usage,
            } => {
                finish_reason = final_reason;
                if let Some(final_usage) = final_usage {
                    usage = talos_usage_from_provider_usage(&final_usage);
                }
                break;
            }
        }

        if let Some(scheduler) = scheduler.as_mut() {
            for completed in scheduler.drain_completed().await? {
                super::tool_batch::emit_tool_result_stream_event(stream_tx, &completed.output)
                    .await;
                streamed_tool_outputs.push(completed.output);
            }
        }
    }

    if let Some(scheduler) = scheduler.as_mut() {
        for completed in scheduler.finish().await? {
            super::tool_batch::emit_tool_result_stream_event(stream_tx, &completed.output).await;
            streamed_tool_outputs.push(completed.output);
        }
    }

    Ok(StreamRound {
        content,
        reasoning,
        tool_calls: finalize_tool_calls(round, pending_calls),
        streamed_tool_outputs,
        finish_reason,
        saw_output_delta,
        usage,
    })
}

async fn chat_round(
    llm_provider: Arc<dyn LlmProvider>,
    request: ChatRequest,
) -> TalosResult<StreamRound> {
    let response = llm_provider.chat_with_tools(request).await?;
    let tool_calls = response.get_tool_calls().to_vec();
    Ok(StreamRound {
        content: response.content,
        reasoning: response.reasoning.join(""),
        tool_calls,
        streamed_tool_outputs: Vec::new(),
        finish_reason: response.finish_reason,
        saw_output_delta: false,
        usage: talos_usage_from_provider_usage(&response.usage),
    })
}

pub async fn execute_general_turn_step(
    step: GeneralTurnStepRequest,
    llm_provider: Arc<dyn LlmProvider>,
) -> TalosResult<GeneralTurnStepResult> {
    let round = step.round;
    let stream_tx = step.stream_tx.clone();
    let streaming_tool_execution = step.streaming_tool_execution.clone();
    let round_response = if stream_tx.is_some() {
        stream_round(
            llm_provider,
            step.into_chat_request(),
            round,
            stream_tx.as_ref(),
            streaming_tool_execution,
        )
        .await?
    } else {
        chat_round(llm_provider, step.into_chat_request()).await?
    };

    let mut output_items = Vec::new();
    let mut reasoning = Vec::new();
    if !round_response.reasoning.trim().is_empty() {
        let reasoning_text = round_response.reasoning.trim().to_string();
        output_items.push(TalosOutputItem::Reasoning {
            text: reasoning_text.clone(),
        });
        reasoning.push(reasoning_text);
    }

    for call in &round_response.tool_calls {
        output_items.push(TalosOutputItem::ToolCall {
            call_id: call.id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.clone(),
        });
    }

    let disposition = if round_response.tool_calls.is_empty() {
        GeneralTurnStepDisposition::FinalResponse
    } else {
        GeneralTurnStepDisposition::ContinueWithTools
    };
    let _finish_reason = ModelFinishReason::from_provider(
        round_response.finish_reason.as_deref(),
        !round_response.tool_calls.is_empty(),
    );

    Ok(GeneralTurnStepResult {
        text: round_response.content,
        reasoning,
        tool_calls: round_response.tool_calls,
        streamed_tool_outputs: round_response.streamed_tool_outputs,
        output_items,
        usage: round_response.usage,
        saw_output_delta: round_response.saw_output_delta,
        disposition,
    })
}

pub fn apply_general_turn_step(state: &mut GeneralTurnLoopState, step: &GeneralTurnStepResult) {
    state.saw_output_delta = state.saw_output_delta || step.saw_output_delta;
    state.usage.accumulate(&step.usage);
    state.output_items.extend(step.output_items.clone());
    state.reasoning_log.extend(step.reasoning.clone());
    if !step.tool_calls.is_empty() {
        state.messages.push(ChatMessage::assistant_with_tool_calls(
            step.text.clone(),
            step.tool_calls.clone(),
        ));
    }
    for call in &step.tool_calls {
        state.tool_call_count += 1;
        state.tool_call_log.push(ToolCallRecord {
            call_id: call.id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.clone(),
        });
    }
}
