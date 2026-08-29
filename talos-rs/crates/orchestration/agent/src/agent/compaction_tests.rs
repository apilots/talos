use super::*;

#[test]
fn test_context_compaction_stream_events_map_to_single_timeline_item() {
    let mut timeline = TimelineBuilder::new(
        "request-compaction",
        "turn-compaction",
        "message-1",
        "session-1",
    );
    let mut state = KernelTimelineState::default();

    let started_events = emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::ContextCompaction,
            KernelStreamEventData::ContextCompaction(ContextCompactionStreamData {
                trigger: ContextCompactionTrigger::ThresholdExceeded,
                stage: ContextCompactionStage::Preparing,
                outcome: None,
                metrics: None,
                message: Some("Preparing context compaction.".to_string()),
            }),
        ),
    )
    .expect("context compaction start should map");

    let compaction_id = match &started_events[0].payload {
        AgentEventPayload::ItemStarted(payload) => {
            assert_eq!(payload.item.kind, TalosTimelineItemKind::ContextCompaction);
            assert_eq!(payload.item.phase, TalosTimelinePhase::System);
            assert!(matches!(
                payload.item.content,
                TalosTimelineContent::ContextCompaction {
                    trigger: ContextCompactionTrigger::ThresholdExceeded,
                    stage: ContextCompactionStage::Preparing,
                    ..
                }
            ));
            payload.item.item_id.clone()
        }
        _ => panic!("expected context compaction item start"),
    };

    let completed_events = emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::ContextCompaction,
            KernelStreamEventData::ContextCompaction(ContextCompactionStreamData {
                trigger: ContextCompactionTrigger::ThresholdExceeded,
                stage: ContextCompactionStage::Completed,
                outcome: Some(ContextCompactionOutcome::Applied),
                metrics: Some(ContextCompactionMetrics {
                    covers_until_seq: Some(12),
                    pre_compaction_total_tokens: 9000,
                    post_compaction_total_tokens: 3000,
                    compaction_input_tokens: 6000,
                    compaction_output_tokens: 500,
                    ..Default::default()
                }),
                message: Some("Context compaction complete.".to_string()),
            }),
        ),
    )
    .expect("context compaction completion should map");

    assert!(completed_events.iter().any(|event| matches!(
        &event.payload,
        AgentEventPayload::ItemDelta(payload)
            if payload.item_id == compaction_id
                && matches!(
                    payload.delta,
                    TalosTimelineDelta::ContextCompaction {
                        stage: Some(ContextCompactionStage::Completed),
                        outcome: Some(ContextCompactionOutcome::Applied),
                        ..
                    }
                )
    )));
    assert!(completed_events.iter().any(|event| matches!(
        &event.payload,
        AgentEventPayload::ItemCompleted(payload)
            if payload.item_id == compaction_id
                && payload.status == TalosTimelineItemStatus::Completed
    )));
    assert!(matches!(
        timeline
            .item(&compaction_id)
            .expect("compaction item should exist")
            .content,
        TalosTimelineContent::ContextCompaction {
            stage: ContextCompactionStage::Completed,
            outcome: Some(ContextCompactionOutcome::Applied),
            metrics: Some(ContextCompactionMetrics {
                covers_until_seq: Some(12),
                ..
            }),
            ..
        }
    ));
}

#[tokio::test]
async fn test_compact_session_rejects_an_existing_session_reservation() {
    let agent = build_test_agent();
    agent
        .try_reserve_active_turn("reserved-session")
        .expect("test should reserve active slot");

    let error = agent
        .compact_session(crate::AgentSessionCompactionParams {
            session: build_test_session("reserved-session", "default"),
            identity: crate::AgentSessionCompactionIdentity {
                request_id: "req-compact".to_string(),
                turn_id: "turn-compact".to_string(),
                message_id: "msg-compact".to_string(),
            },
            event_tx: tokio::sync::mpsc::channel(1).0,
        })
        .await
        .expect_err("compaction must share the session lifecycle reservation");

    assert!(error.is_code(talos_error::ErrorCode::ActiveTurnRunning));
}

