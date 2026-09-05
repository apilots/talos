use super::*;
use crate::{
    EventAuthor, ExecutionBranch, InvocationSession, KernelTranscriptEvent,
    KernelTranscriptPayload, KernelTranscriptService,
};

pub(crate) async fn stage_session_state_update(
    ctx: &mut InvocationContext,
    session_id: &str,
    invocation_id: &str,
    branch: ExecutionBranch,
    update: crate::SessionStateUpdate,
) -> TalosResult<()> {
    let event = KernelTranscriptEvent::new(
        session_id.to_string(),
        invocation_id.to_string(),
        branch,
        EventAuthor::System,
        KernelTranscriptPayload::SessionState { update },
    );
    ctx.transcript_service
        .clone()
        .append_event(&mut ctx.session, event)
        .await?;
    Ok(())
}

pub async fn persist_general_turn_state(
    prepared: &PreparedGeneralTurn,
    executed: &ExecutedTurnArtifacts,
    transcript_service: Arc<dyn KernelTranscriptService>,
) -> TalosResult<()> {
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
    Ok(())
}
