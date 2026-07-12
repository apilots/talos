use super::*;
use crate::{
    EventAuthor, ExecutionBranch, InvocationSession, KernelTranscriptEvent,
    KernelTranscriptEventKind, KernelTranscriptPayload, KernelTranscriptService,
    MemoryBackedKernelTranscriptService,
};

pub async fn persist_general_turn_artifacts(
    prepared: &PreparedGeneralTurn,
    input: &str,
    executed: &ExecutedTurnArtifacts,
    _llm_provider: Arc<dyn LlmProvider>,
    memory_store: Arc<MemoryStore>,
    _kernel_config: &AgentKernelConfig,
) {
    let seq = match memory_store.next_turn_seq(&prepared.session_id).await {
        Ok(seq) => seq,
        Err(err) => {
            warn!(
                session_id = %prepared.session_id,
                error = %err,
                "failed to allocate next turn seq"
            );
            return;
        }
    };
    let turn_id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let mut items = Vec::with_capacity(executed.output_items.len().saturating_add(1));
    if !input.trim().is_empty() {
        items.push(RunItem::UserInput {
            text: input.to_string(),
        });
    }
    items.extend(executed.output_items.clone());
    let talos_turn = TalosTurn {
        turn_id: turn_id.clone(),
        message_id: prepared.message_id.clone(),
        session_id: prepared.session_id.to_string(),
        seq,
        timestamp: now,
        items,
        usage: executed.usage.clone(),
        stop_reason: to_api_stop_reason(executed.stop_reason),
    };
    if let Err(err) = memory_store.append_turn(&talos_turn).await {
        warn!(
            session_id = %prepared.session_id,
            error = %err,
            "failed to append memory store turn"
        );
    }

    let transcript_service = MemoryBackedKernelTranscriptService::new(memory_store);
    let mut session = InvocationSession {
        session_id: prepared.session_id.clone(),
        ..InvocationSession::default()
    };
    for pending in &executed.pending_dynamic_requests {
        let event = KernelTranscriptEvent::new(
            prepared.session_id.clone(),
            prepared.invocation_id.clone(),
            ExecutionBranch::ReactiveLoop,
            EventAuthor::System,
            KernelTranscriptEventKind::SystemMessage,
            KernelTranscriptPayload::DynamicToolRequest {
                payload: crate::session::KernelTranscriptDynamicToolRequestPayload {
                    call_id: pending.call_id.clone(),
                    tool_name: pending.tool_name.clone(),
                    arguments: pending.arguments.clone().into(),
                    reasoning_continuation: pending.reasoning_continuation.clone(),
                },
            },
        );
        if let Err(err) = transcript_service.append_event(&mut session, event).await {
            warn!(
                session_id = %prepared.session_id,
                call_id = %pending.call_id,
                error = %err,
                "failed to append pending dynamic tool transcript"
            );
        }
    }
}
