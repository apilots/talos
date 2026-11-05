use super::*;
use talos_types::{GoalPauseReason, LoopSchedule, LoopStopReason};
#[derive(Clone, Copy)]
pub(super) enum Write {
    Insert,
    Update,
}
fn text(row: &Row, key: &str) -> TalosResult<String> {
    row.get_string(key)
        .ok_or_else(|| TalosError::internal("Missing stored automation text."))
}
fn integer(row: &Row, key: &str) -> TalosResult<i64> {
    row.get_i64(key)
        .ok_or_else(|| TalosError::internal("Missing stored automation number."))
}
fn unsigned(row: &Row, key: &str) -> TalosResult<u64> {
    integer(row, key)?
        .try_into()
        .map_err(|_| TalosError::internal("Invalid stored automation number."))
}
fn enumeration<T: DeserializeOwned>(row: &Row, key: &str) -> TalosResult<T> {
    serde_json::from_value(serde_json::Value::String(text(row, key)?))
        .map_err(|e| TalosError::internal_with_source("Invalid stored automation enum.", e))
}
fn optional_enum<T: DeserializeOwned>(row: &Row, key: &str) -> TalosResult<Option<T>> {
    row.get_string(key)
        .map(|s| decode(&format!("\"{s}\"")))
        .transpose()
}
pub(super) fn goal_from_row(r: &Row) -> TalosResult<SessionGoal> {
    Ok(SessionGoal {
        id: text(r, "id")?,
        session_id: text(r, "session_id")?,
        objective: text(r, "objective")?,
        objective_revision: unsigned(r, "objective_revision")?,
        status: enumeration(r, "status")?,
        pause_reason: optional_enum::<GoalPauseReason>(r, "pause_reason")?,
        reason: r.get_string("reason"),
        token_budget: r.get_i64("token_budget").map(|v| v as u64),
        tokens_used: unsigned(r, "tokens_used")?,
        latest_turn_id: r.get_string("latest_turn_id"),
        completion_evidence: r.get_string("completion_evidence"),

        created_at: integer(r, "created_at")?,
        updated_at: integer(r, "updated_at")?,
    })
}
pub(super) fn loop_from_row(r: &Row) -> TalosResult<SessionLoop> {
    Ok(SessionLoop {
        id: text(r, "id")?,
        session_id: text(r, "session_id")?,
        prompt: text(r, "prompt")?,
        schedule: match text(r, "schedule_type")?.as_str() {
            "fixedInterval" => LoopSchedule::FixedInterval {
                minutes: unsigned(r, "minutes")?.try_into().map_err(|_| conflict())?,
            },
            "modelDecided" => LoopSchedule::ModelDecided {},
            _ => return Err(conflict()),
        },
        config_revision: unsigned(r, "config_revision")?,
        status: enumeration(r, "status")?,
        stop_reason: optional_enum::<LoopStopReason>(r, "stop_reason")?,
        reason: r.get_string("reason"),
        next_due_at: r.get_i64("next_due_at"),
        latest_turn_id: r.get_string("latest_turn_id"),
        created_at: integer(r, "created_at")?,
        updated_at: integer(r, "updated_at")?,
    })
}
pub(super) async fn write_goal(
    tx: &mut dyn StorageTransaction,
    goal: &SessionGoal,
    mode: Write,
) -> TalosResult<()> {
    let fields = [
        ("id", "id"),
        ("session_id", "sessionId"),
        ("objective", "objective"),
        ("objective_revision", "objectiveRevision"),
        ("status", "status"),
        ("pause_reason", "pauseReason"),
        ("reason", "reason"),
        ("token_budget", "tokenBudget"),
        ("tokens_used", "tokensUsed"),
        ("latest_turn_id", "latestTurnId"),
        ("completion_evidence", "completionEvidence"),
        ("created_at", "createdAt"),
        ("updated_at", "updatedAt"),
    ];
    write(tx, "session_goals", &goal.id, &encode(goal)?, &fields, mode).await
}
pub(super) async fn write_loop(
    tx: &mut dyn StorageTransaction,
    task: &SessionLoop,
    mode: Write,
) -> TalosResult<()> {
    let fields = [
        ("id", "id"),
        ("session_id", "sessionId"),
        ("prompt", "prompt"),
        ("schedule_type", "schedule.type"),
        ("minutes", "schedule.minutes"),
        ("config_revision", "configRevision"),
        ("status", "status"),
        ("stop_reason", "stopReason"),
        ("reason", "reason"),
        ("next_due_at", "nextDueAt"),
        ("latest_turn_id", "latestTurnId"),
        ("created_at", "createdAt"),
        ("updated_at", "updatedAt"),
    ];
    write(tx, "session_loops", &task.id, &encode(task)?, &fields, mode).await
}
async fn write(
    tx: &mut dyn StorageTransaction,
    table: &str,
    id: &str,
    raw: &str,
    fields: &[(&str, &str)],
    mode: Write,
) -> TalosResult<()> {
    let sql = match mode {
        Write::Insert => format!(
            "INSERT INTO {table} ({}) SELECT {} FROM (SELECT ? AS data)",
            fields.iter().map(|(c, _)| *c).collect::<Vec<_>>().join(","),
            fields
                .iter()
                .map(|(_, p)| format!("json_extract(data,'$.{p}')"))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Write::Update => format!(
            "UPDATE {table} SET {} WHERE id=?",
            fields
                .iter()
                .map(|(c, p)| format!("{c}=json_extract(?,'$.{p}')"))
                .collect::<Vec<_>>()
                .join(",")
        ),
    };
    let mut parameters = match mode {
        Write::Insert => vec![raw],
        Write::Update => vec![raw; fields.len()],
    };
    if matches!(mode, Write::Update) {
        parameters.push(id);
    }
    if tx.execute(&sql, &parameters).await? != 1 {
        return Err(conflict());
    }
    Ok(())
}
