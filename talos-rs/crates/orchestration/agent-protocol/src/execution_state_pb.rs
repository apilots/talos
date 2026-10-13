//! Closed codecs for causal metadata; business messages retain their original definitions.
use crate::{SessionExecutionState, SessionHistoryCursor};
use agent_pb as pb;
use talos_error::{ErrorCode, TalosError, TalosResult};
pub fn encode_history_cursor(history_cursor: SessionHistoryCursor) -> pb::SessionHistoryCursor {
    pb::SessionHistoryCursor {
        history_generation: history_cursor.history_generation,
        completed_turn_id: history_cursor.completed_turn_id,
        completed_event_sequence: history_cursor.completed_event_sequence,
    }
}
pub fn decode_history_cursor(
    history_cursor: pb::SessionHistoryCursor,
) -> TalosResult<SessionHistoryCursor> {
    if history_cursor
        .completed_turn_id
        .as_ref()
        .is_some_and(|id| id.is_empty())
        || history_cursor.completed_turn_id.is_some()
            != (history_cursor.completed_event_sequence > 0)
    {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Session history cursor is invalid",
        ));
    }
    Ok(SessionHistoryCursor {
        history_generation: history_cursor.history_generation,
        completed_turn_id: history_cursor.completed_turn_id,
        completed_event_sequence: history_cursor.completed_event_sequence,
    })
}
pub fn encode_state(state: SessionExecutionState) -> pb::SessionExecutionState {
    pb::SessionExecutionState {
        history_cursor: Some(encode_history_cursor(state.history_cursor)),
        active_turn: state.active_turn.map(|id| pb::TurnRef {
            session_id: id.session_id,
            turn_id: id.turn_id,
        }),
        coordinator_id: state.coordinator_id,
        handoff_ready: state.handoff_ready,
    }
}
pub fn decode_state(state: pb::SessionExecutionState) -> TalosResult<SessionExecutionState> {
    Ok(SessionExecutionState {
        history_cursor: decode_history_cursor(state.history_cursor.ok_or_else(|| {
            TalosError::new(
                ErrorCode::InvalidRequest,
                "Session execution state has no history cursor",
            )
        })?)?,
        active_turn: state
            .active_turn
            .map(|id| talos_types::AgentTurnRef::new(id.session_id, id.turn_id)),
        coordinator_id: state.coordinator_id,
        handoff_ready: state.handoff_ready,
    })
}
