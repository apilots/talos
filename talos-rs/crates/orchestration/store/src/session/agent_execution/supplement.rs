//! Read-only legacy and projected trajectory metadata.
use crate::session::DatabaseSessionStore;
use talos_agent_api::{
    trajectory::{SessionHistorySupplement, TrajectoryPayload},
    AgentTurnRef,
};
use talos_error::{ErrorCode, TalosError, TalosResult};

fn codec(error: serde_json::Error) -> TalosError {
    TalosError::internal_with_source(
        "Decode execution history supplement; technical details were withheld.",
        error,
    )
}
fn missing() -> TalosError {
    TalosError::new(
        ErrorCode::ResourceNotFound,
        "The requested completed Turn was not found.",
    )
}
pub(super) async fn read_terminal(
    store: &DatabaseSessionStore,
    id: &AgentTurnRef,
) -> TalosResult<Option<SessionHistorySupplement>> {
    let rows = store.storage.query("SELECT CASE WHEN e.execution_capture_json IS NULL THEN NULL ELSE json_set(e.execution_capture_json,'$.payloads',json('{}')) END AS execution_capture_json
         FROM visible_agent_events e WHERE e.session_id=? AND e.turn_id=? AND e.sequence=1 AND json_extract(e.payload_json,'$.type')='turnStarted'
         AND EXISTS (SELECT 1 FROM visible_agent_events terminal WHERE terminal.session_id=e.session_id AND terminal.turn_id=e.turn_id AND json_extract(terminal.payload_json,'$.type') IN ('turnCompleted','turnFailed'))", &[&id.session_id,&id.turn_id]).await?;
    let row = rows.first().ok_or_else(missing)?;
    row.get_string("execution_capture_json")
        .map(|body| {
            let value: SessionHistorySupplement = serde_json::from_str(&body).map_err(codec)?;
            if &value.identity != id {
                return Err(TalosError::internal(
                    "Stored execution history belongs to another Turn.",
                ));
            }
            Ok(value)
        })
        .transpose()
}

pub(super) async fn read_payload(
    store: &DatabaseSessionStore,
    id: &AgentTurnRef,
    payload_id: &str,
) -> TalosResult<Option<String>> {
    let rows = store.storage.query("SELECT payload.value AS body FROM visible_agent_events e, json_each(e.execution_capture_json,'$.payloads') AS payload
         WHERE e.session_id=? AND e.turn_id=? AND e.sequence=1 AND json_extract(e.payload_json,'$.type')='turnStarted' AND payload.key=?
         AND EXISTS (SELECT 1 FROM visible_agent_events terminal WHERE terminal.session_id=e.session_id AND terminal.turn_id=e.turn_id AND json_extract(terminal.payload_json,'$.type') IN ('turnCompleted','turnFailed'))",&[&id.session_id,&id.turn_id,payload_id]).await?;
    let body = rows.first().and_then(|row| row.get_string("body"));
    if let Some(body) = &body {
        // Validate against the shared domain type; keep the saved text for paging.
        serde_json::from_str::<TrajectoryPayload>(body).map_err(codec)?;
    }
    Ok(body)
}
