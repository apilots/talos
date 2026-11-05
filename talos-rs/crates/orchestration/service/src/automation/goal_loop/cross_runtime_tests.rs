use super::*;
use crate::automation::goal_loop::SessionAutomationService;

struct ConcurrentSkills(tokio::sync::Barrier);

impl AutomationSkillResolver for ConcurrentSkills {
    fn resolve<'a>(
        &'a self,
        session: &'a talos_store::session::SessionRecord,
        mode: AutomationMode,
    ) -> futures_util::future::BoxFuture<'a, TalosResult<SkillSelector>> {
        Box::pin(async move {
            // Both owners have read the same deadline before either claims it.
            self.0.wait().await;
            TestSkills.resolve(session, mode).await
        })
    }
}

#[tokio::test]
async fn independent_runtime_queues_share_one_durable_deadline_claim() {
    let (_, store) = fixture().await;
    let session = store.get_session_record("alice").await.unwrap().unwrap();
    let now = chrono::Utc::now().timestamp();
    let task = SessionLoop {
        id: "shared-deadline".into(),
        session_id: session.id.clone(),
        prompt: "Check the build".into(),
        schedule: LoopSchedule::FixedInterval { minutes: 1 },
        config_revision: 1,
        status: LoopStatus::Active,
        stop_reason: None,
        reason: None,
        next_due_at: Some(now - 1),
        latest_turn_id: None,
        created_at: now - AUTOMATION_INITIAL_DELAY_SECONDS - 1,
        updated_at: now - AUTOMATION_INITIAL_DELAY_SECONDS - 1,
    };
    store.automation_store().create_loop(&task).await.unwrap();
    let skills = Arc::new(ConcurrentSkills(tokio::sync::Barrier::new(2)));
    let owner = |queue| {
        SessionAutomationService::new(
            store.automation_store(),
            store.clone(),
            queue,
            skills.clone(),
        )
    };
    let first_queue = crate::SessionMessageQueueService::new(10).unwrap();
    let second_queue = crate::SessionMessageQueueService::new(10).unwrap();
    let first = owner(first_queue.clone());
    let second = owner(second_queue.clone());
    let (left, right) = tokio::join!(first.scan(&session, now), second.scan(&session, now));
    left.unwrap();
    right.unwrap();
    let queued = first_queue.list(&session.id).unwrap().messages.len()
        + second_queue.list(&session.id).unwrap().messages.len();
    assert_eq!(queued, 1);
    assert_eq!(
        store
            .automation_store()
            .loop_task(&task.id)
            .await
            .unwrap()
            .next_due_at,
        None
    );
}

#[tokio::test]
async fn session_gates_release_idle_mutexes_after_last_guard() {
    let (service, _) = fixture().await;
    let automation = service.turns.automation_service().unwrap();
    let first = automation.lock("alice").await;
    let weak = Arc::downgrade(tokio::sync::OwnedMutexGuard::mutex(&first));
    drop(first);
    let second = automation.lock("bob").await;
    assert_eq!(weak.strong_count(), 0);
    drop(second);
}
