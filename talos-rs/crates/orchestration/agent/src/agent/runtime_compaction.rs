use super::runtime::{
    close_active_reasoning_item, item_id_from_started, reject_after_final_answer,
    KernelTimelineState,
};
use talos_agent_kernel::{ContextCompactionStreamData, TimelineBuilder};
use talos_error::TalosResult;
use talos_types::talos::{
    AgentEvent, ContextCompactionStage, TalosTimelineDelta, TalosTimelineItemStatus,
};

pub(super) fn emit_context_compaction_timeline_events(
    timeline: &mut TimelineBuilder,
    state: &mut KernelTimelineState,
    data: ContextCompactionStreamData,
) -> TalosResult<Vec<AgentEvent>> {
    reject_after_final_answer(state, "context compaction")?;
    let mut events = close_active_reasoning_item(timeline, state)?;
    let item_id = match state.context_compaction_item_id.clone() {
        Some(item_id) => item_id,
        None => {
            let started =
                timeline.start_context_compaction(data.trigger, data.stage, data.message.clone());
            let item_id = item_id_from_started(&started)?;
            state.context_compaction_item_id = Some(item_id.clone());
            events.push(started);
            item_id
        }
    };
    events.push(timeline.append_delta(
        &item_id,
        TalosTimelineDelta::ContextCompaction {
            stage: Some(data.stage),
            outcome: data.outcome,
            metrics: data.metrics.clone(),
            message: data.message.clone(),
        },
    )?);
    let status = match data.stage {
        ContextCompactionStage::Completed => Some(TalosTimelineItemStatus::Completed),
        ContextCompactionStage::Failed => Some(TalosTimelineItemStatus::Failed),
        ContextCompactionStage::Preparing
        | ContextCompactionStage::Summarizing
        | ContextCompactionStage::RebuildingPrompt
        | ContextCompactionStage::Trimming => None,
    };
    if let Some(status) = status {
        let final_content = timeline.item(&item_id).map(|item| item.content.clone());
        events.push(timeline.complete_item(&item_id, status, final_content)?);
        state.context_compaction_item_id = None;
    }
    Ok(events)
}