#[tokio::test]
async fn explicit_compaction_streams_a_no_changes_maintenance_turn() {
    let (_workspace, agent) = build_temp_agent();
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(16);

    let usage = agent
        .compact_session(crate::AgentSessionCompactionParams {
            session: build_test_session("compact-session", "default"),
            identity: crate::AgentSessionCompactionIdentity {
                request_id: "req-compact".to_string(),
                turn_id: "turn-compact".to_string(),
                message_id: "msg-compact".to_string(),
            },
            event_tx,
        })
        .await
        .expect("empty session compaction should complete");

    let mut events = Vec::new();
    while let Some(event) = event_rx.recv().await {
        events.push(event);
    }

    assert_eq!(usage, TokenUsage::default());
    assert_eq!(
        events
            .iter()
            .map(|event| event.event_type)
            .collect::<Vec<_>>(),
        vec![
            AgentEventType::TurnStarted,
            AgentEventType::ItemStarted,
            AgentEventType::ItemDelta,
            AgentEventType::ItemDelta,
            AgentEventType::ItemCompleted,
            AgentEventType::TurnCompleted,
        ]
    );
    assert!(events.iter().all(|event| {
        event.session_id == "compact-session"
            && event.turn_id == "turn-compact"
            && event.message_id == "msg-compact"
    }));
    assert!(matches!(
        &events[1].payload,
        AgentEventPayload::ItemStarted(payload)
            if payload.item.kind == TalosTimelineItemKind::ContextCompaction
                && matches!(
                    &payload.item.content,
                    TalosTimelineContent::ContextCompaction {
                        trigger: ContextCompactionTrigger::UserRequested,
                        stage: ContextCompactionStage::Preparing,
                        ..
                    }
                )
    ));
    assert!(matches!(
        &events[3].payload,
        AgentEventPayload::ItemDelta(payload)
            if matches!(
                &payload.delta,
                TalosTimelineDelta::ContextCompaction {
                    stage: Some(ContextCompactionStage::Completed),
                    outcome: Some(ContextCompactionOutcome::NoChanges),
                    ..
                }
            )
    ));
    assert!(!agent.has_active_turn("compact-session"));
}

#[tokio::test]
async fn explicit_compaction_can_be_interrupted_through_the_existing_turn_lifecycle() {
    let workspace = tempfile::tempdir().expect("workspace");
    let agent = test_agent_harness(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        static_router(Arc::new(SequencedProvider {
            started: Arc::new(Notify::new()),
            calls: AtomicUsize::new(0),
        })),
        model_selection("gpt-4o"),
        Arc::new(ToolRegistry::new()),
        AgentKernelConfig {
            workspace_dir: workspace.path().display().to_string(),
            ..AgentKernelConfig::default()
        },
    )
    .expect("build agent");
    let session_id = "compact-cancel";
    let mut session = build_test_session(session_id, "default");
    session.config.agent.kernel.workspace_dir = workspace.path().display().to_string();
    let kernel_config =
        AgentHarnessImpl::kernel_config_for_session(&session).expect("resolve session workspace");
    let workspace_root = std::path::Path::new(&kernel_config.workspace_dir);
    FileSessionArtifactStore::default()
        .append_turn(
            workspace_root,
            &session_artifacts::TalosTurn {
                turn_id: "source-turn".to_string(),
                message_id: "source-message".to_string(),
                session_id: session_id.to_string(),
                seq: 1,
                timestamp: Utc::now(),
                items: vec![
                    talos_types::talos::RunItem::UserInput {
                        text: "remember this".to_string(),
                        file_ids: Vec::new(),
                    },
                    talos_types::talos::RunItem::OutputText {
                        text: "I will remember this".to_string(),
                    },
                ],
                usage: TokenUsage::default(),
                stop_reason: TalosStopReason::EndTurn,
            },
        )
        .await
        .expect("append source turn");
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(16);
    let compact_agent = agent.clone();
    let compact_task = tokio::spawn(async move {
        compact_agent
            .compact_session(crate::AgentSessionCompactionParams {
                session,
                identity: crate::AgentSessionCompactionIdentity {
                    request_id: "req-compact".to_string(),
                    turn_id: "turn-compact".to_string(),
                    message_id: "msg-compact".to_string(),
                },
                event_tx,
            })
            .await
    });
    let first_event = event_rx.recv().await.expect("compaction start event");
    assert_eq!(first_event.event_type, AgentEventType::TurnStarted);

    agent
        .interrupt(AgentInterruptParams {
            session_id: session_id.to_string(),
            turn_id: "turn-compact".to_string(),
        })
        .await
        .expect("interrupt compaction");
    let error = compact_task
        .await
        .expect("join compaction request")
        .expect_err("compaction should be cancelled");
    let mut item_status = None;
    let mut stop_reason = None;
    while let Some(event) = event_rx.recv().await {
        match event.payload {
            AgentEventPayload::ItemCompleted(payload) => item_status = Some(payload.status),
            AgentEventPayload::TurnCompleted(payload) => stop_reason = Some(payload.stop_reason),
            _ => {}
        }
    }

    assert!(error.is_code(talos_error::ErrorCode::Cancelled));
    assert_eq!(
        (item_status, stop_reason, agent.has_active_turn(session_id)),
        (
            Some(TalosTimelineItemStatus::Cancelled),
            Some(TalosStopReason::Cancelled),
            false
        )
    );
}

