//! Typed projection of Worker-owned Dream execution and product consumption positions.

use crate::{dream::*, error_pb, execution_events_pb, harness_pb};
use agent_pb as pb;
use talos_error::{ErrorCode, TalosError, TalosResult};

fn invalid() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "Dream Memory execution data is invalid.",
    )
}
fn encode_cursor(value: DreamMemoryCursor) -> pb::DreamMemoryCursor {
    pb::DreamMemoryCursor {
        generation: value.generation,
        offset: value.offset,
    }
}
fn decode_cursor(value: pb::DreamMemoryCursor) -> TalosResult<DreamMemoryCursor> {
    if value.offset < 0 {
        return Err(invalid());
    }
    Ok(DreamMemoryCursor {
        generation: value.generation,
        offset: value.offset,
    })
}
pub fn encode_params(value: DreamMemoryParams) -> pb::DreamMemoryParams {
    pb::DreamMemoryParams {
        context: Some(harness_pb::encode_session_context(value.session)),
        after_cursor: value.after_cursor.map(encode_cursor),
    }
}
pub fn decode_params(
    value: Option<pb::DreamMemoryParams>,
    config: &talos_config::Config,
) -> TalosResult<DreamMemoryParams> {
    let value = value.ok_or_else(invalid)?;
    Ok(DreamMemoryParams {
        session: harness_pb::decode_session_context(value.context, config)?,
        after_cursor: value.after_cursor.map(decode_cursor).transpose()?,
    })
}
pub fn encode_result(value: DreamMemoryResult) -> pb::DreamMemoryResult {
    pb::DreamMemoryResult {
        formation: value.formation.map(|formation| pb::DreamMemoryFormation {
            covers_until_seq: formation.covers_until_seq,
            model_usage: Some(execution_events_pb::encode_memory_usage(
                formation.model_usage,
            )),
            failure: formation
                .failure
                .map(|error| error_pb::encode_failure(&TalosError::from(error))),
        }),
        next_cursor: Some(encode_cursor(value.next_cursor)),
        processed_until_seq: value.processed_until_seq,
    }
}
pub fn decode_result(value: pb::DreamMemoryResult) -> TalosResult<DreamMemoryResult> {
    Ok(DreamMemoryResult {
        formation: value
            .formation
            .map(|formation| -> TalosResult<DreamMemoryFormation> {
                Ok(DreamMemoryFormation {
                    covers_until_seq: formation.covers_until_seq,
                    model_usage: execution_events_pb::decode_memory_usage(
                        formation.model_usage.ok_or_else(invalid)?,
                    )?,
                    failure: formation
                        .failure
                        .map(error_pb::decode_failure)
                        .transpose()?
                        .map(|error| error.to_api_error()),
                })
            })
            .transpose()?,
        next_cursor: decode_cursor(value.next_cursor.ok_or_else(invalid)?)?,
        processed_until_seq: value.processed_until_seq,
    })
}

#[cfg(test)]
#[path = "dream_pb_tests.rs"]
mod tests;
