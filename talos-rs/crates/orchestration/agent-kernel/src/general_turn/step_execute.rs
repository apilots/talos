use super::types::ToolCallRecord;
use super::*;
use crate::{
    AssistantTextCommittedStreamData, ToolArgsDeltaStreamData, ToolExecutionOutput,
    ToolProgressDeltaStreamData, ToolProgressKind, ToolStreamKey,
};
use std::collections::BTreeSet;

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

#[derive(Debug, Clone)]
enum ActiveModelBlock {
    Reasoning,
    AssistantText {
        item_key: KernelStreamItemKey,
        phase: KernelAssistantTextPhase,
    },
    ToolCall {
        index: usize,
    },
}

#[derive(Debug, Default)]
pub(crate) struct StreamRound {
    pub(crate) content: String,
    pub(crate) reasoning: ReasoningOutput,
    pub(crate) tool_calls: Vec<FunctionCall>,
    pub(crate) streamed_tool_outputs: Vec<ToolExecutionOutput>,
    pub(crate) finish_reason: Option<String>,
    pub(crate) usage: TokenUsage,
    pub(crate) assistant_text_streamed: bool,
    pub(crate) final_answer_streamed: bool,
    pub(crate) execution_summary_streamed: bool,
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
                .unwrap_or(Value::String(call.arguments_buffer))
        };

        calls.push(FunctionCall::new(id, name, arguments));
    }

    calls
}

fn normalized_tool_call_id(round: u32, index: usize, id: Option<String>) -> String {
    id.filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("tool-{}-{}", round, index))
}

fn talos_usage_from_provider_usage(usage: &talos_llm_provider::Usage) -> TokenUsage {
    TokenUsage {
        input_tokens: usage.prompt_tokens,
        output_tokens: usage.completion_tokens,
        total_tokens: usage.total_tokens,
        cache_creation_input_tokens: usage.cache_creation_input_tokens,
        cache_read_input_tokens: usage.cache_read_input_tokens,
    }
}

fn map_assistant_text_phase(
    call_mode: KernelModelCallMode,
    phase_hint: Option<AssistantTextPhaseHint>,
) -> KernelAssistantTextPhase {
    match call_mode {
        KernelModelCallMode::Finalization => KernelAssistantTextPhase::FinalAnswer,
        KernelModelCallMode::WorkingLoop => match phase_hint {
            Some(AssistantTextPhaseHint::FinalAnswer) => KernelAssistantTextPhase::FinalAnswer,
            Some(AssistantTextPhaseHint::Commentary) => KernelAssistantTextPhase::Commentary,
            None => KernelAssistantTextPhase::Unknown,
        },
    }
}

fn provider_protocol_violation(message: impl Into<String>) -> TalosError {
    TalosError::InternalError(format!("provider protocol violation: {}", message.into()))
}

fn ensure_not_after_final(final_answer_started: bool, event: &str) -> TalosResult<()> {
    if final_answer_started {
        return Err(provider_protocol_violation(format!(
            "{event} received after final answer started"
        )));
    }
    Ok(())
}

async fn close_active_non_final_blocks_before_final(
    active_blocks: &mut BTreeMap<String, ActiveModelBlock>,
    completed_blocks: &mut BTreeSet<String>,
    stream_tx: Option<&mpsc::Sender<KernelStreamEvent>>,
) -> TalosResult<()> {
    let active_tool = active_blocks.iter().find_map(|(id, block)| {
        matches!(block, ActiveModelBlock::ToolCall { .. }).then_some(id.clone())
    });
    if let Some(model_item_id) = active_tool {
        return Err(provider_protocol_violation(format!(
            "final answer started with active tool call block {model_item_id}"
        )));
    }

    let active_ids: Vec<String> = active_blocks.keys().cloned().collect();
    for model_item_id in active_ids {
        let Some(block) = active_blocks.remove(&model_item_id) else {
            continue;
        };
        completed_blocks.insert(model_item_id);
        if let ActiveModelBlock::AssistantText {
            item_key,
            phase: KernelAssistantTextPhase::Commentary | KernelAssistantTextPhase::Unknown,
        } = block
        {
            emit_stream_event(
                stream_tx,
                KernelStreamEventType::AssistantTextCompleted,
                KernelStreamEventData::AssistantTextCompleted(AssistantTextCompletedStreamData {
                    item_key,
                }),
            )
            .await;
        }
    }
    Ok(())
}

