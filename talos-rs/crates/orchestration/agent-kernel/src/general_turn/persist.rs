use super::*;
use crate::{
    EventAuthor, ExecutionBranch, InvocationSession, KernelTranscriptEvent,
    KernelTranscriptEventKind, KernelTranscriptPayload, KernelTranscriptService,
};

pub async fn persist_general_turn_artifacts(
    prepared: &PreparedGeneralTurn,
    input: &UserInput,
    executed: &ExecutedTurnArtifacts,
    session_artifact_store: Arc<dyn talos_store::session::SessionArtifactStore>,
    transcript_service: Option<Arc<dyn KernelTranscriptService>>,
    kernel_config: &AgentKernelConfig,
) -> TalosResult<u64> {
    let workspace_root = std::path::Path::new(&kernel_config.workspace_dir);
    let seq = session_artifact_store
        .next_turn_seq(workspace_root, &prepared.session_id)
        .await?;
    let turn_id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let mut items = Vec::with_capacity(executed.output_items.len().saturating_add(1));
    if !input.text.trim().is_empty() || !input.file_ids.is_empty() {
        items.push(RunItem::UserInput {
            text: input.text.clone(),
            file_ids: input.file_ids.clone(),
        });
    }
    items.extend(executed.output_items.clone());
    let talos_turn = talos_store::session::TalosTurn {
        turn_id: turn_id.clone(),
        message_id: prepared.message_id.clone(),
        session_id: prepared.session_id.to_string(),
        seq,
        timestamp: now,
        items,
        usage: executed.usage.clone(),
        stop_reason: to_api_stop_reason(executed.stop_reason),
    };
    session_artifact_store
        .append_turn(workspace_root, &talos_turn)
        .await?;

    let Some(transcript_service) = transcript_service else {
        if !executed.pending_dynamic_requests.is_empty() {
            warn!(
                session_id = %prepared.session_id,
                "pending dynamic transcript was not persisted because no session port is configured"
            );
        }
        return Ok(seq);
    };
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
    Ok(seq)
}
