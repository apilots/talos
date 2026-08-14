use talos_error::TalosResult;
use talos_store::session::{
    ActiveTurnRecord, ActiveTurnStatus, ActiveTurnStore, ActiveTurnUpdate, UserActionRecord,
    UserActionStore,
};
use talos_types::talos::{AgentEvent, AgentEventPayload, TalosTimelineDelta};
use talos_types::{TimelineUserActionEvent, UserActionState};

pub(crate) struct ActiveTurnState {
    pub(crate) status: ActiveTurnStatus,
    pub(crate) current_action_id: Option<String>,
    pub(crate) new_action: Option<UserActionRecord>,
}

impl ActiveTurnState {
    fn running() -> Self {
        Self {
            status: ActiveTurnStatus::Running,
            current_action_id: None,
            new_action: None,
        }
    }

    fn terminal(status: ActiveTurnStatus) -> Self {
        Self {
            status,
            current_action_id: None,
            new_action: None,
        }
    }

    fn waiting(active: &ActiveTurnRecord) -> Self {
        Self {
            status: ActiveTurnStatus::WaitingUserAction,
            current_action_id: active.current_action_id.clone(),
            new_action: None,
        }
    }
}

pub(crate) async fn state_for_event(
    active_turn_store: &dyn ActiveTurnStore,
    user_action_store: &dyn UserActionStore,
    event: &AgentEvent,
) -> TalosResult<Option<ActiveTurnState>> {
    let session_id = event.session_id.as_str();
    let message_id = event.message_id.as_str();
    let turn_id = event.turn_id.as_str();
    let active_state = match &event.payload {
        AgentEventPayload::TurnCompleted(_) => {
            completed_active_state(active_turn_store, session_id, message_id, Some(turn_id)).await?
        }
        AgentEventPayload::TurnError(_) => {
            Some(ActiveTurnState::terminal(ActiveTurnStatus::Failed))
        }
        AgentEventPayload::ItemDelta(payload) => match &payload.delta {
            TalosTimelineDelta::UserAction {
                event: TimelineUserActionEvent::Requested { action },
            } => {
                if action.state != UserActionState::Pending
                    || action.item_id.as_deref() != Some(payload.item_id.as_str())
                {
                    return Err(talos_error::TalosError::ValidationError(
                        "requested user action must be pending and bound to its timeline item"
                            .to_string(),
                    ));
                }
                Some(ActiveTurnState {
                    status: ActiveTurnStatus::WaitingUserAction,
                    current_action_id: Some(action.action_id.clone()),
                    new_action: Some(UserActionRecord {
                        session_id: event.session_id.clone(),
                        turn_id: event.turn_id.clone(),
                        action: action.clone(),
                        response: None,
                        version: 0,
                        updated_at: event
                            .created_at_ms
                            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis()),
                    }),
                })
            }
            TalosTimelineDelta::UserAction {
                event: TimelineUserActionEvent::Finished { .. },
            } => running_active_state(active_turn_store, session_id, turn_id).await?,
            TalosTimelineDelta::ToolResult { .. } => {
                resolve_pending_item_state(
                    active_turn_store,
                    user_action_store,
                    session_id,
                    turn_id,
                    &payload.item_id,
                )
                .await?
            }
            _ => running_active_state(active_turn_store, session_id, turn_id).await?,
        },
        AgentEventPayload::ItemCompleted(payload)
            if matches!(
                payload.status,
                talos_types::talos::TalosTimelineItemStatus::Queued
                    | talos_types::talos::TalosTimelineItemStatus::WaitingUserAction
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
            resolve_pending_item_state(
                active_turn_store,
                user_action_store,
                session_id,
                turn_id,
                &payload.item_id,
            )
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
        current_action_id: None,
        updated_at: chrono::Utc::now().timestamp_millis(),
    }))
}

async fn completed_active_state(
    active_turn_store: &dyn ActiveTurnStore,
    session_id: &str,
    message_id: &str,
    turn_id: Option<&str>,
) -> TalosResult<Option<ActiveTurnState>> {
    let Some(active) = active_turn_store.get_active_turn(session_id).await? else {
        return Ok(Some(ActiveTurnState::terminal(ActiveTurnStatus::Completed)));
    };
    let same_turn = turn_id.is_some_and(|value| value == active.turn_id);
    let same_message = active
        .message_id
        .as_deref()
        .is_some_and(|value| value == message_id);
    if (same_turn || same_message)
        && active.status == ActiveTurnStatus::WaitingUserAction
        && active.current_action_id.is_some()
    {
        return Ok(Some(ActiveTurnState::waiting(&active)));
    }
    Ok(Some(ActiveTurnState::terminal(ActiveTurnStatus::Completed)))
}

async fn running_active_state(
    active_turn_store: &dyn ActiveTurnStore,
    session_id: &str,
    turn_id: &str,
) -> TalosResult<Option<ActiveTurnState>> {
    let Some(active) = active_turn_store.get_active_turn(session_id).await? else {
        return Ok(Some(ActiveTurnState::running()));
    };
    if active.turn_id == turn_id
        && active.status == ActiveTurnStatus::WaitingUserAction
        && active.current_action_id.is_some()
    {
        return Ok(Some(ActiveTurnState::waiting(&active)));
    }
    Ok(Some(ActiveTurnState::running()))
}

async fn resolve_pending_item_state(
    active_turn_store: &dyn ActiveTurnStore,
    user_action_store: &dyn UserActionStore,
    session_id: &str,
    turn_id: &str,
    item_id: &str,
) -> TalosResult<Option<ActiveTurnState>> {
    let Some(active) = active_turn_store.get_active_turn(session_id).await? else {
        return Ok(Some(ActiveTurnState::running()));
    };
    if active.turn_id != turn_id || active.status != ActiveTurnStatus::WaitingUserAction {
        return Ok(Some(ActiveTurnState::running()));
    }
    let resolves_pending = match active.current_action_id.as_deref() {
        Some(action_id) => user_action_store
            .get(session_id, turn_id, action_id)
            .await?
            .is_some_and(|record| {
                matches!(
                    record.action.state,
                    UserActionState::Resolved | UserActionState::Cancelled
                ) && record
                    .action
                    .item_id
                    .as_deref()
                    .is_none_or(|pending_item_id| pending_item_id == item_id)
            }),
        None => false,
    };
    if resolves_pending {
        return Ok(Some(ActiveTurnState::running()));
    }
    Ok(Some(ActiveTurnState::waiting(&active)))
}
