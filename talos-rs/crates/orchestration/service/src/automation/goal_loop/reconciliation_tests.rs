use super::*;
use talos_agent_api::{
    ActiveTurnState, AgentContinuationRef, AgentEvent, AgentEventPayload, AgentPendingInput,
};

#[derive(Clone, Copy, Debug)]
enum ReadEntry {
    Subscribe,
    Recover,
}
const ENTRIES: [ReadEntry; 2] = [ReadEntry::Subscribe, ReadEntry::Recover];

async fn read_goal(service: &TalosService, entry: ReadEntry) {
    let actor = Actor::regular("alice");
    match entry {
        ReadEntry::Subscribe => {
            let subscription = service
                .open_timeline_subscription(
                    &actor,
                    SubscribeTimelineRequest {
                        session_id: "alice".into(),
                        turn_id: None,
                        since_event_id: None,
                        replay_limit: Some(20),
                        close_when_idle: Some(false),
                        close_on_terminal: Some(false),
                    },
                )
                .await
                .expect("metadata reconciliation must not make subscription fail");
            drop(subscription);
        }
        ReadEntry::Recover => {
            service
                .recover_timeline(
                    &actor,
                    RecoverTimelineRequest {
                        session_id: "alice".into(),
                        after_event_id: None,
                        replay_limit: Some(20),
                    },
                )
                .await
                .expect("metadata reconciliation must not make recovery reads fail");
        }
    }
}
async fn fixture_with_state(
    mode: TestAgentExecutionMode,
    installed: bool,
    status: GoalStatus,
) -> (TalosService, Arc<DatabaseSessionStore>) {
    let (service, store) = test_application(
        Arc::new(TestAgentExecution { mode }),
        configured_test_config(),
    )
    .await;
    if installed {
        service
            .install_session_automation(store.automation_store(), Arc::new(TestSkills))
            .unwrap();
    }
    service
        .create_session(
            &Actor::regular("alice"),
            create_session_params("alice", request_source(), None),
        )
        .await
        .unwrap();
    store
        .automation_store()
        .create_goal(&goal("g", "alice", status), None)
        .await
        .unwrap();
    (service, store)
}
async fn started_turns(store: &DatabaseSessionStore) -> usize {
    store
        .list_agent_events_after("alice", None, 100)
        .await
        .unwrap()
        .iter()
        .filter(|event| matches!(event.payload, AgentEventPayload::TurnStarted(_)))
        .count()
}
async fn assert_idle_goal(store: &DatabaseSessionStore, status: GoalStatus) {
    assert!(store
        .get_agent_active_turn("alice")
        .await
        .unwrap()
        .is_none());
    assert_eq!(started_turns(store).await, 0);
    assert_eq!(
        store.automation_store().goal("g").await.unwrap().status,
        status
    );
}

