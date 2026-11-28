//! Bind exact triggering Input metadata inside the original admission transaction.
use super::*;
pub(super) async fn bind(
    tx: &mut dyn StorageTransaction,
    turn: &AgentTurnRef,
    origin: &AutomationTurnOrigin,
) -> TalosResult<()> {
    let rows=tx.query("SELECT initial_message_id FROM agent_active_turns WHERE session_id=? AND turn_id=? AND state='running'", &[&turn.session_id,&turn.turn_id]).await?;
    let message = rows
        .first()
        .and_then(|r| r.get_string("initial_message_id"))
        .ok_or_else(missing)?;
    bind_input(tx, turn, origin, &message).await
}
async fn bind_input(
    tx: &mut dyn StorageTransaction,
    turn: &AgentTurnRef,
    origin: &AutomationTurnOrigin,
    message: &str,
) -> TalosResult<()> {
    if tx.query("SELECT event_seq FROM visible_agent_events WHERE session_id=? AND turn_id=? AND json_extract(payload_json,'$.type')='messageCompleted' AND json_extract(payload_json,'$.data.messageId')=? AND json_extract(payload_json,'$.data.role')='user'",&[&turn.session_id,&turn.turn_id,message]).await?.is_empty(){return Err(missing())}
    let rows = tx.query("SELECT automation_origin_json,automation_input_message_id FROM visible_agent_events WHERE session_id=? AND turn_id=? AND sequence=1", &[&turn.session_id, &turn.turn_id]).await?;
    let row = rows.first().ok_or_else(missing)?;
    if let Some(raw) = row.get_string("automation_origin_json") {
        return if decode::<AutomationTurnOrigin>(&raw)? == *origin
            && row.get_string("automation_input_message_id").as_deref() == Some(message)
        {
            Ok(())
        } else {
            Err(conflict())
        };
    }
    tx.execute("UPDATE agent_events SET automation_origin_json=?,automation_input_message_id=? WHERE event_seq=(SELECT event_seq FROM visible_agent_events WHERE session_id=? AND turn_id=? AND sequence=1 AND automation_origin_json IS NULL)", &[&encode(origin)?,message,&turn.session_id,&turn.turn_id]).await?;
    Ok(())
}
/// Replay records accepted versions without re-admitting work or undoing newer controls.
pub(crate) async fn bind_replayed_admission_origin(
    tx: &mut dyn StorageTransaction,
    turn: &AgentTurnRef,
    origin: &AutomationTurnOrigin,
    message_id: &str,
) -> TalosResult<()> {
    bind_input(tx, turn, origin, message_id).await?;
    match origin {
        AutomationTurnOrigin::Goal {
            goal_id,
            objective_revision,
        } => {
            tx.execute("UPDATE session_goals SET latest_turn_id=? WHERE id=? AND session_id=? AND objective_revision=?", &[&turn.turn_id,goal_id,&turn.session_id,&objective_revision.to_string()]).await?;
        }
        AutomationTurnOrigin::Loop {
            loop_id,
            config_revision,
            ..
        } => {
            tx.execute("UPDATE session_loops SET latest_turn_id=?,pending_turn_id=?,next_due_at=NULL WHERE id=? AND session_id=? AND config_revision=? AND (pending_turn_id IS NULL OR pending_turn_id=?)", &[&turn.turn_id,&turn.turn_id,loop_id,&turn.session_id,&config_revision.to_string(),&turn.turn_id]).await?;
        }
    }
    Ok(())
}
