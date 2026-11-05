//! User queue arrival during Skill resolution.
use super::*;

struct EnqueueUserSkills {
    queue: crate::SessionMessageQueueService,
}
impl AutomationSkillResolver for EnqueueUserSkills {
    fn resolve<'a>(
        &'a self,
        session: &'a talos_store::session::SessionRecord,
        mode: AutomationMode,
    ) -> futures_util::future::BoxFuture<'a, TalosResult<SkillSelector>> {
        Box::pin(async move {
            // Reproduce User enqueue after the preliminary idle check but before
            // automatic admission, while the Worker Skill request is in flight.
            self.queue.add(
                &session.id,
                crate::test_support::test_user_input("New User work"),
            )?;
            Ok(SkillSelector::selection(
                mode.skill_name(),
                format!("/verified-system/{}/SKILL.md", mode.skill_name()),
            ))
        })
    }
}
async fn enqueue_during_resolution_fixture() -> (TalosService, Arc<DatabaseSessionStore>) {
    let (service, store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::Accepted,
        }),
        configured_test_config(),
    )
    .await;
    service
        .install_session_automation(
            store.automation_store(),
            Arc::new(EnqueueUserSkills {
                queue: service.message_queue.clone(),
            }),
        )
        .unwrap();
    service
        .create_session(
            &Actor::regular("alice"),
            create_session_params("alice", request_source(), None),
        )
        .await
        .unwrap();
    (service, store)
}
#[tokio::test]
async fn goal_continuation_does_not_jump_user_queued_during_skill_resolution() {
    let (service, store) = enqueue_during_resolution_fixture().await;
    let db = store.automation_store();
    db.create_goal(&goal("g", "alice", GoalStatus::Active), None)
        .await
        .unwrap();
    let session = store.get_session_record("alice").await.unwrap().unwrap();
    let error = service
        .turns
        .continue_goal(session, false)
        .await
        .unwrap_err();
    assert!(error.is_code(talos_error::ErrorCode::Conflict));
    assert!(store
        .get_agent_active_turn("alice")
        .await
        .unwrap()
        .is_none());
    assert_eq!(db.goal("g").await.unwrap().status, GoalStatus::Active);
    assert_eq!(
        service.message_queue.list("alice").unwrap().messages.len(),
        1
    );
    service
        .turns
        .try_dispatch_next("alice")
        .await
        .unwrap()
        .unwrap();
    let active = store.get_agent_active_turn("alice").await.unwrap().unwrap();
    assert_eq!(db.origin(&active.identity).await.unwrap(), None);
}
#[tokio::test]
async fn loop_reservation_yields_to_user_queued_during_skill_resolution() {
    let (service, store) = enqueue_during_resolution_fixture().await;
    let db = store.automation_store();
    db.create_loop(&task("l", LoopSchedule::ModelDecided {}))
        .await
        .unwrap();
    service
        .message_queue
        .add_loop(
            "alice",
            crate::test_support::test_user_input("Queued Loop"),
            "l".into(),
            "o".into(),
        )
        .unwrap();
    let error = service.turns.try_dispatch_next("alice").await.unwrap_err();
    assert!(error.is_code(talos_error::ErrorCode::Conflict));
    assert!(store
        .get_agent_active_turn("alice")
        .await
        .unwrap()
        .is_none());
    let queue = service.message_queue.list("alice").unwrap();
    assert_eq!(queue.messages.len(), 2);
    assert_eq!(queue.messages[0].source, QueuedMessageSource::User);
    assert_eq!(db.loop_task("l").await.unwrap().status, LoopStatus::Active);
    service
        .turns
        .try_dispatch_next("alice")
        .await
        .unwrap()
        .unwrap();
    let active = store.get_agent_active_turn("alice").await.unwrap().unwrap();
    assert_eq!(db.origin(&active.identity).await.unwrap(), None);
    assert_eq!(
        service.message_queue.list("alice").unwrap().messages.len(),
        1
    );
}
