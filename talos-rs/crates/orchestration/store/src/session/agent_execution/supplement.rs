//! Optional execution facts on the original durable TurnStarted event.
use crate::session::DatabaseSessionStore;
use talos_agent_api::{
    trajectory::{SessionHistorySupplement, TrajectoryPayload},
    AgentTurnRef,
};
use talos_error::{ErrorCode, TalosError, TalosResult};

#[path = "supplement_merge.rs"]
mod merge;

const MAX_TURN_BYTES: usize = 32 * 1024 * 1024;

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
fn encode(value: &SessionHistorySupplement, limit: usize) -> TalosResult<String> {
    let body = serde_json::to_string(value).map_err(codec)?;
    if body.len() > limit {
        return Err(TalosError::new(
            ErrorCode::SessionBudgetExceeded,
            "Execution history supplement exceeded its size limit.",
        ));
    }
    Ok(body)
}

pub(super) async fn append(
    store: &DatabaseSessionStore,
    incoming: SessionHistorySupplement,
) -> TalosResult<()> {
    encode(&incoming, MAX_TURN_BYTES)?;
    for payload in incoming.payloads.values() {
        let bytes = serde_json::to_vec(payload).map_err(codec)?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(TalosError::new(
                ErrorCode::SessionBudgetExceeded,
                "Execution history content exceeded its size limit.",
            ));
        }
    }
    // The original invocation deadline bounds this single normal admission.
    // Pending progress writes must finish rather than silently discard this Turn.
    let mut tx = store.storage.begin_transaction().await?;
    let id = &incoming.identity;
    let rows = tx.query("SELECT event_seq, execution_capture_json FROM agent_events WHERE session_id=? AND turn_id=? AND sequence=1 AND json_extract(payload_json,'$.type')='turnStarted' AND EXISTS (SELECT 1 FROM sessions WHERE session_key=agent_events.session_id)", &[&id.session_id,&id.turn_id]).await?;
    let row = rows.first().ok_or_else(missing)?;
    if !tx.query("SELECT 1 FROM agent_events WHERE session_id=? AND turn_id=? AND json_extract(payload_json,'$.type') IN ('turnCompleted','turnFailed') LIMIT 1", &[&id.session_id,&id.turn_id]).await?.is_empty() {
        return Err(TalosError::new(ErrorCode::Conflict, "Completed Turn execution facts are immutable."));
    }
    let previous = row.get_string("execution_capture_json");
    let merged = match previous.as_deref() {
        Some(body) => merge::append(serde_json::from_str(body).map_err(codec)?, incoming)?,
        None => merge::append_empty(incoming)?,
    };
    let body = encode(&merged, MAX_TURN_BYTES)?;
    if previous.as_deref() != Some(&body) {
        tx.execute(
            "UPDATE agent_events SET execution_capture_json=? WHERE event_seq=?",
            &[
                &body,
                &row.get_i64("event_seq").ok_or_else(missing)?.to_string(),
            ],
        )
        .await?;
    }
    tx.commit().await
}

pub(super) async fn read_terminal(
    store: &DatabaseSessionStore,
    id: &AgentTurnRef,
) -> TalosResult<Option<SessionHistorySupplement>> {
    let rows = store.storage.query("SELECT CASE WHEN execution_capture_json IS NULL THEN NULL ELSE json_set(execution_capture_json,'$.payloads',json('{}')) END AS execution_capture_json FROM agent_events WHERE session_id=? AND turn_id=? AND sequence=1 AND json_extract(payload_json,'$.type')='turnStarted' AND EXISTS (SELECT 1 FROM sessions WHERE session_key=agent_events.session_id) AND EXISTS (SELECT 1 FROM agent_events terminal WHERE terminal.session_id=agent_events.session_id AND terminal.turn_id=agent_events.turn_id AND json_extract(terminal.payload_json,'$.type') IN ('turnCompleted','turnFailed'))", &[&id.session_id,&id.turn_id]).await?;
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
    let rows=store.storage.query("SELECT payload.value AS body FROM agent_events, json_each(agent_events.execution_capture_json,'$.payloads') AS payload WHERE agent_events.session_id=? AND agent_events.turn_id=? AND agent_events.sequence=1 AND json_extract(agent_events.payload_json,'$.type')='turnStarted' AND payload.key=? AND EXISTS (SELECT 1 FROM sessions WHERE session_key=agent_events.session_id) AND EXISTS (SELECT 1 FROM agent_events terminal WHERE terminal.session_id=agent_events.session_id AND terminal.turn_id=agent_events.turn_id AND json_extract(terminal.payload_json,'$.type') IN ('turnCompleted','turnFailed'))",&[&id.session_id,&id.turn_id,payload_id]).await?;
    let body = rows.first().and_then(|row| row.get_string("body"));
    if let Some(body) = &body {
        // Validate against the shared domain type; keep the saved text for paging.
        serde_json::from_str::<TrajectoryPayload>(body).map_err(codec)?;
    }
    Ok(body)
}
