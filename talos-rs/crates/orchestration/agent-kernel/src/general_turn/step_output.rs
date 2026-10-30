//! Canonical assistant text and lifecycle event emission.
use super::*;

pub(crate) async fn emit_stream_event(
    stream_tx: Option<&mpsc::Sender<AgentKernelEvent>>,
    payload: AgentKernelEventPayload,
) {
    if let Some(tx) = stream_tx {
        let _ = tx.send(AgentKernelEvent::new(payload)).await;
    }
}

pub(crate) async fn emit_assistant_text_stream_item(
    stream_tx: Option<&mpsc::Sender<AgentKernelEvent>>,
    item_key: KernelStreamItemKey,
    phase: AssistantTextPhase,
    round: Option<u32>,
    text: &str,
) -> bool {
    if stream_tx.is_none() || text.trim().is_empty() {
        return false;
    }

    emit_stream_event(
        stream_tx,
        AgentKernelEventPayload::AssistantTextStarted(AssistantTextStartedStreamData {
            item_key: item_key.clone(),
            phase,
            round,
        }),
    )
    .await;
    emit_stream_event(
        stream_tx,
        AgentKernelEventPayload::AssistantTextDelta(AssistantTextDeltaStreamData {
            item_key: item_key.clone(),
            delta: text.to_string(),
        }),
    )
    .await;
    emit_stream_event(
        stream_tx,
        AgentKernelEventPayload::AssistantTextCompleted(AssistantTextCompletedStreamData {
            item_key,
        }),
    )
    .await;
    true
}

pub(super) async fn emit_assistant_text_for_step(
    stream_tx: Option<&mpsc::Sender<AgentKernelEvent>>,
    round: u32,
    text: &str,
    disposition: GeneralTurnStepDisposition,
) -> bool {
    let phase = match disposition {
        GeneralTurnStepDisposition::ContinueWithTools => AssistantTextPhase::Commentary,
        GeneralTurnStepDisposition::FinalResponse => AssistantTextPhase::FinalAnswer,
    };
    emit_assistant_text_stream_item(
        stream_tx,
        KernelStreamItemKey {
            round: Some(round),
            local_id: "assistant_text".to_string(),
        },
        phase,
        Some(round),
        text,
    )
    .await
}
