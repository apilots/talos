use std::collections::HashMap;
use std::path::Path;

use talos_agent_kernel::{GeneralTurnResume, ResumeToolCall, UserActionResume};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_store::session::{
    SessionArtifactStore, SessionTranscriptBranch, SessionTranscriptPayload,
};
use talos_types::agent_dispatch::AgentDispatchResumeContext;

pub(super) async fn build_general_turn_resume(
    session_artifact_store: &dyn SessionArtifactStore,
    workspace_root: &Path,
    session_id: &str,
    turn_id: &str,
    resume_context: Option<AgentDispatchResumeContext>,
) -> TalosResult<Option<GeneralTurnResume>> {
    let Some(resume_context) = resume_context else {
        return Ok(None);
    };
    let pending_call_id = resume_context.pending_call_id;
    let execution_metrics = resume_context.execution_metrics;
    let Some(tool_call) = resume_context
        .tool_calls
        .into_iter()
        .find(|tool_call| tool_call.call_id == pending_call_id)
    else {
        return Err(TalosError::public(
            ErrorCode::InvalidRequest,
            format!(
                "user action resume context for session `{session_id}` turn `{turn_id}` is missing pending tool call `{pending_call_id}`"
            ),
        ));
    };

    let records = session_artifact_store
        .load_transcript_records(
            workspace_root,
            session_id,
            None,
            Some(SessionTranscriptBranch::ReactiveLoop),
        )
        .await?;
    let mut request = None;
    let mut response = None;
    let mut request_belongs_to_another_turn = false;
    for record in records {
        match record.payload {
            SessionTranscriptPayload::UserActionRequest { ref payload }
                if record.header.invocation_id.as_str() != turn_id
                    && payload.call_id.as_str() == tool_call.call_id.as_str()
                    && payload.tool_name.as_str() == tool_call.tool_name.as_str() =>
            {
                request_belongs_to_another_turn = true;
            }
            SessionTranscriptPayload::UserActionRequest { payload }
                if record.header.invocation_id.as_str() == turn_id
                    && payload.call_id.as_str() == tool_call.call_id.as_str()
                    && payload.tool_name.as_str() == tool_call.tool_name.as_str() =>
            {
                if request
                    .as_ref()
                    .is_none_or(|(timestamp, _)| record.header.timestamp > *timestamp)
                {
                    request = Some((record.header.timestamp, payload));
                }
            }
            SessionTranscriptPayload::UserActionResponse { payload }
                if record.header.invocation_id.as_str() == turn_id
                    && payload.call_id.as_str() == tool_call.call_id.as_str()
                    && payload.tool_name.as_str() == tool_call.tool_name.as_str()
                    && response
                        .as_ref()
                        .is_none_or(|(timestamp, _)| record.header.timestamp > *timestamp) =>
            {
                response = Some((record.header.timestamp, payload));
            }
            _ => {}
        }
    }
    let request = request.map(|(_, payload)| payload).ok_or_else(|| {
        let message = format!(
            "user action call `{}` has no pending request for session `{}` turn `{}`",
            tool_call.call_id, session_id, turn_id
        );
        if request_belongs_to_another_turn {
            TalosError::public(ErrorCode::InvalidRequest, message)
        } else {
            TalosError::internal(message)
        }
    })?;
    let response = response.map(|(_, payload)| payload).ok_or_else(|| {
        TalosError::internal(format!(
            "user action call `{}` has no submitted response for session `{}` turn `{}`",
            tool_call.call_id, session_id, turn_id
        ))
    })?;

    Ok(Some(GeneralTurnResume::UserAction(UserActionResume {
        call_id: tool_call.call_id.clone(),
        tool_name: tool_call.tool_name.clone(),
        response: response.response,
        sandbox_request_fingerprint: request.sandbox_request_fingerprint,
        original_call: Some(ResumeToolCall {
            item_id: Some(tool_call.item_id),
            call_id: tool_call.call_id,
            tool_name: tool_call.tool_name,
            arguments: request.arguments,
            reasoning_continuation: request.reasoning_continuation,
        }),
        tool_calls: request.tool_calls,
        tool_messages: request.tool_messages,
        deferred_tool_calls: request.deferred_tool_calls,
        replay_dynamic_tool_results: HashMap::new(),
        execution_metrics,
    })))
}
