use super::*;

#[tokio::test]
async fn timeline_recovery_does_not_settle_this_process_admission() {
    let reached = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let (service, store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::AdmissionBlocked {
                reached: reached.clone(),
                release: release.clone(),
            },
        }),
        configured_test_config(),
    )
    .await;
    let service = Arc::new(service);
    let actor = Actor::regular("member-user");
    let session_id = "session-admission-recovery";
    let start_service = service.clone();
    let start_actor = actor.clone();
    let start = tokio::spawn(async move {
        start_service
            .start_turn(
                &start_actor,
                request_source(),
                start_turn_request(session_id, "hello"),
                None,
            )
            .await
    });
    reached.notified().await;

    let active_before = store
        .get_agent_active_turn(session_id)
        .await
        .unwrap()
        .expect("admission must be durable before the start permit is consumed");
    let recovered = service
        .recover_timeline(
            &actor,
            talos_types::RecoverTimelineRequest {
                session_id: session_id.to_string(),
                after_event_id: None,
                replay_limit: Some(32),
            },
        )
        .await
        .expect("recovery should reuse the live admission owner");

    assert_eq!(
        recovered.active_turn,
        Some(crate::timeline::mapping::map_timeline_active_turn(
            active_before
        ))
    );
    assert!(recovered.replay.events.iter().all(|event| !matches!(
        event.event.agent_event().unwrap().payload,
        CanonicalAgentEventPayload::TurnFailed(_)
    )));

    release.notify_one();
    start.await.unwrap().unwrap();
}
