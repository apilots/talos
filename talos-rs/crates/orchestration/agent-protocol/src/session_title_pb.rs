//! Strict typed projection for Worker title execution.
use crate::{
    error_pb, execution_events_pb, harness_pb, AgentSessionTitleMode, AgentSessionTitleOutcome,
    AgentSessionTitleParams, AgentSessionTitleResult,
};
use agent_pb as pb;
use talos_error::{ErrorCode, TalosError, TalosResult};
fn invalid() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "Session title execution data is invalid.",
    )
}
pub fn encode_params(value: AgentSessionTitleParams) -> pb::AgentSessionTitleParams {
    pb::AgentSessionTitleParams {
        mode: match value.mode {
            AgentSessionTitleMode::Generate => pb::AgentSessionTitleMode::Generate as i32,
            AgentSessionTitleMode::ReadRetained => pb::AgentSessionTitleMode::ReadRetained as i32,
        },
        session: Some(harness_pb::encode_session_context(value.session)),
        identity: Some(harness_pb::encode_turn_ref(value.identity)),
    }
}
pub fn decode_params(
    value: Option<pb::AgentSessionTitleParams>,
    config: &talos_config::Config,
) -> TalosResult<AgentSessionTitleParams> {
    let value = value.ok_or_else(invalid)?;
    let session = harness_pb::decode_session_context(value.session, config)?;
    let identity = harness_pb::decode_turn_ref(value.identity)?;
    if identity.session_id != session.session_id {
        return Err(invalid());
    }
    Ok(AgentSessionTitleParams {
        session,
        identity,
        mode: match pb::AgentSessionTitleMode::try_from(value.mode) {
            Ok(pb::AgentSessionTitleMode::Generate) => AgentSessionTitleMode::Generate,
            Ok(pb::AgentSessionTitleMode::ReadRetained) => AgentSessionTitleMode::ReadRetained,
            _ => return Err(invalid()),
        },
    })
}
pub fn encode_result(value: AgentSessionTitleResult) -> pb::AgentSessionTitleResult {
    pb::AgentSessionTitleResult {
        outcome: Some(match value.outcome {
            AgentSessionTitleOutcome::Generated { title } => {
                pb::agent_session_title_result::Outcome::Title(title)
            }
            AgentSessionTitleOutcome::Failed { error } => {
                pb::agent_session_title_result::Outcome::Failure(error_pb::encode_failure(
                    &TalosError::from(error),
                ))
            }
        }),
        model_usage: value
            .model_usage
            .map(execution_events_pb::encode_memory_usage),
    }
}
pub fn decode_result(value: pb::AgentSessionTitleResult) -> TalosResult<AgentSessionTitleResult> {
    let outcome = match value.outcome.ok_or_else(invalid)? {
        pb::agent_session_title_result::Outcome::Title(title) => {
            if talos_types::normalize_session_title(&title).as_ref() != Some(&title) {
                return Err(invalid());
            }
            AgentSessionTitleOutcome::Generated { title }
        }
        pb::agent_session_title_result::Outcome::Failure(failure) => {
            AgentSessionTitleOutcome::Failed {
                error: error_pb::decode_failure(failure)?.to_api_error(),
            }
        }
    };
    if matches!(outcome, AgentSessionTitleOutcome::Generated { .. }) && value.model_usage.is_none()
    {
        return Err(invalid());
    }
    Ok(AgentSessionTitleResult {
        outcome,
        model_usage: value
            .model_usage
            .map(execution_events_pb::decode_memory_usage)
            .transpose()?,
    })
}

#[cfg(test)]
#[path = "session_title_pb_tests.rs"]
mod tests;
