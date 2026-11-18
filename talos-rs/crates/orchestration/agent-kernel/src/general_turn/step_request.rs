use super::*;
use talos_llm_provider::{MessageInputKind, MessageInputOrigin, MessageInputSource, Role};

pub fn build_general_turn_step_request(
    prepared: &PreparedGeneralTurn,
    state: &GeneralTurnLoopState,
    tool_definitions: Vec<ToolDefinition>,
    parallel_tool_calls: bool,
    retrieved_memory_max_items: usize,
    retrieved_memory_max_chars: usize,
) -> GeneralTurnStepRequest {
    let mut messages = prepared.messages.clone();
    messages.extend(state.messages.clone());
    if let Some(retrieved) = talos_prompt::render_retrieved_memories(
        &state.retrieved_memories,
        retrieved_memory_max_items,
        retrieved_memory_max_chars,
    ) {
        messages.push(retrieved);
    }

    // Give a long tool-driven turn another chance to report meaningful progress.
    if state.started_at.elapsed().as_secs() >= 30
        && state
            .messages
            .last()
            .is_some_and(|message| message.role == Role::Tool)
        && !state
            .messages
            .iter()
            .any(|message| message.role == Role::Assistant && !message.content.trim().is_empty())
    {
        messages.push(
            ChatMessage::user("If more tool calls are needed, first send one brief user-visible progress update in the user's language. Summarize a verified finding or explain what changed in the investigation using the tool results. Then continue with the next tool call in the same reply. If the investigation is complete, give the final answer.")
                .with_input_origin(MessageInputOrigin {
                    kind: MessageInputKind::Context,
                    source: MessageInputSource::Runtime,
                }),
        );
        let window = &prepared.prompt_diagnostics.window_usage;
        if window.window_resolved
            && window.effective_prompt_budget > 0
            && super::working_context::estimate_messages_tokens(&messages)
                .saturating_add(prepared.attachment_tokens)
                .saturating_add(state.active_turn_attachment_tokens)
                > window.effective_prompt_budget
        {
            messages.pop();
        }
    }

    GeneralTurnStepRequest {
        trajectory: prepared.trajectory.clone(),
        round: state.round,
        call_mode: KernelModelCallMode::WorkingLoop,
        model: prepared.selected_model.clone(),
        max_tokens: prepared.max_output_tokens,
        messages,
        tool_definitions,
        parallel_tool_calls,
        reasoning_effort: prepared.reasoning_effort,
        speed: prepared.speed,
        pre_final_execution_summary: None,
        stream_tx: prepared.stream_tx.clone(),
        streaming_tool_execution: None,
    }
}
