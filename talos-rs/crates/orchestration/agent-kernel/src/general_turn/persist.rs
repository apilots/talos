use super::*;
use crate::{
    EventAuthor, ExecutionBranch, InvocationSession, KernelTranscriptEvent,
    KernelTranscriptEventKind, KernelTranscriptPayload, KernelTranscriptService,
};

pub(crate) async fn stage_session_state_update(
    ctx: &mut InvocationContext,
    session_id: &str,
    invocation_id: &str,
    branch: ExecutionBranch,
    key: &str,
    value: serde_json::Value,
) -> TalosResult<()> {
    let mut event = KernelTranscriptEvent::new(
        session_id.to_string(),
        invocation_id.to_string(),
        branch,
        EventAuthor::System,
        KernelTranscriptEventKind::Planning,
        KernelTranscriptPayload::StateOnly,
    );
    event.actions.state_delta.push(crate::StateDelta {
        namespace: crate::StateNamespace::Session,
        key: key.to_string(),
        value,
    });
    if let Some(transcript_service) = ctx.transcript_service.clone() {
        transcript_service
            .append_event(&mut ctx.session, event)
            .await?;
    } else {
        stage_kernel_transcript_event(ctx, event);
    }
    Ok(())
}

fn stage_kernel_transcript_event(ctx: &mut InvocationContext, event: KernelTranscriptEvent) {
    for delta in &event.actions.state_delta {
        ctx.session.state.apply_delta(delta);
    }
    ctx.session.events.push(event);
}

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
        if executed.pending_user_action.is_some() {
            warn!(
                session_id = %prepared.session_id,
                "pending user action transcript was not persisted because no session port is configured"
            );
        }
        return Ok(seq);
    };
    let mut session = InvocationSession {
        session_id: prepared.session_id.clone(),
        ..InvocationSession::default()
    };
    if let Some(pending) = &executed.pending_user_action {
        let event = KernelTranscriptEvent::new(
            prepared.session_id.clone(),
            prepared.invocation_id.clone(),
            ExecutionBranch::ReactiveLoop,
            EventAuthor::System,
            KernelTranscriptEventKind::SystemMessage,
            KernelTranscriptPayload::UserActionRequest {
                payload: crate::session::KernelTranscriptUserActionRequestPayload {
                    action_id: pending.action_id.clone(),
                    call_id: pending.call_id.clone(),
                    tool_name: pending.tool_name.clone(),
                    arguments: pending.arguments.clone().into(),
                    params: pending.params.clone(),
                    sandbox_request_fingerprint: pending.sandbox_request_fingerprint.clone(),
                    reasoning_continuation: pending.reasoning_continuation.clone(),
                    tool_calls: pending.tool_calls.clone(),
                    tool_messages: pending.tool_messages.clone(),
                    deferred_tool_calls: pending.deferred_tool_calls.clone(),
                },
            },
        );
        if let Err(err) = transcript_service.append_event(&mut session, event).await {
            warn!(
                session_id = %prepared.session_id,
                call_id = %pending.call_id,
                error = %err,
                "failed to append pending user action transcript"
            );
        }
    }
    Ok(seq)
}
