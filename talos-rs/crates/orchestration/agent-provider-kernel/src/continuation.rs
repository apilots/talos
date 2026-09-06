use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use talos_agent::AgentResult;
use talos_agent_api::AgentContinuationRef;
use talos_agent_kernel::{
    AgentKernelContinuation, AgentKernelContinuationToolCall, AgentKernelUserActionContinuation,
    EventAuthor, ExecutionBranch, KernelSessionArtifactStore, KernelTranscriptEvent,
    KernelTranscriptPayload, KernelTranscriptUserActionResponsePayload,
};
use talos_error::TalosError;
use talos_types::{TurnExecutionMetrics, UserActionResponse};

const CONTINUATION_VERSION: u8 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct KernelProviderContinuation {
    version: u8,
    pub(crate) action_id: String,
    pub(crate) call_id: String,
    pub(crate) tool_name: String,
    pub(crate) message_id: String,
    pub(crate) block_id: String,
    pub(crate) execution_metrics: TurnExecutionMetrics,
}

impl KernelProviderContinuation {
    pub(crate) fn new(
        action_id: String,
        call_id: String,
        tool_name: String,
        message_id: String,
        block_id: String,
        execution_metrics: TurnExecutionMetrics,
    ) -> Self {
        Self {
            version: CONTINUATION_VERSION,
            action_id,
            call_id,
            tool_name,
            message_id,
            block_id,
            execution_metrics,
        }
    }

    pub(crate) fn encode(&self) -> AgentResult<AgentContinuationRef> {
        serde_json::to_string(self)
            .map(|opaque_id| AgentContinuationRef { opaque_id })
            .map_err(|error| {
                TalosError::from_source(
                    talos_error::ErrorCode::InternalError,
                    "serialize Kernel continuation reference",
                    error,
                )
            })
    }

    pub(crate) fn decode(reference: &AgentContinuationRef) -> AgentResult<Self> {
        let decoded: Self = serde_json::from_str(&reference.opaque_id).map_err(|error| {
            TalosError::from_source(
                talos_error::ErrorCode::InvalidRequest,
                "Kernel continuation reference is invalid",
                error,
            )
        })?;
        if decoded.version != CONTINUATION_VERSION {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Kernel continuation reference version is unsupported",
            ));
        }
        Ok(decoded)
    }
}

pub(crate) async fn build_kernel_continuation(
    store: &dyn KernelSessionArtifactStore,
    workspace_root: &Path,
    session_id: &str,
    turn_id: &str,
    reference: &KernelProviderContinuation,
    action_id: &str,
    response: UserActionResponse,
) -> AgentResult<AgentKernelContinuation> {
    if reference.action_id != action_id {
        return Err(TalosError::new(
            talos_error::ErrorCode::Conflict,
            "UserAction response does not match Kernel continuation",
        ));
    }
    let records = store
        .load_transcript_events(
            workspace_root,
            session_id,
            None,
            Some(ExecutionBranch::ReactiveLoop),
        )
        .await?;
    let request = records
        .into_iter()
        .filter_map(|record| match record.payload {
            KernelTranscriptPayload::UserActionRequest { payload }
                if record.invocation_id == turn_id
                    && payload.call_id == reference.call_id
                    && payload.tool_name == reference.tool_name =>
            {
                Some((record.timestamp, payload))
            }
            _ => None,
        })
        .max_by_key(|(timestamp, _)| *timestamp)
        .map(|(_, payload)| payload)
        .ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::ResourceNotFound,
                "Kernel continuation request was not found",
            )
        })?;

    store
        .append_transcript_event(
            workspace_root,
            &KernelTranscriptEvent::new(
                session_id,
                turn_id,
                ExecutionBranch::ReactiveLoop,
                EventAuthor::System,
                KernelTranscriptPayload::UserActionResponse {
                    payload: KernelTranscriptUserActionResponsePayload {
                        call_id: reference.call_id.clone(),
                        tool_name: reference.tool_name.clone(),
                        response: response.clone(),
                    },
                },
            ),
        )
        .await?;

    Ok(AgentKernelContinuation::UserAction(
        AgentKernelUserActionContinuation {
            call_id: reference.call_id.clone(),
            tool_name: reference.tool_name.clone(),
            response,
            sandbox_request_fingerprint: request.sandbox_request_fingerprint,
            original_call: Some(AgentKernelContinuationToolCall {
                item_id: None,
                call_id: reference.call_id.clone(),
                tool_name: reference.tool_name.clone(),
                arguments: request.arguments,
                reasoning_continuation: request.reasoning_continuation,
            }),
            tool_calls: request.tool_calls,
            tool_messages: request.tool_messages,
            deferred_tool_calls: request.deferred_tool_calls,
            replay_dynamic_tool_results: HashMap::new(),
            execution_metrics: reference.execution_metrics.clone(),
        },
    ))
}

#[cfg(test)]
#[path = "continuation_tests.rs"]
mod tests;
