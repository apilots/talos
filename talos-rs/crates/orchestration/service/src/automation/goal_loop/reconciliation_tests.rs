use super::*;
use talos_agent_api::AgentEventPayload;

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
async fn started_turns(store: &DatabaseSessionStore) -> usize {
    store
        .list_agent_events_after("alice", None, 100)
        .await
        .unwrap()
        .iter()
        .filter(|event| matches!(event.payload, AgentEventPayload::TurnStarted(_)))
        .count()
}

#[tokio::test]
async fn persisted_idle_goal_continues_from_each_original_read_and_admits_only_one_turn() {
    for entry in ENTRIES {
        let (service, store) = fixture().await;
        store
            .automation_store()
            .create_goal(&goal("g", "alice", GoalStatus::Active), None)
            .await
            .unwrap();
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
