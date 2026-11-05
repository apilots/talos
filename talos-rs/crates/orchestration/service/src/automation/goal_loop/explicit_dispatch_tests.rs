use super::initial_delay_tests::create;
use super::*;
use talos_error::ErrorCode;

async fn cancel_user_turn(service: &TalosService, store: &DatabaseSessionStore) {
    let (identity, commit) = admitted(
        service,
        store,
        None,
        crate::test_support::test_user_input("Ordinary User work"),
    )
    .await;
    store
        .commit_terminal_settlement(talos_store::session::CommitTerminalSettlementParams {
            event: CanonicalAgentEvent {
                identity,
                sequence: commit.events.last().unwrap().sequence + 1,
                occurred_at: chrono::Utc::now().timestamp(),
                payload: CanonicalAgentEventPayload::TurnFailed(talos_agent_api::AgentTurnFailed {
                    error: TalosError::new(ErrorCode::Cancelled, "User cancelled the Turn")
                        .to_api_error(),
                    retryable: false,
                }),
            },
            expected_event_seq: commit.active_turn.last_event_seq,
            completed_turn_metrics: None,
        })
        .await
        .unwrap();
}

async fn make_due(store: &DatabaseSessionStore, result: &AutomationControlResult) -> i64 {
    let deadline = chrono::Utc::now().timestamp();
    let db = store.automation_store();
    if let Some(goal) = result.snapshot.goal.as_ref() {
        let mut due = goal.clone();
        due.created_at = deadline - AUTOMATION_INITIAL_DELAY_SECONDS;
        db.replace_goal(goal, &due).await.unwrap();
    } else {
        let task = &result.snapshot.loops[0];
        let mut due = task.clone();
        due.created_at = deadline - AUTOMATION_INITIAL_DELAY_SECONDS;
        due.next_due_at = Some(deadline);
        db.replace_loop(task, &due).await.unwrap();
    }
    deadline
}

#[tokio::test]
async fn direct_creation_after_old_cancel_starts_only_its_due_work() {
    for action in [
        AutomationAction::Goal {
            action: GoalAction::Create {
                objective: "Finish tests".into(),
                token_budget: None,
            },
        },
        AutomationAction::Loop {
            action: LoopAction::Create {
                prompt: "Check CI".into(),
                schedule: LoopSchedule::ModelDecided {},
            },
        },
    ] {
        let (service, store) = fixture().await;
        cancel_user_turn(&service, &store).await;
        let result = create(&service, action).await;
        assert!(store
            .get_agent_active_turn("alice")
            .await
            .unwrap()
            .is_none());
        let deadline = make_due(&store, &result).await;
        service.scan_session_loops(deadline).await.unwrap();
        let active = store.get_agent_active_turn("alice").await.unwrap().unwrap();
        let origin = store
            .automation_store()
            .origin(&active.identity)
            .await
            .unwrap()
            .unwrap();
        match (result.created_object.unwrap(), origin) {
            (
                AutomationObjectRef::Goal { goal_id },
                AutomationTurnOrigin::Goal {
                    goal_id: actual, ..
                },
            ) => assert_eq!(actual, goal_id),
            (
                AutomationObjectRef::Loop { loop_id },
                AutomationTurnOrigin::Loop {
                    loop_id: actual, ..
                },
            ) => assert_eq!(actual, loop_id),
            pair => panic!("Unexpected admitted work: {pair:?}"),
        }
    }
}

#[tokio::test]
async fn deferred_creation_does_not_resume_user_queue_or_override_a_new_cancel() {
    let (service, store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::Accepted,
        }),
        configured_test_config(),
    )
    .await;
    let skills = Arc::new(DispatchSkills::default());
    service
        .install_session_automation(store.automation_store(), skills.clone())
        .unwrap();
    service
        .create_session(
            &Actor::regular("alice"),
            create_session_params("alice", request_source(), None),
        )
        .await
        .unwrap();
    cancel_user_turn(&service, &store).await;
    let result = create(
        &service,
        AutomationAction::Loop {
            action: LoopAction::Create {
                prompt: "Check CI".into(),
                schedule: LoopSchedule::ModelDecided {},
            },
        },
    )
    .await;
    let deadline = make_due(&store, &result).await;
    let queue = service
        .message_queue
        .add(
            "alice",
            crate::test_support::test_user_input("Wait for Play"),
        )
        .unwrap();
    let user_message = queue.messages[0].id.clone();
    service.scan_session_loops(deadline).await.unwrap();
    assert!(store
        .get_agent_active_turn("alice")
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        service.message_queue.list("alice").unwrap().messages.len(),
        2
    );
    service
        .message_queue
        .delete("alice", &user_message)
        .unwrap();
    // The explicit permit passes the preliminary check. A newer User cancellation
    // while the remote Skill request is in flight must still win final admission.
    skills
        .block
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let dispatch = tokio::spawn({
        let service = service.clone();
        async move { service.turns.try_dispatch_next("alice").await }
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), skills.entered.notified())
        .await
        .unwrap();
    cancel_user_turn(&service, &store).await;
    skills.release.notify_one();
    let error = dispatch.await.unwrap().unwrap_err();
    assert!(error.is_code(ErrorCode::Conflict));
    assert!(store
        .get_agent_active_turn("alice")
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        service.message_queue.list("alice").unwrap().messages.len(),
        1
    );
}

#[derive(Default)]
struct DispatchSkills {
    block: std::sync::atomic::AtomicBool,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
impl AutomationSkillResolver for DispatchSkills {
    fn resolve<'a>(
        &'a self,
        session: &'a talos_store::session::SessionRecord,
        mode: AutomationMode,
    ) -> futures_util::future::BoxFuture<'a, TalosResult<SkillSelector>> {
        Box::pin(async move {
            if self.block.swap(false, std::sync::atomic::Ordering::SeqCst) {
                self.entered.notify_one();
                self.release.notified().await;
            }
            TestSkills.resolve(session, mode).await
        })
    }
}
