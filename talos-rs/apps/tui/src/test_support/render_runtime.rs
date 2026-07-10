use crate::render_runtime::render::live::LiveTextKind;
use crate::render_runtime::runtime::engine::RenderRuntimeEngine;
use talos_sdk::{
    TalosTimelineContent, TalosTimelineDelta, TalosTimelineItem, TalosTimelineItemKind,
    TalosTimelineItemStatus, TalosTimelinePhase,
};

pub(crate) fn start_live_text(
    engine: &mut RenderRuntimeEngine,
    item_id: &str,
    kind: LiveTextKind,
    text: &str,
) {
    let (item_kind, phase) = match kind {
        LiveTextKind::Commentary => (
            TalosTimelineItemKind::AssistantText,
            TalosTimelinePhase::Commentary,
        ),
        LiveTextKind::Thinking => (
            TalosTimelineItemKind::Reasoning,
            TalosTimelinePhase::Thinking,
        ),
        LiveTextKind::FinalAnswer => (
            TalosTimelineItemKind::AssistantText,
            TalosTimelinePhase::FinalAnswer,
        ),
    };
    engine.apply_timeline_item_started(&TalosTimelineItem {
        item_id: item_id.to_string(),
        parent_item_id: None,
        kind: item_kind,
        phase,
        status: TalosTimelineItemStatus::Running,
        title: None,
        started_at_ms: 1,
        completed_at_ms: None,
        content: TalosTimelineContent::Text {
            text: text.to_string(),
        },
    });
}

pub(crate) fn append_live_text(
    engine: &mut RenderRuntimeEngine,
    item_id: &str,
    kind: LiveTextKind,
    text: &str,
) {
    let item = live_text_item(item_id, kind);
    engine.apply_timeline_item_delta(
        &item,
        TalosTimelineDelta::Text {
            text: text.to_string(),
        },
    );
}

pub(crate) fn finalize_live_text(
    engine: &mut RenderRuntimeEngine,
    item_id: &str,
    kind: LiveTextKind,
) {
    let item = live_text_item(item_id, kind);
    engine.apply_timeline_item_completed(&item, None);
}

pub(crate) fn start_live_tool(
    engine: &mut RenderRuntimeEngine,
    item_id: &str,
    call_id: &str,
    name: &str,
    arguments: &str,
) {
    engine.apply_timeline_item_started(&TalosTimelineItem {
        item_id: item_id.to_string(),
        parent_item_id: None,
        kind: TalosTimelineItemKind::ToolCall,
        phase: TalosTimelinePhase::Acting,
        status: TalosTimelineItemStatus::Running,
        title: None,
        started_at_ms: 1,
        completed_at_ms: None,
        content: TalosTimelineContent::ToolCall {
            call_id: call_id.to_string(),
            name: name.to_string(),
            arguments: arguments.to_string(),
            progress: Vec::new(),
            result: None,
        },
    });
}

pub(crate) fn append_live_tool_progress(
    engine: &mut RenderRuntimeEngine,
    item_id: &str,
    text: &str,
) {
    let item = TalosTimelineItem {
        item_id: item_id.to_string(),
        parent_item_id: None,
        kind: TalosTimelineItemKind::ToolCall,
        phase: TalosTimelinePhase::Acting,
        status: TalosTimelineItemStatus::Running,
        title: None,
        started_at_ms: 1,
        completed_at_ms: None,
        content: TalosTimelineContent::Empty,
    };
    engine.apply_timeline_item_delta(
        &item,
        TalosTimelineDelta::ToolProgress {
            progress_kind: talos_sdk::TalosToolProgressKind::Stdout,
            text_delta: text.to_string(),
        },
    );
}

fn live_text_item(item_id: &str, kind: LiveTextKind) -> TalosTimelineItem {
    let (item_kind, phase) = match kind {
        LiveTextKind::Commentary => (
            TalosTimelineItemKind::AssistantText,
            TalosTimelinePhase::Commentary,
        ),
        LiveTextKind::Thinking => (
            TalosTimelineItemKind::Reasoning,
            TalosTimelinePhase::Thinking,
        ),
        LiveTextKind::FinalAnswer => (
            TalosTimelineItemKind::AssistantText,
            TalosTimelinePhase::FinalAnswer,
        ),
    };
    TalosTimelineItem {
        item_id: item_id.to_string(),
        parent_item_id: None,
        kind: item_kind,
        phase,
        status: TalosTimelineItemStatus::Running,
        title: None,
        started_at_ms: 1,
        completed_at_ms: None,
        content: TalosTimelineContent::Text {
            text: String::new(),
        },
    }
}
