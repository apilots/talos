use talos_store::session::{ActiveTurnRecord, ActiveTurnStatus, PendingUserAction};
use talos_types::{
    TimelineActiveTurnSnapshot, TimelineActiveTurnStatus, TimelinePendingDynamicToolResult,
    TimelinePendingUserAction,
};

pub(crate) fn map_active_turn_snapshot(
    session_id: &str,
    active_turn: Option<ActiveTurnRecord>,
) -> TimelineActiveTurnSnapshot {
    match active_turn {
        Some(record) => TimelineActiveTurnSnapshot {
            session_id: record.session_id,
            turn_id: Some(record.turn_id),
            message_id: record.message_id,
            status: map_active_turn_status(record.status),
            last_event_id: record.last_event_id.map(|value| value.to_string()),
            pending_user_action: record.pending_user_action.map(map_pending_user_action),
            updated_at: record.updated_at,
        },
        None => TimelineActiveTurnSnapshot {
            session_id: session_id.to_string(),
            turn_id: None,
            message_id: None,
            status: TimelineActiveTurnStatus::Completed,
            last_event_id: None,
            pending_user_action: None,
            updated_at: chrono::Utc::now().timestamp_millis(),
        },
    }
}

fn map_active_turn_status(status: ActiveTurnStatus) -> TimelineActiveTurnStatus {
    match status {
        ActiveTurnStatus::Running => TimelineActiveTurnStatus::Running,
        ActiveTurnStatus::WaitingUserAction => TimelineActiveTurnStatus::WaitingUserAction,
        ActiveTurnStatus::Cancelling => TimelineActiveTurnStatus::Cancelling,
        ActiveTurnStatus::Completed => TimelineActiveTurnStatus::Completed,
        ActiveTurnStatus::Failed => TimelineActiveTurnStatus::Failed,
    }
}

fn map_pending_user_action(action: PendingUserAction) -> TimelinePendingUserAction {
    match action {
        PendingUserAction::DynamicToolResult(pending) => {
            TimelinePendingUserAction::DynamicToolResult(TimelinePendingDynamicToolResult {
                item_id: pending.item_id,
                call_id: pending.call_id,
                tool_name: pending.tool_name,
            })
        }
    }
}
