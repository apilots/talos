use super::*;

fn completed() -> Vec<AgentCommittedEvent> {
    let mut events = admission();
    events.push(fact(
        14,
        3,
        AgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    ));
    events
}

async fn begin(s: &DatabaseSessionStore) -> String {
    s.begin_worker_projection_rebuild(WorkerProjectionRebuildParams {
        session_id: "session-1".into(),
        execution_node_id: "embedded".into(),
        history_generation: 0,
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn rebuild_restores_missing_events_and_summary_without_reapplying_product_receipts() {
    let (s, _dir) = store().await;
    let events = completed();
    let mut original = s
        .project_worker_events(params(events.clone()))
        .await
        .unwrap();
    s.storage
        .execute(
            "UPDATE agent_events SET persisted_at=100 WHERE session_id='session-1'",
            &[],
        )
        .await
        .unwrap();
    s.storage.execute("UPDATE worker_event_projection SET product_persisted_at=100 WHERE session_id='session-1'", &[]).await.unwrap();
    for event in &mut original.events {
        event.persisted_at = 100;
    }
    let notice = s
        .complete_worker_settlement_with_notice(
            &original.cursor,
            talos_agent_api::TimelineSystemNotice {
                identity: AgentTurnRef::new("session-1", "turn-1"),
                occurred_at: 11,
                notice: talos_agent_api::AgentSystemNotice {
                    level: talos_types::TalosTimelineNoticeLevel::Info,
                    content: talos_agent_api::SystemNoticeContent::Text(
                        "Keep the original notice".into(),
                    ),
                },
            },
        )
        .await
        .unwrap()
        .unwrap();
    s.storage
        .execute(
            "UPDATE sessions SET title='User title' WHERE session_key='session-1'",
            &[],
        )
        .await
        .unwrap();
    s.patch_session(crate::session::PatchSessionParams {
        session_id: "session-1".into(),
        last_activity: 90000,
        change: crate::session::SessionRecordPatch::Settings {
            conf: SessionConf::default(),
            current_directory: Some("/user/settings".into()),
            pinned: Some(true),
        },
    })
    .await
    .unwrap();
    s.advance_dream_memory_cursor(
        "session-1",
        None,
        &agent_protocol::DreamMemoryCursor {
            generation: 0,
            offset: 3,
        },
        Some(14),
    )
    .await
    .unwrap();
    s.storage.execute("UPDATE agent_events SET automation_accounted_tokens=17 WHERE session_id='session-1' AND sequence=1", &[]).await.unwrap();
    let mappings = s
        .storage
        .query(
            "SELECT * FROM worker_event_projection ORDER BY worker_offset",
            &[],
        )
        .await
        .unwrap();
    let ledger: Vec<_> = mappings.iter().map(|row| row.values.clone()).collect();
    s.storage
        .execute(
            "DELETE FROM agent_events WHERE session_id='session-1' AND sequence=2",
            &[],
        )
        .await
        .unwrap();
    s.storage.execute("UPDATE sessions SET message_count=999,latest_turn_seq=999 WHERE session_key='session-1'", &[]).await.unwrap();
    let rebuild = begin(&s).await;
    s.stage_worker_projection_rebuild(&rebuild, params(events))
        .await
        .unwrap();
    let repaired = s.finish_worker_projection_rebuild(&rebuild).await.unwrap();
    assert_eq!(repaired.event_count, 3);
    assert_eq!(repaired.cursor, original.cursor);
    let restored = s
        .list_agent_turn_events(&AgentTurnRef::new("session-1", "turn-1"), 10)
        .await
        .unwrap();
    assert_eq!(
        restored
            .iter()
            .map(|event| event.event_seq)
            .collect::<Vec<_>>(),
        original
            .events
            .iter()
            .map(|event| event.event_seq)
            .collect::<Vec<_>>()
    );
    assert_eq!(restored, original.events);
    let timeline = s
        .list_timeline_events_after("session-1", 0, 100)
        .await
        .unwrap();
    assert!(timeline.contains(&notice));
    let product = s.get_session_record("session-1").await.unwrap().unwrap();
    assert_eq!(product.title, "User title");
    assert_eq!(product.last_activity, 90000);
    assert!(product.pinned);
    assert_eq!(product.current_directory, Some("/user/settings".into()));
    let row = s.storage.query("SELECT automation_accounted_tokens FROM agent_events WHERE session_id='session-1' AND sequence=1", &[]).await.unwrap().remove(0);
    assert_eq!(row.get_i64("automation_accounted_tokens"), Some(17));
    assert_eq!(
        s.get_session_record("session-1")
            .await
            .unwrap()
            .unwrap()
            .message_count,
        2
    );
    let after = s
        .storage
        .query(
            "SELECT * FROM worker_event_projection ORDER BY worker_offset",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(
        after
            .iter()
            .map(|row| row.values.clone())
            .collect::<Vec<_>>(),
        ledger
    );
    assert!(s
        .pending_worker_settlements("session-1")
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        s.dream_memory_cursor("session-1").await.unwrap(),
        Some(agent_protocol::DreamMemoryCursor {
            generation: 0,
            offset: 3
        })
    );
}

#[tokio::test]
async fn interrupted_staging_and_expired_source_leave_the_old_view_visible() {
    let (s, _dir) = store().await;
    let original = s.project_worker_events(params(completed())).await.unwrap();
    let before = s
        .list_agent_turn_events(&AgentTurnRef::new("session-1", "turn-1"), 10)
        .await
        .unwrap();
    let rebuild = begin(&s).await;
    let mut first = params(admission());
    first.page.has_more = true;
    s.stage_worker_projection_rebuild(&rebuild, first)
        .await
        .unwrap();
    assert!(s
        .finish_worker_projection_rebuild(&rebuild)
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    assert_eq!(
        s.list_agent_turn_events(&AgentTurnRef::new("session-1", "turn-1"), 10)
            .await
            .unwrap(),
        before
    );
    let replacement = begin(&s).await;
    s.stage_worker_projection_rebuild(&replacement, params(vec![completed().pop().unwrap()]))
        .await
        .unwrap();
    assert!(s
        .finish_worker_projection_rebuild(&replacement)
        .await
        .unwrap_err()
        .is_code(ErrorCode::EventCursorExpired));
    assert_eq!(
        s.worker_event_cursor("session-1").await.unwrap(),
        Some(original.cursor)
    );
    assert_eq!(
        s.list_agent_turn_events(&AgentTurnRef::new("session-1", "turn-1"), 10)
            .await
            .unwrap(),
        before
    );
}

#[tokio::test]
async fn rebuild_rejects_missing_receipt_mapping_before_touching_the_view() {
    let (s, _dir) = store().await;
    let events = completed();
    s.project_worker_events(params(events.clone()))
        .await
        .unwrap();
    s.storage
        .execute(
            "DELETE FROM worker_event_projection WHERE worker_offset=9",
            &[],
        )
        .await
        .unwrap();
    let before = s
        .list_agent_turn_events(&AgentTurnRef::new("session-1", "turn-1"), 10)
        .await
        .unwrap();
    let rebuild = begin(&s).await;
    s.stage_worker_projection_rebuild(&rebuild, params(events))
        .await
        .unwrap();
    assert!(s
        .finish_worker_projection_rebuild(&rebuild)
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    assert_eq!(
        s.list_agent_turn_events(&AgentTurnRef::new("session-1", "turn-1"), 10)
            .await
            .unwrap(),
        before
    );
}

#[tokio::test]
async fn rebuild_cannot_restore_a_previous_history_generation() {
    let (s, _dir) = store().await;
    let rebuild = begin(&s).await;
    s.stage_worker_projection_rebuild(&rebuild, params(completed()))
        .await
        .unwrap();
    s.clear_session_history("session-1", 20).await.unwrap();
    assert!(s
        .finish_worker_projection_rebuild(&rebuild)
        .await
        .unwrap_err()
        .is_code(ErrorCode::EventCursorExpired));
    assert!(s
        .list_agent_turn_events(&AgentTurnRef::new("session-1", "turn-1"), 10)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn waiting_view_rebuild_preserves_turn_action_and_cancellation_identity_before_resume() {
    for cancel in [false, true] {
        let (s, _dir) = store().await;
        let mut events = admission();
        let mut waiting = fact(
            14,
            3,
            AgentEventPayload::WaitingForInput(talos_agent_api::AgentPendingInput::Message {
                action_id: "action-1".into(),
                prompt: "Continue?".into(),
            }),
        );
        waiting.settlement = Some(AgentExecutionSettlement::WaitingForInput {
            metrics: AgentRunMetrics::default(),
        });
        events.push(waiting);
        let original = s
            .project_worker_events(params(events.clone()))
            .await
            .unwrap();
        s.complete_worker_settlement(&original.cursor)
            .await
            .unwrap();
        let identity = AgentTurnRef::new("session-1", "turn-1");
        if cancel {
            s.begin_cancellation(&identity).await.unwrap();
        }
        let cancellation = s.storage.query("SELECT cancellation_request_id FROM agent_active_turns WHERE session_id='session-1'", &[]).await.unwrap().remove(0).get_string("cancellation_request_id");
        s.storage
            .execute(
                "DELETE FROM agent_events WHERE session_id='session-1' AND sequence=3",
                &[],
            )
            .await
            .unwrap();
        let rebuild = begin(&s).await;
        s.stage_worker_projection_rebuild(&rebuild, params(events))
            .await
            .unwrap();
        let repaired = s.finish_worker_projection_rebuild(&rebuild).await.unwrap();
        let active = s.get_agent_active_turn("session-1").await.unwrap().unwrap();
        assert_eq!(active.identity, identity);
        assert_eq!(active.current_action_id.as_deref(), Some("action-1"));
        assert_eq!(active.cancellation_requested, cancel);
        let restored = s.storage.query("SELECT cancellation_request_id FROM agent_active_turns WHERE session_id='session-1'", &[]).await.unwrap().remove(0).get_string("cancellation_request_id");
        assert_eq!(restored, cancellation);
        assert!(s
            .pending_worker_settlements("session-1")
            .await
            .unwrap()
            .is_empty());
        let mut next = params(vec![fact(
            20,
            4,
            AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                stop_reason: AgentStopReason::EndTurn,
            }),
        )]);
        next.after_cursor = Some(repaired.cursor);
        let continued = s.project_worker_events(next).await.unwrap();
        assert_eq!(continued.events[0].identity, identity);
        assert!(continued.active_turn.is_none());
        assert_eq!(
            s.pending_worker_settlements("session-1")
                .await
                .unwrap()
                .len(),
            1
        );
    }
}

#[tokio::test]
async fn rebuild_preserves_current_loop_controls_and_profiles_while_mapping_a_new_suffix() {
    let (s, _dir) = store().await;
    let automation = crate::automation::goal_loop::DatabaseAutomationStore::new(s.storage.clone());
    automation
        .create_loop(&talos_types::SessionLoop {
            id: "loop-1".into(),
            session_id: "session-1".into(),
            prompt: "Check progress".into(),
            schedule: talos_types::LoopSchedule::ModelDecided {},
            config_revision: 1,
            status: talos_types::LoopStatus::Active,
            stop_reason: None,
            reason: None,
            next_due_at: Some(100),
            latest_turn_id: None,
            created_at: 1,
            updated_at: 1,
        })
        .await
        .unwrap();
    let mut events = completed();
    events[1].automation_input = Some(talos_types::AutomationInputOrigin {
        message_id: "message-1".into(),
        origin: talos_types::AutomationTurnOrigin::Loop {
            loop_id: "loop-1".into(),
            occurrence_id: "occurrence-1".into(),
            config_revision: 1,
        },
    });
    s.project_worker_events(params(events[..2].to_vec()))
        .await
        .unwrap();
    s.storage.execute("UPDATE session_loops SET pending_turn_id=NULL,next_due_at=999,latest_turn_id='newer-turn' WHERE id='loop-1'", &[]).await.unwrap();
    s.storage.execute("INSERT INTO profile_turn_usage(user_id,usage_kind,session_id,turn_id,last_request_id,model_provider,model_id,input_tokens,output_tokens,total_tokens,usage_available,finalized,timezone) VALUES ('user-1','turn','session-1','turn-1','request-1','provider','model',3,4,7,1,1,'UTC')", &[]).await.unwrap();
    let loop_before = automation.loop_task("loop-1").await.unwrap();
    let profile_before = s
        .storage
        .query("SELECT * FROM profile_turn_usage", &[])
        .await
        .unwrap()
        .remove(0)
        .values;
    let rebuild = begin(&s).await;
    s.stage_worker_projection_rebuild(&rebuild, params(events))
        .await
        .unwrap();
    s.finish_worker_projection_rebuild(&rebuild).await.unwrap();
    assert_eq!(automation.loop_task("loop-1").await.unwrap(), loop_before);
    let pending = s
        .storage
        .query(
            "SELECT pending_turn_id FROM session_loops WHERE id='loop-1'",
            &[],
        )
        .await
        .unwrap()
        .remove(0);
    assert_eq!(pending.get_string("pending_turn_id"), None);
    assert_eq!(
        s.storage
            .query("SELECT * FROM profile_turn_usage", &[])
            .await
            .unwrap()
            .remove(0)
            .values,
        profile_before
    );
    assert_eq!(
        s.pending_worker_settlements("session-1")
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn rebuild_requires_the_original_timestamp_before_replacing_any_rows() {
    let (s, _dir) = store().await;
    let events = completed();
    s.project_worker_events(params(events.clone()))
        .await
        .unwrap();
    s.storage
        .execute(
            "UPDATE worker_event_projection SET product_persisted_at=NULL WHERE worker_offset=9",
            &[],
        )
        .await
        .unwrap();
    let before = s
        .list_agent_turn_events(&AgentTurnRef::new("session-1", "turn-1"), 10)
        .await
        .unwrap();
    let rebuild = begin(&s).await;
    s.stage_worker_projection_rebuild(&rebuild, params(events))
        .await
        .unwrap();
    assert!(s
        .finish_worker_projection_rebuild(&rebuild)
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    assert_eq!(
        s.list_agent_turn_events(&AgentTurnRef::new("session-1", "turn-1"), 10)
            .await
            .unwrap(),
        before
    );
}

#[tokio::test]
async fn rebuild_rejects_missing_start_business_watermark_after_waiting_settlement() {
    let (s, _dir) = store().await;
    let automation = crate::automation::goal_loop::DatabaseAutomationStore::new(s.storage.clone());
    automation
        .create_goal(
            &talos_types::SessionGoal {
                id: "goal-1".into(),
                session_id: "session-1".into(),
                objective: "Keep accounted usage".into(),
                objective_revision: 1,
                status: talos_types::GoalStatus::Active,
                pause_reason: None,
                reason: None,
                token_budget: Some(100),
                tokens_used: 0,
                latest_turn_id: None,
                completion_evidence: None,
                created_at: 1,
                updated_at: 1,
            },
            None,
        )
        .await
        .unwrap();
    let mut events = admission();
    events[1].automation_input = Some(talos_types::AutomationInputOrigin {
        message_id: "message-1".into(),
        origin: talos_types::AutomationTurnOrigin::Goal {
            goal_id: "goal-1".into(),
            objective_revision: 1,
        },
    });
    let mut waiting = fact(
        14,
        3,
        AgentEventPayload::WaitingForInput(talos_agent_api::AgentPendingInput::Message {
            action_id: "action-1".into(),
            prompt: "Continue?".into(),
        }),
    );
    waiting.settlement = Some(AgentExecutionSettlement::WaitingForInput {
        metrics: AgentRunMetrics {
            usage: talos_types::TokenUsage {
                input_tokens: 17,
                total_tokens: 17,
                ..Default::default()
            },
            ..Default::default()
        },
    });
    events.push(waiting);
    let commit = s
        .project_worker_events(params(events.clone()))
        .await
        .unwrap();
    let identity = AgentTurnRef::new("session-1", "turn-1");
    automation.account_usage(&identity, 17, 20).await.unwrap();
    s.complete_worker_settlement(&commit.cursor).await.unwrap();
    let original = s.storage.query("SELECT automation_accounted_tokens FROM agent_events WHERE session_id='session-1' AND sequence=1", &[]).await.unwrap().remove(0);
    assert_eq!(original.get_i64("automation_accounted_tokens"), Some(17));
    let goal = automation.goal("goal-1").await.unwrap();
    assert_eq!(goal.tokens_used, 17);
    s.storage
        .execute(
            "DELETE FROM agent_events WHERE session_id='session-1' AND sequence=1",
            &[],
        )
        .await
        .unwrap();
    let view = s.list_agent_turn_events(&identity, 10).await.unwrap();
    let active = s.get_agent_active_turn("session-1").await.unwrap();
    let ledger: Vec<_> = s
        .storage
        .query(
            "SELECT * FROM worker_event_projection ORDER BY worker_offset",
            &[],
        )
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.values)
        .collect();
    let product = s
        .storage
        .query("SELECT * FROM sessions WHERE session_key='session-1'", &[])
        .await
        .unwrap()
        .remove(0)
        .values;
    let rebuild = begin(&s).await;
    s.stage_worker_projection_rebuild(&rebuild, params(events))
        .await
        .unwrap();
    let error = s
        .finish_worker_projection_rebuild(&rebuild)
        .await
        .unwrap_err();
    assert!(error.is_code(ErrorCode::Conflict));
    assert_eq!(s.list_agent_turn_events(&identity, 10).await.unwrap(), view);
    assert_eq!(s.get_agent_active_turn("session-1").await.unwrap(), active);
    assert_eq!(
        s.storage
            .query(
                "SELECT * FROM worker_event_projection ORDER BY worker_offset",
                &[]
            )
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.values)
            .collect::<Vec<_>>(),
        ledger
    );
    assert_eq!(
        s.storage
            .query("SELECT * FROM sessions WHERE session_key='session-1'", &[])
            .await
            .unwrap()
            .remove(0)
            .values,
        product
    );
    assert_eq!(automation.goal("goal-1").await.unwrap(), goal);
    assert!(s
        .pending_worker_settlements("session-1")
        .await
        .unwrap()
        .is_empty());
}