#[tokio::test]
async fn persisted_idle_goal_continues_from_each_original_read_and_admits_only_one_turn() {
    for entry in ENTRIES {
        let (service, store) =
            fixture_with_state(TestAgentExecutionMode::Accepted, true, GoalStatus::Active).await;
        // Two reconnect requests can observe the same idle boundary; original
        // admission still owns the single durable Turn.
        tokio::join!(read_goal(&service, entry), read_goal(&service, entry));
        let first = store
            .get_agent_active_turn("alice")
            .await
            .unwrap()
            .expect("known idle Goal should resume work");
        assert_eq!(
            store
                .automation_store()
                .origin(&first.identity)
                .await
                .unwrap(),
            Some(AutomationTurnOrigin::Goal {
                goal_id: "g".into(),
                objective_revision: 1,
            })
        );
        for again in ENTRIES {
            read_goal(&service, again).await;
            assert_eq!(
                store
                    .get_agent_active_turn("alice")
                    .await
                    .unwrap()
                    .unwrap()
                    .identity,
                first.identity
            );
        }
        assert_eq!(started_turns(&store).await, 1);
        assert_eq!(
            store.automation_store().goal("g").await.unwrap().status,
            GoalStatus::Active
        );
    }
}
#[tokio::test]
async fn worker_busy_and_foreign_coordinator_leave_goal_active_without_local_admission() {
    for entry in ENTRIES {
        for foreign_coordinator in [false, true] {
            let (service, store) = fixture_with_state(
                TestAgentExecutionMode::WorkerStateActive,
                true,
                GoalStatus::Active,
            )
            .await;
            if foreign_coordinator {
                service.set_coordinator_id("local-peer".into());
            }
            read_goal(&service, entry).await;
            assert_idle_goal(&store, GoalStatus::Active).await;
        }
    }
}
#[tokio::test]
async fn offline_worker_does_not_fail_reads_pause_goal_or_fabricate_work() {
    for entry in ENTRIES {
        let (service, store) = fixture_with_state(
            TestAgentExecutionMode::WorkerStateUnavailable,
            true,
            GoalStatus::Active,
        )
        .await;
        read_goal(&service, entry).await;
        assert_idle_goal(&store, GoalStatus::Active).await;
    }
}
#[tokio::test]
async fn pending_user_queue_prevents_goal_reconnect_from_jumping_or_consuming_it() {
    for entry in ENTRIES {
        let (service, store) =
            fixture_with_state(TestAgentExecutionMode::Accepted, true, GoalStatus::Active).await;
        let queued = service
            .message_queue
            .add(
                "alice",
                crate::test_support::test_user_input("User work first"),
            )
            .unwrap();
        read_goal(&service, entry).await;
        assert_idle_goal(&store, GoalStatus::Active).await;
        assert_eq!(service.message_queue.list("alice").unwrap(), queued);
    }
}
#[tokio::test]
async fn paused_or_uninstalled_goal_does_not_start_work_from_reads() {
    for entry in ENTRIES {
        for (installed, status) in [(true, GoalStatus::Paused), (false, GoalStatus::Active)] {
            let (service, store) = fixture_with_state(
                TestAgentExecutionMode::WorkerStateUnavailable,
                installed,
                status,
            )
            .await;
            read_goal(&service, entry).await;
            assert_idle_goal(&store, status).await;
        }
    }
}
#[tokio::test]
async fn durable_waiting_turn_is_preserved_and_does_not_start_another_goal_turn() {
    for entry in ENTRIES {
        let (service, store) = fixture_with_state(
            TestAgentExecutionMode::WorkerStateUnavailable,
            true,
            GoalStatus::Active,
        )
        .await;
        let (identity, admission) = admitted(
            &service,
            &store,
            Some(AutomationTurnOrigin::Goal {
                goal_id: "g".into(),
                objective_revision: 1,
            }),
            crate::test_support::test_user_input("Goal work awaiting user input"),
        )
        .await;
        store
            .commit_waiting_settlement(
                AgentEvent {
                    identity: identity.clone(),
                    sequence: admission.events.last().unwrap().sequence + 1,
                    occurred_at: 2,
                    payload: AgentEventPayload::WaitingForInput(AgentPendingInput::Message {
                        action_id: "pending-input".into(),
                        prompt: "Please provide a decision".into(),
                    }),
                },
                AgentContinuationRef {
                    opaque_id: "known-continuation".into(),
                },
                admission.active_turn.last_event_seq,
            )
            .await
            .unwrap();
        let before = store.get_agent_active_turn("alice").await.unwrap().unwrap();
        assert_eq!(before.state, ActiveTurnState::WaitingForInput);
        read_goal(&service, entry).await;
        assert_eq!(
            store.get_agent_active_turn("alice").await.unwrap(),
            Some(before)
        );
        assert_eq!(started_turns(&store).await, 1);
        assert_eq!(
            store.automation_store().goal("g").await.unwrap().status,
            GoalStatus::Active
        );
        assert_eq!(
            store
                .get_agent_continuation_binding(&identity)
                .await
                .unwrap()
                .unwrap()
                .action_id,
            "pending-input"
        );
    }
}
