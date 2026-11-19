use super::*;

#[tokio::test]
async fn late_explicit_phases_determine_commentary_and_final_text_without_tools() {
    let provider = Arc::new(ScriptedModelBlockProvider::new(
        "late-response-phases",
        vec![
            assistant_started("progress", ModelOutputPhase::Unknown),
            ModelBlockStreamEvent::BlockDelta {
                model_item_id: "progress".into(),
                delta: ModelBlockDelta::AssistantText {
                    delta: "Progress.".into(),
                },
            },
            block_completed("progress"),
            assistant_started("answer", ModelOutputPhase::Unknown),
            ModelBlockStreamEvent::BlockDelta {
                model_item_id: "answer".into(),
                delta: ModelBlockDelta::AssistantText {
                    delta: "Answer.".into(),
                },
            },
            ModelBlockStreamEvent::AssistantTextPhase {
                model_item_id: "answer".into(),
                phase: ModelOutputPhase::FinalAnswer,
            },
            block_completed("answer"),
            ModelBlockStreamEvent::AssistantTextPhase {
                model_item_id: "progress".into(),
                phase: ModelOutputPhase::Commentary,
            },
            done(),
        ],
    ));
    let (tx, mut rx) = mpsc::channel(16);
    let result = stream_round(
        provider,
        ChatRequest::new_user("test", "test-model"),
        0,
        KernelModelCallMode::WorkingLoop,
        None,
        Some(&tx),
        None,
        None,
    )
    .await
    .unwrap();
    assert!(result.tool_calls.is_empty());
    assert!(result.final_answer_streamed);
    assert_eq!(result.content, "Answer.");
    let mut committed = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let AgentKernelEventPayload::AssistantTextCommitted(data) = event.payload {
            committed.push((data.item_key.local_id, data.phase));
        }
    }
    assert_eq!(
        committed,
        [
            ("answer".into(), AssistantTextPhase::FinalAnswer),
            ("progress".into(), AssistantTextPhase::Commentary)
        ]
    );
}