pub(crate) async fn stream_round(
    llm_provider: Arc<dyn LlmProvider>,
    request: ChatRequest,
    round: u32,
    call_mode: KernelModelCallMode,
    _pre_final_execution_summary: Option<String>,
    stream_tx: Option<&mpsc::Sender<KernelStreamEvent>>,
    streaming_tool_execution: Option<StreamingToolExecutionConfig>,
) -> TalosResult<StreamRound> {
    let mut stream = llm_provider.stream_model_blocks(request).await?;
    let mut content = String::new();
    let mut streamed_reasoning = String::new();
    let mut completed_reasoning: Option<ReasoningOutput> = None;
    let mut finish_reason = None;
    let mut pending_calls: BTreeMap<usize, PendingToolCall> = BTreeMap::new();
    let mut active_blocks = BTreeMap::<String, ActiveModelBlock>::new();
    let mut completed_blocks = BTreeSet::<String>::new();
    let mut streamed_tool_outputs = Vec::new();
    let mut usage = TokenUsage::default();
    let mut assistant_text_streamed = false;
    let mut final_answer_streamed = false;
    let execution_summary_streamed = false;
    let mut final_answer_started = false;
    let mut unknown_assistant_text_items = Vec::<KernelStreamItemKey>::new();
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
            ModelBlockStreamEvent::BlockStarted(started) => match started.kind {
                ModelBlockKind::Reasoning => {
                    ensure_not_after_final(final_answer_started, "reasoning block")?;
                    if active_blocks.contains_key(&started.model_item_id) {
                        return Err(provider_protocol_violation(format!(
                            "duplicate active model block {}",
                            started.model_item_id
                        )));
                    }
                    if completed_blocks.contains(&started.model_item_id) {
                        return Err(provider_protocol_violation(format!(
                            "model block restarted after completion {}",
                            started.model_item_id
                        )));
                    }
                    active_blocks.insert(started.model_item_id, ActiveModelBlock::Reasoning);
                }
                ModelBlockKind::AssistantText => {
                    let phase =
                        map_assistant_text_phase(call_mode, started.assistant_text_phase_hint);
                    let model_item_id = started.model_item_id;
                    if active_blocks.contains_key(&model_item_id) {
                        return Err(provider_protocol_violation(format!(
                            "duplicate active model block {model_item_id}"
                        )));
                    }
                    if completed_blocks.contains(&model_item_id) {
                        return Err(provider_protocol_violation(format!(
                            "model block restarted after completion {model_item_id}"
                        )));
                    }
                    let item_key = KernelStreamItemKey {
                        round: Some(round),
                        local_id: model_item_id.clone(),
                    };

                    if matches!(phase, KernelAssistantTextPhase::FinalAnswer) {
                        if final_answer_started {
                            return Err(provider_protocol_violation(
                                "multiple final answer blocks in one stream",
                            ));
                        }
                        if !pending_calls.is_empty() {
                            return Err(provider_protocol_violation(
                                "final answer started after tool call was emitted in same round",
                            ));
                        }
                        close_active_non_final_blocks_before_final(
                            &mut active_blocks,
                            &mut completed_blocks,
                            stream_tx,
                        )
                        .await?;
                        final_answer_started = true;
                        final_answer_streamed = true;
                    } else if matches!(phase, KernelAssistantTextPhase::Commentary) {
                        ensure_not_after_final(final_answer_started, "commentary assistant text")?;
                    } else {
                        ensure_not_after_final(final_answer_started, "unknown assistant text")?;
                    }

                    assistant_text_streamed = true;
                    if matches!(phase, KernelAssistantTextPhase::Unknown) {
                        unknown_assistant_text_items.push(item_key.clone());
                    }
                    active_blocks.insert(
                        model_item_id,
                        ActiveModelBlock::AssistantText {
                            item_key: item_key.clone(),
                            phase,
                        },
                    );
                    emit_stream_event(
                        stream_tx,
                        KernelStreamEventType::AssistantTextStarted,
                        KernelStreamEventData::AssistantTextStarted(
                            AssistantTextStartedStreamData {
                                item_key,
                                phase,
                                round: Some(round),
                            },
                        ),
                    )
                    .await;
                }
                ModelBlockKind::ToolCall => {
                    ensure_not_after_final(final_answer_started, "tool call block")?;
                    if active_blocks.contains_key(&started.model_item_id) {
                        return Err(provider_protocol_violation(format!(
                            "duplicate active model block {}",
                            started.model_item_id
                        )));
                    }
                    if completed_blocks.contains(&started.model_item_id) {
                        return Err(provider_protocol_violation(format!(
                            "model block restarted after completion {}",
                            started.model_item_id
                        )));
                    }
                    let Some(tool_call) = started.tool_call else {
                        return Err(provider_protocol_violation(
                            "tool call model block started without tool metadata",
                        ));
                    };
                    let entry = pending_calls.entry(tool_call.index).or_default();
                    if tool_call.call_id.is_some() {
                        entry.id = tool_call.call_id;
                    }
                    if tool_call.tool_name.is_some() {
                        entry.name = tool_call.tool_name;
                    }
                    active_blocks.insert(
                        started.model_item_id,
                        ActiveModelBlock::ToolCall {
                            index: tool_call.index,
                        },
                    );
                }
                ModelBlockKind::ToolResult | ModelBlockKind::SystemNotice => {
                    ensure_not_after_final(final_answer_started, "non-final model block")?;
                }
            },
            ModelBlockStreamEvent::BlockDelta {
                model_item_id,
                delta,
            } => match delta {
                ModelBlockDelta::AssistantText { delta } => {
                    let Some(ActiveModelBlock::AssistantText { item_key, .. }) =
                        active_blocks.get(&model_item_id)
                    else {
                        return Err(provider_protocol_violation(format!(
                            "assistant text delta received without active assistant block {model_item_id}"
                        )));
                    };
                    content.push_str(&delta);
                    emit_stream_event(
                        stream_tx,
                        KernelStreamEventType::AssistantTextDelta,
                        KernelStreamEventData::AssistantTextDelta(AssistantTextDeltaStreamData {
                            item_key: item_key.clone(),
                            delta,
                        }),
                    )
                    .await;
                }
                ModelBlockDelta::ReasoningText { delta } => {
                    ensure_not_after_final(final_answer_started, "reasoning delta")?;
                    if !matches!(
                        active_blocks.get(&model_item_id),
                        Some(ActiveModelBlock::Reasoning)
                    ) {
                        return Err(provider_protocol_violation(format!(
                            "reasoning delta received without active reasoning block {model_item_id}"
                        )));
                    }
                    streamed_reasoning.push_str(&delta);
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
                ModelBlockDelta::ToolArguments { delta } => {
                    ensure_not_after_final(final_answer_started, "tool arguments delta")?;
                    let Some(ActiveModelBlock::ToolCall { index }) =
                        active_blocks.get(&model_item_id)
                    else {
                        return Err(provider_protocol_violation(format!(
                            "tool arguments delta received without active tool block {model_item_id}"
                        )));
                    };
                    let index = *index;
                    let entry = pending_calls.entry(index).or_default();
                    entry.arguments_buffer.push_str(&delta);
                    emit_stream_event(
                        stream_tx,
                        KernelStreamEventType::ToolArgsDelta,
                        KernelStreamEventData::ToolArgsDelta(ToolArgsDeltaStreamData {
                            stream_key: ToolStreamKey { round, index },
                            call_id: entry.id.clone(),
                            tool_name: entry.name.clone(),
                            delta,
                        }),
                    )
                    .await;
                }
                ModelBlockDelta::ToolProgress { delta } => {
                    ensure_not_after_final(final_answer_started, "tool progress delta")?;
                    let Some(ActiveModelBlock::ToolCall { index }) =
                        active_blocks.get(&model_item_id)
                    else {
                        return Err(provider_protocol_violation(format!(
                            "tool progress delta received without active tool block {model_item_id}"
                        )));
                    };
                    let index = *index;
                    let entry = pending_calls.entry(index).or_default();
                    let call_id = normalized_tool_call_id(round, index, entry.id.clone());
                    entry.id = Some(call_id.clone());
                    let tool_name = entry.name.clone().unwrap_or_else(|| "unknown".to_string());
                    emit_stream_event(
                        stream_tx,
                        KernelStreamEventType::ToolProgressDelta,
                        KernelStreamEventData::ToolProgressDelta(ToolProgressDeltaStreamData {
                            stream_key: ToolStreamKey { round, index },
                            call_id,
                            tool_name,
                            message: delta,
                            progress_kind: ToolProgressKind::Status,
                        }),
                    )
                    .await;
                }
            },
            ModelBlockStreamEvent::BlockCompleted(completed) => {
                let Some(active_block) = active_blocks.remove(&completed.model_item_id) else {
                    if completed_blocks.contains(&completed.model_item_id) {
                        continue;
                    }
                    return Err(provider_protocol_violation(format!(
                        "model block completed without active block {}",
                        completed.model_item_id
                    )));
                };
                completed_blocks.insert(completed.model_item_id);
                match active_block {
                    ActiveModelBlock::Reasoning => {}
                    ActiveModelBlock::AssistantText { item_key, .. } => {
                        emit_stream_event(
                            stream_tx,
                            KernelStreamEventType::AssistantTextCompleted,
                            KernelStreamEventData::AssistantTextCompleted(
                                AssistantTextCompletedStreamData { item_key },
                            ),
                        )
                        .await;
                    }
                    ActiveModelBlock::ToolCall { index } => {
                        ensure_not_after_final(final_answer_started, "tool call completion")?;
                        let entry = pending_calls.entry(index).or_default();
                        let arguments = if entry.arguments_buffer.trim().is_empty() {
                            serde_json::json!({})
                        } else {
                            serde_json::from_str::<Value>(&entry.arguments_buffer)
                                .unwrap_or_else(|_| Value::String(entry.arguments_buffer.clone()))
                        };
                        let stable_call_id =
                            normalized_tool_call_id(round, index, entry.id.clone());
                        entry.id = Some(stable_call_id.clone());
                        entry.final_arguments = Some(arguments.clone());
                        emit_stream_event(
                            stream_tx,
                            KernelStreamEventType::ToolCallStart,
                            KernelStreamEventData::ToolCallStart(ToolCallStartStreamData {
                                index,
                                call_id: Some(stable_call_id.clone()),
                                tool_name: entry.name.clone(),
                                arguments: arguments.clone(),
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
                                        arguments,
                                    },
                                )
                                .await?;
                        }
                    }
                }
            }
            ModelBlockStreamEvent::ReasoningCompleted { output } => {
                if completed_reasoning.replace(output).is_some() {
                    return Err(provider_protocol_violation(
                        "provider emitted multiple completed reasoning outputs",
                    ));
                }
            }
            ModelBlockStreamEvent::Done {
                finish_reason: final_reason,
                usage: final_usage,
            } => {
                if !active_blocks.is_empty() {
                    let active_ids = active_blocks.keys().cloned().collect::<Vec<_>>().join(", ");
                    return Err(provider_protocol_violation(format!(
                        "provider stream completed with active model blocks: {active_ids}"
                    )));
                }
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

    if finish_reason.is_none() {
        return Err(TalosError::InternalError(
            "LLM stream ended before a terminal provider event".to_string(),
        ));
    }

    let tool_calls = finalize_tool_calls(round, pending_calls);
    let reasoning = match completed_reasoning {
        Some(output) => {
            if output.display_text() != streamed_reasoning {
                return Err(provider_protocol_violation(
                    "completed reasoning output does not match streamed reasoning text",
                ));
            }
            output
        }
        None if streamed_reasoning.is_empty() => ReasoningOutput::default(),
        None => {
            return Err(provider_protocol_violation(
                "reasoning deltas completed without provider continuation output",
            ));
        }
    };
    if !unknown_assistant_text_items.is_empty() {
        let phase = if tool_calls.is_empty() {
            KernelAssistantTextPhase::FinalAnswer
        } else {
            KernelAssistantTextPhase::Commentary
        };
        for item_key in unknown_assistant_text_items {
            emit_stream_event(
                stream_tx,
                KernelStreamEventType::AssistantTextCommitted,
                KernelStreamEventData::AssistantTextCommitted(AssistantTextCommittedStreamData {
                    item_key,
                    phase,
                }),
            )
            .await;
        }
        if matches!(phase, KernelAssistantTextPhase::FinalAnswer) {
            final_answer_streamed = true;
        }
    }
    if !assistant_text_streamed && !content.trim().is_empty() {
        let phase = if tool_calls.is_empty() {
            KernelAssistantTextPhase::FinalAnswer
        } else {
            KernelAssistantTextPhase::Commentary
        };
        assistant_text_streamed = emit_assistant_text_stream_item(
            stream_tx,
            KernelStreamItemKey {
                round: Some(round),
                local_id: "round_terminal_output".to_string(),
            },
            phase,
            Some(round),
            &content,
        )
        .await;
        final_answer_streamed =
            assistant_text_streamed && matches!(phase, KernelAssistantTextPhase::FinalAnswer);
    }

    Ok(StreamRound {
        content,
        reasoning,
        tool_calls,
        streamed_tool_outputs,
        finish_reason,
        usage,
        assistant_text_streamed,
        final_answer_streamed,
        execution_summary_streamed,
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
        reasoning: response.reasoning,
        tool_calls,
        streamed_tool_outputs: Vec::new(),
        finish_reason: response.finish_reason,
        usage: talos_usage_from_provider_usage(&response.usage),
        assistant_text_streamed: false,
        final_answer_streamed: false,
        execution_summary_streamed: false,
    })
}

pub(crate) async fn emit_assistant_text_stream_item(
    stream_tx: Option<&mpsc::Sender<KernelStreamEvent>>,
    item_key: KernelStreamItemKey,
    phase: KernelAssistantTextPhase,
    round: Option<u32>,
    text: &str,
) -> bool {
    if stream_tx.is_none() || text.trim().is_empty() {
        return false;
    }

    emit_stream_event(
        stream_tx,
        KernelStreamEventType::AssistantTextStarted,
        KernelStreamEventData::AssistantTextStarted(AssistantTextStartedStreamData {
            item_key: item_key.clone(),
            phase,
            round,
        }),
    )
    .await;
    emit_stream_event(
        stream_tx,
        KernelStreamEventType::AssistantTextDelta,
        KernelStreamEventData::AssistantTextDelta(AssistantTextDeltaStreamData {
            item_key: item_key.clone(),
            delta: text.to_string(),
        }),
    )
    .await;
    emit_stream_event(
        stream_tx,
        KernelStreamEventType::AssistantTextCompleted,
        KernelStreamEventData::AssistantTextCompleted(AssistantTextCompletedStreamData {
            item_key,
        }),
    )
    .await;
    true
}

async fn emit_assistant_text_for_step(
    stream_tx: Option<&mpsc::Sender<KernelStreamEvent>>,
    round: u32,
    text: &str,
    disposition: GeneralTurnStepDisposition,
) -> bool {
    let phase = match disposition {
        GeneralTurnStepDisposition::ContinueWithTools => KernelAssistantTextPhase::Commentary,
        GeneralTurnStepDisposition::FinalResponse => KernelAssistantTextPhase::FinalAnswer,
    };
    emit_assistant_text_stream_item(
        stream_tx,
        KernelStreamItemKey {
            round: Some(round),
            local_id: "assistant_text".to_string(),
        },
        phase,
        Some(round),
        text,
    )
    .await
}

pub async fn execute_general_turn_step(
    step: GeneralTurnStepRequest,
    llm_provider: Arc<dyn LlmProvider>,
) -> TalosResult<GeneralTurnStepResult> {
    let round = step.round;
    let call_mode = step.call_mode;
    let stream_tx = step.stream_tx.clone();
    let streaming_tool_execution = step.streaming_tool_execution.clone();
    let pre_final_execution_summary = step.pre_final_execution_summary.clone();
    let round_response = if stream_tx.is_some() {
        stream_round(
            llm_provider,
            step.into_chat_request(),
            round,
            call_mode,
            pre_final_execution_summary,
            stream_tx.as_ref(),
            streaming_tool_execution,
        )
        .await?
    } else {
        chat_round(llm_provider, step.into_chat_request()).await?
    };

    let mut output_items = Vec::new();
    let reasoning_text = round_response.reasoning.display_text();
    if !reasoning_text.trim().is_empty() {
        let reasoning_text = reasoning_text.trim().to_string();
        output_items.push(RunItem::Reasoning {
            text: reasoning_text,
        });
    }

    for call in &round_response.tool_calls {
        output_items.push(RunItem::ToolCall {
            call_id: call.id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.clone().into(),
        });
    }

    let disposition = if round_response.tool_calls.is_empty() {
        GeneralTurnStepDisposition::FinalResponse
    } else {
        GeneralTurnStepDisposition::ContinueWithTools
    };
    let final_answer_streamed = if round_response.assistant_text_streamed {
        round_response.final_answer_streamed
    } else {
        emit_assistant_text_for_step(
            stream_tx.as_ref(),
            round,
            &round_response.content,
            disposition,
        )
        .await
            && matches!(disposition, GeneralTurnStepDisposition::FinalResponse)
    };
    let model_finish_reason = ModelFinishReason::from_provider(
        round_response.finish_reason.as_deref(),
        !round_response.tool_calls.is_empty(),
    );

    Ok(GeneralTurnStepResult {
        text: round_response.content,
        reasoning: round_response.reasoning,
        tool_calls: round_response.tool_calls,
        streamed_tool_outputs: round_response.streamed_tool_outputs,
        model_finish_reason,
        output_items,
        usage: round_response.usage,
        final_answer_streamed,
        execution_summary_streamed: round_response.execution_summary_streamed,
        disposition,
    })
}

pub fn apply_general_turn_step(state: &mut GeneralTurnLoopState, step: &GeneralTurnStepResult) {
    state.final_answer_streamed = state.final_answer_streamed || step.final_answer_streamed;
    state.execution_summary_streamed =
        state.execution_summary_streamed || step.execution_summary_streamed;
    state.usage.accumulate(&step.usage);
    state.output_items.extend(step.output_items.clone());
    state
        .reasoning_log
        .extend(step.reasoning.display_text.clone());
    if !step.tool_calls.is_empty() {
        let mut message =
            ChatMessage::assistant_with_tool_calls(step.text.clone(), step.tool_calls.clone());
        if let Some(continuation) = step.reasoning.continuation.clone() {
            message = message.with_reasoning_continuation(continuation);
        }
        state.messages.push(message);
    }
    for call in &step.tool_calls {
        state.tool_call_count += 1;
        state.tool_call_log.push(ToolCallRecord {
            call_id: call.id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.clone().into(),
        });
    }
}
