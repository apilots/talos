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
    if tx.query("SELECT event_seq FROM agent_events WHERE session_id=? AND turn_id=? AND json_extract(payload_json,'$.type')='messageCompleted' AND json_extract(payload_json,'$.data.messageId')=? AND json_extract(payload_json,'$.data.role')='user'",&[&turn.session_id,&turn.turn_id,message]).await?.is_empty(){return Err(missing())}
    let rows = tx.query("SELECT automation_origin_json,automation_input_message_id FROM agent_events WHERE session_id=? AND turn_id=? AND sequence=1", &[&turn.session_id, &turn.turn_id]).await?;
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
    tx.execute("UPDATE agent_events SET automation_origin_json=?,automation_input_message_id=? WHERE session_id=? AND turn_id=? AND sequence=1 AND automation_origin_json IS NULL", &[&encode(origin)?,message,&turn.session_id,&turn.turn_id]).await?;
    Ok(())
}
/// Called inside the original admission transaction, before the execution permit
/// is consumed. Stop/edit cannot race an already reserved queue snapshot.
pub(crate) async fn bind_admission_origin(
    tx: &mut dyn StorageTransaction,
    turn: &AgentTurnRef,
    origin: &AutomationTurnOrigin,
    message_id: &str,
) -> TalosResult<()> {
    match origin {
        AutomationTurnOrigin::Loop {
            loop_id,
            config_revision,
            ..
        } => {
            let rows = tx
                .query(
                    "SELECT * FROM session_loops WHERE id=? AND session_id=?",
                    &[loop_id, &turn.session_id],
                )
                .await?;
            let task = rows
                .first()
                .map(loop_from_row)
                .transpose()?
                .ok_or_else(missing)?;
            if task.status != LoopStatus::Active
                || task.config_revision != *config_revision
                || (task.latest_turn_id.is_none()
                    && chrono::Utc::now().timestamp()
                        < task
                            .created_at
                            .saturating_add(talos_types::AUTOMATION_INITIAL_DELAY_SECONDS))
            {
                return Err(conflict());
            }
            tx.execute(
                "UPDATE session_loops SET latest_turn_id=?,next_due_at=NULL WHERE id=?",
                &[&turn.turn_id, loop_id],
            )
            .await?;
        }
        AutomationTurnOrigin::Goal {
            goal_id,
            objective_revision,
        } => {
            let rows = tx
                .query(
                    "SELECT * FROM session_goals WHERE id=? AND session_id=?",
                    &[goal_id, &turn.session_id],
                )
                .await?;
            let task = rows
                .first()
                .map(goal_from_row)
                .transpose()?
                .ok_or_else(missing)?;
            if task.status != GoalStatus::Active
                || task.objective_revision != *objective_revision
                || (task.latest_turn_id.is_none()
                    && chrono::Utc::now().timestamp()
                        < task
                            .created_at
                            .saturating_add(talos_types::AUTOMATION_INITIAL_DELAY_SECONDS))
            {
                return Err(conflict());
            }
            tx.execute(
                "UPDATE session_goals SET latest_turn_id=? WHERE id=?",
                &[&turn.turn_id, goal_id],
            )
            .await?;
        }
    }
    bind_input(tx, turn, origin, message_id).await
}
