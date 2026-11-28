//! Product title delivery consumes retained Worker facts, including optional failures.
use super::*;

#[tokio::test]
async fn worker_titles_settle_usage_once_and_keep_failures_or_user_titles_nonblocking() {
    for (outcome, paid, existing_title) in [
        (
            agent_protocol::AgentSessionTitleOutcome::Generated {
                title: "Worker title".into(),
            },
            true,
            false,
        ),
        (
            agent_protocol::AgentSessionTitleOutcome::Generated {
                title: "Worker title".into(),
            },
            true,
            true,
        ),
        (
            agent_protocol::AgentSessionTitleOutcome::Failed {
                error: TalosError::internal("The title model returned empty text.").to_api_error(),
            },
            true,
            false,
        ),
        (
            agent_protocol::AgentSessionTitleOutcome::Failed {
                error: TalosError::new(
                    talos_error::ErrorCode::ProviderUnavailable,
                    "Title model rejected the request.",
                )
                .to_api_error(),
            },
            false,
            false,
        ),
    ] {
        let (service, store, journal) = test_application_with_worker_storage(
            Arc::new(TestAgentExecution {
                mode: TestAgentExecutionMode::Accepted,
            }),
            Arc::new(TestAgentCoordinator {
                cancel_mode: TestCancelMode::Cancelled,
                cancelled_turn_ids: Arc::default(),
                compaction_usage: Default::default(),
                compaction_error: None,
            }),
            configured_test_config(),
            None,
            Arc::new(SqliteStorage::new(":memory:").await.unwrap()),
            Some(Arc::new(Mutex::new(
                crate::test_worker::AdmissionFault::TitleDeliveryBusy(if existing_title {
                    talos_error::ErrorCode::Conflict
                } else {
                    talos_error::ErrorCode::ActiveTurnRunning
                }),
            ))),
        )
        .await;
        let actor = Actor::regular("member-user");
        let session_id = "title-session";
        service
            .create_session(
                &actor,
                create_session_params(session_id, request_source(), None),
            )
            .await
            .unwrap();
        let identity = AgentTurnRef::new(session_id, "turn");
        let message = crate::turn::completed_agent_user_input(
            &identity.turn_id,
            "input",
            crate::test_support::test_user_input("hello"),
            1,
        )
        .unwrap();
        let mut events =
            crate::test_support::canonical_user_admission(identity.clone(), message, 1, true);
        events.push(CanonicalAgentEvent {
            identity: identity.clone(),
            sequence: events.last().unwrap().sequence + 1,
            occurred_at: 2,
            payload: CanonicalAgentEventPayload::TurnCompleted(AgentTurnCompleted {
                stop_reason: talos_agent_api::AgentStopReason::EndTurn,
            }),
        });
        seed_worker_fixture(&service, &store, &journal, events, None).await;
        let scope = journal.session_scope(session_id).await.unwrap().unwrap();
        let usage = TokenUsage {
            input_tokens: 2,
            output_tokens: 1,
            total_tokens: 3,
            ..Default::default()
        };
        journal
            .save_session_title(
                &agent_protocol::AgentSessionTitleParams {
                    session: scope,
                    identity: identity.clone(),
                    mode: agent_protocol::AgentSessionTitleMode::Generate,
                },
                &agent_protocol::AgentSessionTitleResult {
                    outcome: outcome.clone(),
                    model_usage: paid.then(|| agent_protocol::AgentMemoryModelUsage {
                        request_id: "paid-worker-title".into(),
                        model_ref: talos_types::ModelRef {
                            provider: "target-worker".into(),
                            id: "worker-inner".into(),
                        },
                        usage: usage.clone(),
                        completed_at: 42,
                        timezone: "Asia/Shanghai".into(),
                    }),
                },
            )
            .await
            .unwrap();
        if existing_title {
            store
                .set_generated_title(&identity, "User title")
                .await
                .unwrap();
        }
        service
            .turns
            .recover_worker_settlements(session_id, false)
            .await
            .unwrap();
        assert_eq!(
            store
                .pending_worker_settlements(session_id)
                .await
                .unwrap()
                .len(),
            1
        );
        service
            .turns
            .recover_worker_settlements(session_id, false)
            .await
            .unwrap();
        let record = store.get_session_record(session_id).await.unwrap().unwrap();
        let expected_title = if existing_title {
            "User title"
        } else if matches!(
            outcome,
            agent_protocol::AgentSessionTitleOutcome::Generated { .. }
        ) {
            "Worker title"
        } else {
            talos_types::DEFAULT_SESSION_TITLE
        };
        assert_eq!(record.title, expected_title);
        let profile = service.profile.read_profile(&actor).await.unwrap();
        assert_eq!(
            profile.usage,
            if paid { usage } else { TokenUsage::default() }
        );
        assert!(store
            .pending_worker_settlements(session_id)
            .await
            .unwrap()
            .is_empty());
    }
}
