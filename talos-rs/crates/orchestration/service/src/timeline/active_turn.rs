use talos_error::TalosResult;
use talos_store::session::{
    ActiveTurnStatus, ActiveTurnStore, ActiveTurnUpdate, PendingDynamicToolResult,
    PendingUserAction,
};
use talos_types::talos::{AgentEvent, AgentEventPayload, TalosTimelineDelta};

pub(crate) type ActiveTurnState = Option<(ActiveTurnStatus, Option<PendingUserAction>)>;

pub(crate) async fn state_for_event(
    active_turn_store: &dyn ActiveTurnStore,
    event: &AgentEvent,
) -> TalosResult<ActiveTurnState> {
    let session_id = event.session_id.as_str();
    let message_id = event.message_id.as_str();
    let turn_id = event.turn_id.as_str();
    let active_state = match &event.payload {
        AgentEventPayload::TurnCompleted(_) => {
            completed_active_state(active_turn_store, session_id, message_id, Some(turn_id)).await?
        }
        AgentEventPayload::TurnError(_) => Some((ActiveTurnStatus::Failed, None)),
        AgentEventPayload::ItemDelta(payload) => match &payload.delta {
            TalosTimelineDelta::ToolDynamicRequest {
                call_id, tool_name, ..
            } => Some((
                ActiveTurnStatus::WaitingUserAction,
                Some(PendingUserAction::DynamicToolResult(
                    PendingDynamicToolResult {
                        item_id: Some(payload.item_id.clone()),
                        call_id: call_id.clone(),
                        tool_name: tool_name.clone(),
                    },
                )),
            )),
            TalosTimelineDelta::ToolResult { .. } => {
                resolve_pending_item_state(active_turn_store, session_id, turn_id, &payload.item_id)
                    .await?
            }
            _ => running_active_state(active_turn_store, session_id, turn_id).await?,
        },
        AgentEventPayload::ItemCompleted(payload)
            if matches!(
                payload.status,
                talos_types::talos::TalosTimelineItemStatus::Queued
                    | talos_types::talos::TalosTimelineItemStatus::WaitingDynamicInput
            ) =>
        {
            None
        }
        AgentEventPayload::ItemCompleted(payload)
            if matches!(
                payload.status,
                talos_types::talos::TalosTimelineItemStatus::Completed
                    | talos_types::talos::TalosTimelineItemStatus::Failed
                    | talos_types::talos::TalosTimelineItemStatus::Cancelled
            ) =>
        {
            resolve_pending_item_state(active_turn_store, session_id, turn_id, &payload.item_id)
                .await?
        }
        _ => running_active_state(active_turn_store, session_id, turn_id).await?,
    };
    Ok(active_state)
}

pub(crate) async fn resolve_pending_user_action_update(
    active_turn_store: &dyn ActiveTurnStore,
    session_id: &str,
) -> TalosResult<Option<ActiveTurnUpdate>> {
    let Some(active) = active_turn_store.get_active_turn(session_id).await? else {
        return Ok(None);
    };
    if active.status != ActiveTurnStatus::WaitingUserAction {
        return Ok(None);
    }
    Ok(Some(ActiveTurnUpdate {
        session_id: active.session_id,
        turn_id: active.turn_id,
        message_id: active.message_id,
        status: ActiveTurnStatus::Running,
        last_event_id: active.last_event_id,
        pending_user_action: None,
        updated_at: chrono::Utc::now().timestamp_millis(),
    }))
}

async fn completed_active_state(
    active_turn_store: &dyn ActiveTurnStore,
    session_id: &str,
    message_id: &str,
    turn_id: Option<&str>,
) -> TalosResult<ActiveTurnState> {
    let Some(active) = active_turn_store.get_active_turn(session_id).await? else {
        return Ok(Some((ActiveTurnStatus::Completed, None)));
    };
    let same_turn = turn_id.is_some_and(|value| value == active.turn_id);
    let same_message = active
        .message_id
        .as_deref()
        .is_some_and(|value| value == message_id);
    if (same_turn || same_message)
        && active.status == ActiveTurnStatus::WaitingUserAction
        && active.pending_user_action.is_some()
    {
        return Ok(Some((
            ActiveTurnStatus::WaitingUserAction,
            active.pending_user_action,
        )));
    }
    Ok(Some((ActiveTurnStatus::Completed, None)))
}

async fn running_active_state(
    active_turn_store: &dyn ActiveTurnStore,
    session_id: &str,
    turn_id: &str,
) -> TalosResult<ActiveTurnState> {
    let Some(active) = active_turn_store.get_active_turn(session_id).await? else {
        return Ok(Some((ActiveTurnStatus::Running, None)));
    };
    if active.turn_id == turn_id
        && active.status == ActiveTurnStatus::WaitingUserAction
        && active.pending_user_action.is_some()
    {
        return Ok(Some((
            ActiveTurnStatus::WaitingUserAction,
            active.pending_user_action,
        )));
    }
    Ok(Some((ActiveTurnStatus::Running, None)))
}

async fn resolve_pending_item_state(
    active_turn_store: &dyn ActiveTurnStore,
    session_id: &str,
    turn_id: &str,
    item_id: &str,
) -> TalosResult<ActiveTurnState> {
    let Some(active) = active_turn_store.get_active_turn(session_id).await? else {
        return Ok(Some((ActiveTurnStatus::Running, None)));
    };
    if active.turn_id != turn_id || active.status != ActiveTurnStatus::WaitingUserAction {
        return Ok(Some((ActiveTurnStatus::Running, None)));
    }
    let resolves_pending = match active.pending_user_action.as_ref() {
        Some(PendingUserAction::DynamicToolResult(pending)) => pending
            .item_id
            .as_deref()
            .is_none_or(|pending_item_id| pending_item_id == item_id),
        None => false,
    };
    if resolves_pending {
        return Ok(Some((ActiveTurnStatus::Running, None)));
    }
    Ok(Some((
        ActiveTurnStatus::WaitingUserAction,
        active.pending_user_action,
    )))
}
