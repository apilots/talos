use super::*;
use talos_agent_api::{
    AgentAssistantText, AgentAssistantTextPhase, AgentMessageBlockDeltaContent, AgentTurnStarted,
    AgentUserContent,
};

#[tokio::test]
async fn pipeline_separates_admission_from_provider_progress() {
    let (pipeline, sink, mut progress) =
        AgentEventPipeline::fresh(AgentTurnRef::new("session-1", "turn-1"));
    let admission = pipeline
        .record(AgentEventPayload::TurnStarted(AgentTurnStarted {}))
        .await
        .expect("record Turn admission");
    assert_eq!(admission.sequence, 1);
    assert!(matches!(
        progress.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));

    let message = sink
        .start_message(AgentMessageRole::Agent)
        .await
        .expect("start Provider message");
    let block = sink
        .start_block(
            &message,
            AgentMessageBlockContent::AssistantText(AgentAssistantText {
                phase: AgentAssistantTextPhase::FinalAnswer,
                text: String::new(),
            }),
        )
        .await
        .expect("start Provider block");
    sink.append_block_delta(
        &block,
        AgentMessageBlockDeltaContent::Text("done".to_string()),
    )
    .await
    .expect("append text delta");
    sink.complete_block(&block, AgentMessageBlockStatus::Completed)
        .await
        .expect("complete block");
    sink.complete_message(&message, AgentMessageStatus::Completed)
        .await
        .expect("complete message");

    let mut events = Vec::new();
    while events.len() < 5 {
        events.push(progress.recv().await.expect("Provider progress event"));
    }
    assert_eq!(
        events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![2, 3, 4, 5, 6]
    );
    assert_eq!(
        pipeline.checkpoint().await.messages[0].blocks[0].content,
        AgentMessageBlockContent::AssistantText(AgentAssistantText {
            phase: AgentAssistantTextPhase::FinalAnswer,
            text: "done".to_string(),
        })
    );
}

#[tokio::test]
async fn provider_sink_cannot_create_user_lifecycle() {
    let (pipeline, sink, _progress) =
        AgentEventPipeline::fresh(AgentTurnRef::new("session-1", "turn-1"));
    pipeline
        .record(AgentEventPayload::TurnStarted(AgentTurnStarted {}))
        .await
        .unwrap();

    let error = sink
        .start_message(AgentMessageRole::User)
        .await
        .expect_err("Provider must not create trusted User messages");

    assert!(error.diagnostic_message().contains("cannot create User"));
}

#[tokio::test]
async fn handles_cannot_cross_event_sinks() {
    let (first_pipeline, first_sink, _first_progress) =
        AgentEventPipeline::fresh(AgentTurnRef::new("session-1", "turn-1"));
    first_pipeline
        .record(AgentEventPayload::TurnStarted(AgentTurnStarted {}))
        .await
        .unwrap();
    let first_message = first_sink
        .start_message(AgentMessageRole::Agent)
        .await
        .unwrap();

    let (second_pipeline, second_sink, _second_progress) =
        AgentEventPipeline::fresh(AgentTurnRef::new("session-2", "turn-2"));
    second_pipeline
        .record(AgentEventPayload::TurnStarted(AgentTurnStarted {}))
        .await
        .unwrap();

    let error = second_sink
        .complete_message(&first_message, AgentMessageStatus::Completed)
        .await
        .expect_err("message handle must remain scoped to its sink");

    assert!(error.diagnostic_message().contains("another event sink"));
}

#[tokio::test]
async fn user_admission_reservation_blocks_progress_until_commit() {
    let (pipeline, sink, mut progress) =
        AgentEventPipeline::fresh(AgentTurnRef::new("session-1", "turn-1"));
    pipeline
        .record(AgentEventPayload::TurnStarted(AgentTurnStarted {}))
        .await
        .unwrap();
    let (admission, reservation) = pipeline
        .reserve_user_message(completed_user_message())
        .await
        .expect("reserve User admission");

    let task = tokio::spawn(async move { sink.start_message(AgentMessageRole::Agent).await });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), progress.recv())
            .await
            .is_err()
    );

    reservation.commit().await.expect("commit admission");
    task.await
        .expect("join Provider event")
        .expect("start Provider message");
    assert_eq!(
        admission
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
    assert_eq!(
        progress.recv().await.expect("Provider progress").sequence,
        4
    );
}

#[tokio::test]
async fn dropped_user_admission_restores_reducer_sequence() {
    let (pipeline, sink, mut progress) =
        AgentEventPipeline::fresh(AgentTurnRef::new("session-1", "turn-1"));
    pipeline
        .record(AgentEventPayload::TurnStarted(AgentTurnStarted {}))
        .await
        .unwrap();
    let (_, reservation) = pipeline
        .reserve_user_message(completed_user_message())
        .await
        .expect("reserve User admission");

    drop(reservation);
    pipeline.wait_until_unreserved().await;
    sink.start_message(AgentMessageRole::Agent)
        .await
        .expect("start Provider message");

    assert_eq!(
        progress.recv().await.expect("Provider progress").sequence,
        2
    );
    assert!(pipeline
        .checkpoint()
        .await
        .messages
        .iter()
        .all(|message| message.role != AgentMessageRole::User));
}

#[tokio::test]
async fn user_admission_is_deterministic_for_continuation_claim_retry() {
    let input = AgentMessage {
        message_id: "message-user-1".to_string(),
        turn_id: "turn-1".to_string(),
        role: AgentMessageRole::User,
        status: AgentMessageStatus::Completed,
        blocks: vec![AgentMessageBlock {
            block_id: "block-user-1".to_string(),
            status: AgentMessageBlockStatus::Completed,
            content: AgentMessageBlockContent::UserContent(AgentUserContent::Text {
                text: "continue".to_string(),
            }),
            started_at: 20,
            completed_at: Some(21),
        }],
        created_at: 19,
        completed_at: Some(22),
    };
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let (first, _, _) = AgentEventPipeline::fresh(identity.clone());
    first
        .record(AgentEventPayload::TurnStarted(AgentTurnStarted {}))
        .await
        .unwrap();
    let first_events = first.record_user_message(input.clone()).await.unwrap();
    let (second, _, _) = AgentEventPipeline::fresh(identity);
    second
        .record(AgentEventPayload::TurnStarted(AgentTurnStarted {}))
        .await
        .unwrap();
    let second_events = second.record_user_message(input).await.unwrap();

    assert_eq!(first_events, second_events);
    assert_eq!(
        first_events
            .iter()
            .map(|event| event.occurred_at)
            .collect::<Vec<_>>(),
        vec![19, 20, 21, 22]
    );
}

fn completed_user_message() -> AgentMessage {
    AgentMessage {
        message_id: "message-user-1".to_string(),
        turn_id: "turn-1".to_string(),
        role: AgentMessageRole::User,
        status: AgentMessageStatus::Completed,
        blocks: Vec::new(),
        created_at: 1,
        completed_at: Some(1),
    }
}
