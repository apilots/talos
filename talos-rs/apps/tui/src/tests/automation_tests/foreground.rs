use super::*;
use crate::{wait_for_session_automation, GatewayClient};
use std::time::Duration;

async fn observed_active(client: &MockGatewayClient) {
    tokio::time::timeout(Duration::from_secs(1), async {
        while client.state.lock().unwrap().automation_reads == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("foreground observer should read the active definition");
}
async fn invalidate(client: &MockGatewayClient) {
    let senders = client
        .state
        .lock()
        .unwrap()
        .message_queue_subscribers
        .clone();
    for sender in senders {
        sender
            .send(crate::client::SessionEvent::AutomationChanged(
                AutomationChangedNotification {
                    session_id: "sess-test".into(),
                },
            ))
            .await
            .unwrap();
    }
}
#[tokio::test]
async fn foreground_preserves_admitted_work_after_goal_pause_or_loop_stop() {
    for mode in [AutomationMode::Goal, AutomationMode::Loop] {
        let client = Arc::new(MockGatewayClient::new());
        let (object, origin) = match mode {
            AutomationMode::Goal => (
                AutomationObjectRef::Goal {
                    goal_id: "goal-1".into(),
                },
                AutomationTurnOrigin::Goal {
                    goal_id: "goal-1".into(),
                    objective_revision: 4,
                },
            ),
            AutomationMode::Loop => (
                AutomationObjectRef::Loop {
                    loop_id: "loop-1".into(),
                },
                AutomationTurnOrigin::Loop {
                    loop_id: "loop-1".into(),
                    occurrence_id: "occurrence".into(),
                    config_revision: 3,
                },
            ),
        };
        {
            let mut state = client.state.lock().unwrap();
            state.automation = Some(snapshot(mode));
            state.active_turn_id = Some("running-1".into());
            state.active_input_origin = Some(AutomationInputOrigin {
                message_id: "input".into(),
                origin,
            });
        }
        let observer = tokio::spawn(wait_for_session_automation(
            client.clone(),
            "sess-test".into(),
            object,
        ));
        observed_active(&client).await;
        {
            let mut state = client.state.lock().unwrap();
            let automation = state.automation.as_mut().unwrap();
            match mode {
                AutomationMode::Goal => {
                    automation.goal.as_mut().unwrap().status = GoalStatus::Paused
                }
                AutomationMode::Loop => automation.loops[0].status = LoopStatus::Stopped,
            }
        }
        invalidate(&client).await;
        tokio::time::timeout(Duration::from_secs(1), async {
            while client.state.lock().unwrap().automation_reads < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(!observer.is_finished());
        assert_eq!(client.state.lock().unwrap().shutdown_calls, 0);
        client.state.lock().unwrap().active_turn_id = None;
        invalidate(&client).await;
        tokio::time::timeout(Duration::from_secs(1), observer)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        client.shutdown().await.unwrap();
        assert_eq!(client.state.lock().unwrap().shutdown_calls, 1);
        assert!(client.state.lock().unwrap().automation_requests.is_empty());
    }
}
#[tokio::test]
async fn foreground_tracks_exact_loop_id_among_identical_prompts() {
    let client = Arc::new(MockGatewayClient::new());
    let mut state = snapshot(AutomationMode::Loop);
    let mut other = loop_fixture();
    other.id = "other-loop".into();
    state.loops.push(other);
    let mut independent = loop_fixture();
    independent.id = "keeps-running".into();
    state.loops.push(independent);
    client.state.lock().unwrap().automation = Some(state);
    let observer = tokio::spawn(wait_for_session_automation(
        client.clone(),
        "sess-test".into(),
        AutomationObjectRef::Loop {
            loop_id: "loop-1".into(),
        },
    ));
    observed_active(&client).await;
    client
        .state
        .lock()
        .unwrap()
        .automation
        .as_mut()
        .unwrap()
        .loops[1]
        .status = LoopStatus::Stopped;
    invalidate(&client).await;
    tokio::task::yield_now().await;
    assert!(!observer.is_finished());
    client
        .state
        .lock()
        .unwrap()
        .automation
        .as_mut()
        .unwrap()
        .loops[0]
        .status = LoopStatus::Stopped;
    invalidate(&client).await;
    tokio::time::timeout(Duration::from_secs(1), observer)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(client.state.lock().unwrap().automation_requests.is_empty());
}
#[tokio::test]
async fn terminal_before_subscription_does_not_require_another_notification() {
    let client = Arc::new(MockGatewayClient::new());
    let mut state = snapshot(AutomationMode::Goal);
    state.goal.as_mut().unwrap().status = GoalStatus::Complete;
    client.state.lock().unwrap().automation = Some(state);
    tokio::time::timeout(
        Duration::from_secs(1),
        wait_for_session_automation(
            client.clone(),
            "sess-test".into(),
            AutomationObjectRef::Goal {
                goal_id: "goal-1".into(),
            },
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        client.state.lock().unwrap().message_queue_subscribers.len(),
        1
    );
    assert_eq!(client.state.lock().unwrap().automation_reads, 1);
}
#[tokio::test]
async fn saved_state_change_from_another_process_is_observed_without_scheduling_work() {
    let client = Arc::new(MockGatewayClient::new());
    client.state.lock().unwrap().automation = Some(snapshot(AutomationMode::Goal));
    let observer = tokio::spawn(wait_for_session_automation(
        client.clone(),
        "sess-test".into(),
        AutomationObjectRef::Goal {
            goal_id: "goal-1".into(),
        },
    ));
    observed_active(&client).await;
    // Model a management process that commits the same DB but cannot publish on
    // this observer's process-local subscription channel.
    client
        .state
        .lock()
        .unwrap()
        .automation
        .as_mut()
        .unwrap()
        .goal
        .as_mut()
        .unwrap()
        .status = GoalStatus::Paused;
    tokio::time::timeout(Duration::from_secs(3), observer)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(client.state.lock().unwrap().automation_reads >= 2);
    assert!(client.state.lock().unwrap().automation_requests.is_empty());
}
