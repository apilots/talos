//! Keep the existing embedded CLI owner alive; never schedule execution here.
use crate::GatewayClient;
use std::sync::Arc;
use std::time::Duration;
use talos_error::TalosResult;
use talos_sdk::{
    AutomationObjectRef, GoalStatus, LoopStatus, SessionAutomationReadRequest,
    SessionAutomationSnapshot,
};

/// Observe one exact created object using the existing Session stream and reads.
/// A second management process shares durable state but not this process' event
/// channel, so a low-frequency read also observes its changes. Reads cannot tick,
/// resume, reconstruct or schedule work. The caller retains normal shutdown/SIGINT.
pub async fn wait_for_session_automation(
    client: Arc<dyn GatewayClient>,
    session_id: String,
    object: AutomationObjectRef,
) -> TalosResult<()> {
    // Subscribe before reading; a terminal state before subscription must still
    // be observed by the first snapshot rather than waiting for another event.
    let mut subscription = match client.subscribe_session_events(&session_id).await {
        Ok(subscription) => Some(subscription),
        Err(error) => {
            tracing::warn!(message=%error.to_api_error().message,"Session notifications unavailable; observing saved automation state");
            None
        }
    };
    let mut refresh = tokio::time::interval(Duration::from_secs(2));
    refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    refresh.tick().await;
    loop {
        let state = client
            .session_automation(SessionAutomationReadRequest {
                session_id: session_id.clone(),
            })
            .await?;
        if !is_active(&state, &object) {
            return Ok(());
        }
        tokio::select! {
            _=refresh.tick()=>{},
            live=async {subscription.as_mut().expect("guarded subscription").wait_for_change().await},if subscription.is_some()=>{
                if !live {subscription=None;}
            }
        }
    }
}
fn is_active(state: &SessionAutomationSnapshot, object: &AutomationObjectRef) -> bool {
    match object {
        AutomationObjectRef::Goal { goal_id: id } => state
            .goal
            .as_ref()
            .is_some_and(|goal| &goal.id == id && goal.status == GoalStatus::Active),
        AutomationObjectRef::Loop { loop_id: id } => state
            .loops
            .iter()
            .any(|task| &task.id == id && task.status == LoopStatus::Active),
    }
}