#[tokio::test]
async fn explicit_compaction_persists_recovery_for_the_next_prompt() {
    let (workspace, agent) = build_temp_agent();
    let session_id = "compact-recovery";
    let mut session = build_test_session(session_id, "default");
    session.config.agent.kernel.workspace_dir = workspace.path().display().to_string();
    let kernel_config =
        AgentHarnessImpl::kernel_config_for_session(&session).expect("resolve session workspace");
    let workspace_root = std::path::Path::new(&kernel_config.workspace_dir);
    FileSessionArtifactStore::default()
        .append_turn(
            workspace_root,
            &session_artifacts::TalosTurn {
                turn_id: "source-turn".to_string(),
                message_id: "source-message".to_string(),
                session_id: session_id.to_string(),
                seq: 1,
                timestamp: Utc::now(),
                items: vec![
                    talos_types::talos::RunItem::UserInput {
                        text: "User prefers tea over coffee".to_string(),
                        file_ids: Vec::new(),
                    },
                    talos_types::talos::RunItem::OutputText {
                        text: "Preference noted".to_string(),
                    },
                ],
                usage: TokenUsage::default(),
                stop_reason: TalosStopReason::EndTurn,
            },
        )
        .await
        .expect("append source turn");
    let (event_tx, _event_rx) = tokio::sync::mpsc::channel(16);
    agent
        .compact_session(crate::AgentSessionCompactionParams {
            session,
            identity: crate::AgentSessionCompactionIdentity {
                request_id: "req-compact".to_string(),
                turn_id: "turn-compact".to_string(),
                message_id: "msg-compact".to_string(),
            },
            event_tx,
        })
        .await
        .expect("compact session");

    let records = FileSessionArtifactStore::default()
        .load_transcript_records(
            workspace_root,
            session_id,
            Some("turn-compact"),
            Some(session_artifacts::SessionTranscriptBranch::ModelThinking),
        )
        .await
        .expect("load recovery state");
    let state = records
        .iter()
        .flat_map(|record| &record.actions.state_delta)
        .find(|delta| delta.key == talos_agent_kernel::SESSION_COMPACTION_STATE_KEY)
        .expect("recovery state delta");
    assert_eq!(
        state.value,
        serde_json::to_value(talos_agent_kernel::SessionCompactionState::recovery())
            .expect("serialize expected state")
            .into()
    );
}
