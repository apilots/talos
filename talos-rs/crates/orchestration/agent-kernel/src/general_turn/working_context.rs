use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MessageGroup {
    start: usize,
    end: usize,
    complete_tool_exchange: bool,
    unresolved_tool_linkage: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WorkingContextTrimReport {
    pub(super) pre_tokens: usize,
    pub(super) post_tokens: usize,
    pub(super) input_messages: usize,
    pub(super) trimmed_messages: usize,
    pub(super) preserved_recent_exchanges: usize,
}

/// Keeps the active run working context within the model prompt budget before provider calls.
///
/// This guard owns only in-memory loop messages. It must not mutate completed session history,
/// MemoryStore summaries, or public run artifacts.
pub(crate) async fn enforce_working_context_budget(
    prepared: &PreparedGeneralTurn,
    state: &mut GeneralTurnLoopState,
    kernel_config: &AgentKernelConfig,
) -> TalosResult<()> {
    let window_usage = &prepared.prompt_diagnostics.window_usage;
    if !window_usage.window_resolved || window_usage.effective_prompt_budget == 0 {
        return Ok(());
    }

    let pre_tokens = estimate_working_context_tokens(prepared, state, kernel_config);
    if pre_tokens <= window_usage.effective_prompt_budget {
        return Ok(());
    }

    emit_working_context_compaction(
        prepared,
        talos_types::talos::ContextCompactionStage::Trimming,
        None,
        Some(build_working_context_metrics(
            pre_tokens,
            pre_tokens,
            state.messages.len(),
            0,
            0,
        )),
        "Working context exceeded the effective prompt budget; trimming older loop messages.",
    )
    .await;

    let report = trim_working_context_to_budget(
        prepared,
        state,
        window_usage.effective_prompt_budget,
        kernel_config.compaction.tool_output_keep_last.max(1),
        kernel_config,
    )?;
    let outcome = if report.post_tokens <= window_usage.effective_prompt_budget {
        talos_types::talos::ContextCompactionOutcome::TrimmedFallbackApplied
    } else {
        talos_types::talos::ContextCompactionOutcome::FailedContextStillTooLarge
    };
    let stage = if report.post_tokens <= window_usage.effective_prompt_budget {
        talos_types::talos::ContextCompactionStage::Completed
    } else {
        talos_types::talos::ContextCompactionStage::Failed
    };
    emit_working_context_compaction(
        prepared,
        stage,
        Some(outcome),
        Some(build_working_context_metrics(
            report.pre_tokens,
            report.post_tokens,
            report.input_messages,
            report.trimmed_messages,
            report.preserved_recent_exchanges,
        )),
        "Working context budget maintenance finished.",
    )
    .await;

    if report.post_tokens > window_usage.effective_prompt_budget {
        return Err(TalosError::new(talos_error::ErrorCode::ContextWindowExceeded, format!(
            "working context budget remains exhausted before model request: estimated_tokens={}, effective_prompt_budget={}",
            report.post_tokens, window_usage.effective_prompt_budget
        )));
    }

    Ok(())
}

fn combined_messages(
    prepared: &PreparedGeneralTurn,
    state: &GeneralTurnLoopState,
    kernel_config: &AgentKernelConfig,
) -> Vec<ChatMessage> {
    let mut messages = prepared.messages.clone();
    messages.extend(state.messages.clone());
    if let Some(retrieved) = talos_prompt::render_retrieved_memories(
        &state.retrieved_memories,
        kernel_config.compaction.retrieved_memories_keep,
        kernel_config.compaction.retrieved_memory_max_chars,
    ) {
        messages.push(retrieved);
    }
    messages
}

pub(super) fn trim_working_context_to_budget(
    prepared: &PreparedGeneralTurn,
    state: &mut GeneralTurnLoopState,
    effective_prompt_budget: usize,
    preserve_recent_exchanges: usize,
    kernel_config: &AgentKernelConfig,
) -> TalosResult<WorkingContextTrimReport> {
    let input_messages = state.messages.len();
    let pre_tokens = estimate_working_context_tokens(prepared, state, kernel_config);
    let mut preserved_recent_exchanges = 0;

    loop {
        let current_tokens = estimate_working_context_tokens(prepared, state, kernel_config);
        if current_tokens <= effective_prompt_budget {
            return Ok(WorkingContextTrimReport {
                pre_tokens,
                post_tokens: current_tokens,
                input_messages,
                trimmed_messages: input_messages.saturating_sub(state.messages.len()),
                preserved_recent_exchanges,
            });
        }

        let groups = group_working_messages(&state.messages);
        let Some(group_index) = find_removable_group(&groups, preserve_recent_exchanges) else {
            return Ok(WorkingContextTrimReport {
                pre_tokens,
                post_tokens: current_tokens,
                input_messages,
                trimmed_messages: input_messages.saturating_sub(state.messages.len()),
                preserved_recent_exchanges,
            });
        };
        preserved_recent_exchanges =
            count_preserved_recent_exchanges(&groups, preserve_recent_exchanges);
        let group = groups[group_index];
        let mut candidates = Vec::new();
        for message in state.messages[group.start..group.end]
            .iter()
            .filter(|message| matches!(message.name.as_deref(), Some("MemoryGet" | "memory_get")))
        {
            let value =
                serde_json::from_str::<serde_json::Value>(&message.content).map_err(|error| {
                    TalosError::internal_with_source(
                        "parse trimmed MemoryGet tool result; technical details were withheld.",
                        error,
                    )
                })?;
            if value.get("ok").and_then(serde_json::Value::as_bool) == Some(false) {
                continue;
            }
            let item = serde_json::from_value::<talos_types::talos::TalosContentItem>(value)
                .map_err(|error| {
                    TalosError::internal_with_source(
                        "parse trimmed MemoryGet tool result; technical details were withheld.",
                        error,
                    )
                })?;
            let talos_types::talos::TalosContentItem::Json { value } = item else {
                return Err(TalosError::internal(
                    "trimmed MemoryGet tool result is not structured JSON",
                ));
            };
            candidates.extend(super::tool_batch::parse_retrieved_memory_items(
                "memory_get",
                value.as_inner(),
            )?);
        }
        state.messages.drain(group.start..group.end);
        let retrieved = super::tool_batch::take_new_retrieved_memories(
            &state.retrieved_memories,
            &candidates,
            kernel_config.compaction.retrieved_memories_keep,
        );
        state.retrieved_memories.extend(retrieved);
    }
}

fn group_working_messages(messages: &[ChatMessage]) -> Vec<MessageGroup> {
    let mut groups = Vec::new();
    let mut index = 0;
    while index < messages.len() {
        let message = &messages[index];
        if message.role == talos_llm_provider::Role::Assistant {
            let call_ids = message
                .tool_calls
                .as_ref()
                .map(|calls| {
                    calls
                        .iter()
                        .map(|call| call.id.as_str())
                        .collect::<std::collections::HashSet<_>>()
                })
                .unwrap_or_default();
            if !call_ids.is_empty() {
                let mut end = index + 1;
                let mut matched_results = std::collections::HashSet::new();
                while end < messages.len()
                    && messages[end].role == talos_llm_provider::Role::Tool
                    && messages[end]
                        .tool_call_id
                        .as_deref()
                        .is_some_and(|id| call_ids.contains(id))
                {
                    if let Some(id) = messages[end].tool_call_id.as_deref() {
                        matched_results.insert(id);
                    }
                    end += 1;
                }
                groups.push(MessageGroup {
                    start: index,
                    end,
                    complete_tool_exchange: matched_results.len() == call_ids.len(),
                    unresolved_tool_linkage: matched_results.len() != call_ids.len(),
                });
                index = end;
                continue;
            }
        }

        groups.push(MessageGroup {
            start: index,
            end: index + 1,
            complete_tool_exchange: false,
            unresolved_tool_linkage: message.role == talos_llm_provider::Role::Tool,
        });
        index += 1;
    }
    groups
}

fn find_removable_group(
    groups: &[MessageGroup],
    preserve_recent_exchanges: usize,
) -> Option<usize> {
    if groups.len() <= 1 {
        return None;
    }
    let preserved_exchange_indexes =
        recent_complete_exchange_indexes(groups, preserve_recent_exchanges);
    groups.iter().enumerate().find_map(|(index, group)| {
        if group.unresolved_tool_linkage || preserved_exchange_indexes.contains(&index) {
            None
        } else {
            Some(index)
        }
    })
}

fn recent_complete_exchange_indexes(
    groups: &[MessageGroup],
    preserve_recent_exchanges: usize,
) -> std::collections::HashSet<usize> {
    groups
        .iter()
        .enumerate()
        .rev()
        .filter_map(|(index, group)| group.complete_tool_exchange.then_some(index))
        .take(preserve_recent_exchanges)
        .collect()
}

fn count_preserved_recent_exchanges(
    groups: &[MessageGroup],
    preserve_recent_exchanges: usize,
) -> usize {
    recent_complete_exchange_indexes(groups, preserve_recent_exchanges).len()
}

fn estimate_messages_tokens(messages: &[ChatMessage]) -> usize {
    messages.iter().map(estimate_message_tokens).sum()
}

fn estimate_working_context_tokens(
    prepared: &PreparedGeneralTurn,
    state: &GeneralTurnLoopState,
    kernel_config: &AgentKernelConfig,
) -> usize {
    prepared
        .attachment_tokens
        .saturating_add(state.active_turn_attachment_tokens)
        .saturating_add(estimate_messages_tokens(&combined_messages(
            prepared,
            state,
            kernel_config,
        )))
}

fn estimate_message_tokens(message: &ChatMessage) -> usize {
    let mut chars = message.content.chars().count()
        + format!("{:?}", message.role).chars().count()
        + message.name.as_deref().map(str::len).unwrap_or_default()
        + message
            .tool_call_id
            .as_deref()
            .map(str::len)
            .unwrap_or_default();
    if let Some(calls) = message.tool_calls.as_ref() {
        chars += calls
            .iter()
            .map(|call| {
                call.id.len() + call.name.len() + call.arguments.to_string().chars().count()
            })
            .sum::<usize>();
    }
    let image_tokens = if message.role == talos_llm_provider::Role::Tool {
        message
            .image_file_ids
            .len()
            .saturating_mul(talos_llm_provider::MAX_ATTACHMENT_TOKENS)
    } else {
        0
    };
    image_tokens.saturating_add(if chars == 0 { 0 } else { chars.div_ceil(4) + 4 })
}

fn build_working_context_metrics(
    pre_tokens: usize,
    post_tokens: usize,
    input_messages: usize,
    trimmed_messages: usize,
    preserved_recent_exchanges: usize,
) -> talos_types::talos::ContextCompactionMetrics {
    talos_types::talos::ContextCompactionMetrics {
        pre_compaction_total_tokens: saturating_u32(pre_tokens),
        post_compaction_total_tokens: saturating_u32(post_tokens),
        trimmed_context_unit_count: saturating_u32(trimmed_messages),
        working_context_input_messages: Some(saturating_u32(input_messages)),
        working_context_compacted_messages: Some(saturating_u32(trimmed_messages)),
        working_context_preserved_recent_exchanges: Some(saturating_u32(
            preserved_recent_exchanges,
        )),
        ..Default::default()
    }
}

async fn emit_working_context_compaction(
    prepared: &PreparedGeneralTurn,
    stage: talos_types::talos::ContextCompactionStage,
    outcome: Option<talos_types::talos::ContextCompactionOutcome>,
    metrics: Option<talos_types::talos::ContextCompactionMetrics>,
    message: impl Into<String>,
) {
    emit_stream_event(
        prepared.stream_tx.as_ref(),
        AgentKernelEventPayload::ContextCompaction(crate::ContextCompactionStreamData {
            trigger: talos_types::talos::ContextCompactionTrigger::WorkingContextExceeded,
            stage,
            outcome,
            metrics,
            message: Some(message.into()),
        }),
    )
    .await;
}

fn saturating_u32(value: usize) -> u32 {
    value.min(u32::MAX as usize) as u32
}
