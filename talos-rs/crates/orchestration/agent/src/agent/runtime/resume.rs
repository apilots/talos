use std::collections::HashMap;
use std::path::Path;

use talos_agent_kernel::{DynamicToolResultResume, GeneralTurnResume, ResumeToolCall};
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
    let Some(tool_call) = resume_context.tool_call else {
        return Err(TalosError::public(ErrorCode::InvalidRequest, format!(
            "dynamic resume context for session `{session_id}` turn `{turn_id}` is missing tool call metadata"
        )));
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
    for record in records {
        match record.payload {
            SessionTranscriptPayload::DynamicToolRequest { payload }
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
            SessionTranscriptPayload::DynamicToolResponse { payload }
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
        TalosError::public(
            ErrorCode::InvalidRequest,
            format!(
                "dynamic tool call `{}` has no pending request for session `{}` turn `{}`",
                tool_call.call_id, session_id, turn_id
            ),
        )
    })?;
    let response = response.map(|(_, payload)| payload).ok_or_else(|| {
        TalosError::public(
            ErrorCode::InvalidRequest,
            format!(
                "dynamic tool call `{}` has no submitted result for session `{}` turn `{}`",
                tool_call.call_id, session_id, turn_id
            ),
        )
    })?;
    let original_arguments = request.arguments;

    Ok(Some(GeneralTurnResume::DynamicToolResult(
        DynamicToolResultResume {
            call_id: tool_call.call_id.clone(),
            tool_name: tool_call.tool_name.clone(),
            output: response.output,
            success: response.success,
            original_call: Some(ResumeToolCall {
                item_id: Some(tool_call.item_id),
                call_id: tool_call.call_id,
                tool_name: tool_call.tool_name,
                arguments: original_arguments,
                reasoning_continuation: request.reasoning_continuation,
            }),
            replay_dynamic_tool_results: HashMap::new(),
        },
    )))
}
