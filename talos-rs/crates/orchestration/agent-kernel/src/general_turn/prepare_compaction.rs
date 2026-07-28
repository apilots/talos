use super::*;
use talos_prompt::PromptSectionKind;
use talos_types::talos::{
    ContextCompactionMetrics, ContextCompactionOutcome, ContextCompactionStage,
    ContextCompactionTrigger,
};

/// Session transcript state key controlling the next prompt's compaction policy.
pub const SESSION_COMPACTION_STATE_KEY: &str = "session_compaction_state";

pub(super) fn count_section_kind(
    sections: &[talos_prompt::PromptSection],
    kind: PromptSectionKind,
) -> usize {
    sections
        .iter()
        .filter(|section| section.kind == kind)
        .count()
}

fn history_sections_were_dropped(prompt_assembly: &talos_prompt::PromptAssembly) -> bool {
    prompt_assembly
        .diagnostics
        .dropped_sections
        .iter()
        .any(|kind| {
            matches!(
                kind,
                PromptSectionKind::RecentMessages | PromptSectionKind::SessionSummary
            )
        })
}

fn prompt_budget_usage_percent(window_usage: &talos_prompt::PromptWindowUsage) -> Option<u8> {
    if !window_usage.window_resolved {
        return None;
    }
    if window_usage.effective_prompt_budget == 0 {
        return Some(100);
    }
    let usage_percent =
        (window_usage.total_tokens.saturating_mul(100)) / window_usage.effective_prompt_budget;
    Some(usage_percent.min(100) as u8)
}

pub(super) fn resolve_compaction_input_budget_tokens(
    model_info: &talos_types::ModelInfo,
) -> Option<usize> {
    let context_window = model_info.model.context_window as usize;
    if context_window == 0 {
        return None;
    }

    Some((context_window / 2).clamp(256, 16_384).min(context_window))
}

pub(super) fn determine_history_compaction_trigger(
    prompt_assembly: &talos_prompt::PromptAssembly,
    kernel_config: &AgentKernelConfig,
) -> Option<SessionHistoryCompactionTrigger> {
    let history_reduction_needed = history_sections_were_dropped(prompt_assembly);
    let window_usage = &prompt_assembly.diagnostics.window_usage;
    if !window_usage.window_resolved {
        return None;
    }

    if !window_usage.within_effective_budget {
        Some(SessionHistoryCompactionTrigger::BudgetExceeded)
    } else if prompt_budget_usage_percent(window_usage)
        .is_some_and(|usage| usage >= kernel_config.compaction.hard_ratio_percent)
    {
        Some(SessionHistoryCompactionTrigger::HardThresholdExceeded)
    } else if prompt_budget_usage_percent(window_usage)
        .is_some_and(|usage| usage >= kernel_config.compaction.trigger_ratio_percent)
    {
        Some(SessionHistoryCompactionTrigger::ThresholdExceeded)
    } else if history_reduction_needed {
        Some(SessionHistoryCompactionTrigger::HistorySectionsDropped)
    } else {
        None
    }
}

pub(super) fn is_recoverable_compaction_parse_error(error: &talos_error::TalosError) -> bool {
    matches!(
        error,
        talos_error::TalosError::MemoryError {
            kind: talos_error::TalosMemoryErrorKind::SummaryParseFailed,
            ..
        }
    )
}

pub(super) async fn emit_context_compaction_stream_event(
    stream_tx: Option<&mpsc::Sender<KernelStreamEvent>>,
    trigger: ContextCompactionTrigger,
    stage: ContextCompactionStage,
    outcome: Option<ContextCompactionOutcome>,
    metrics: Option<ContextCompactionMetrics>,
    message: Option<String>,
) {
    emit_stream_event(
        stream_tx,
        KernelStreamEventType::ContextCompaction,
        KernelStreamEventData::ContextCompaction(crate::ContextCompactionStreamData {
            trigger,
            stage,
            outcome,
            metrics,
            message,
        }),
    )
    .await;
}

pub(super) fn map_context_compaction_trigger(
    trigger: SessionHistoryCompactionTrigger,
) -> ContextCompactionTrigger {
    match trigger {
        SessionHistoryCompactionTrigger::ThresholdExceeded => {
            ContextCompactionTrigger::ThresholdExceeded
        }
        SessionHistoryCompactionTrigger::HardThresholdExceeded => {
            ContextCompactionTrigger::HardThresholdExceeded
        }
        SessionHistoryCompactionTrigger::BudgetExceeded => ContextCompactionTrigger::BudgetExceeded,
        SessionHistoryCompactionTrigger::HistorySectionsDropped => {
            ContextCompactionTrigger::HistorySectionsDropped
        }
    }
}

pub(super) fn map_context_compaction_outcome(
    outcome: SessionHistoryCompactionOutcome,
) -> ContextCompactionOutcome {
    match outcome {
        SessionHistoryCompactionOutcome::Applied => ContextCompactionOutcome::Applied,
        SessionHistoryCompactionOutcome::RejectedMissingSummary => {
            ContextCompactionOutcome::RejectedMissingSummary
        }
    }
}

pub(super) fn context_compaction_failure_outcome(error: &TalosError) -> ContextCompactionOutcome {
    match error {
        TalosError::NetworkError(_)
        | TalosError::RuntimeUnavailable(_)
        | TalosError::TransportUnavailable(_)
        | TalosError::UnsupportedModel(_)
        | TalosError::Timeout(_)
        | TalosError::ToolError(_) => ContextCompactionOutcome::FailedProviderError,
        _ => ContextCompactionOutcome::FailedInternalError,
    }
}

pub(super) fn build_context_compaction_metrics(
    compaction: &SessionHistoryCompaction,
) -> ContextCompactionMetrics {
    ContextCompactionMetrics {
        covers_from_seq: compaction.covers_from_seq,
        covers_until_seq: compaction.covers_until_seq,
        pre_compaction_total_tokens: saturating_u32(compaction.pre_compaction_total_tokens),
        post_compaction_total_tokens: saturating_u32(compaction.post_compaction_total_tokens),
        compaction_input_tokens: compaction.compaction_input_tokens,
        compaction_output_tokens: compaction.compaction_output_tokens,
        trimmed_context_unit_count: compaction.trimmed_context_unit_count,
        raw_tail_preferred_count: Some(saturating_u32(compaction.recent_messages_after)),
        ..Default::default()
    }
}

pub(super) fn token_usage_from_session_history_compaction(
    compaction: Option<&SessionHistoryCompaction>,
) -> TokenUsage {
    let Some(compaction) = compaction else {
        return TokenUsage::default();
    };
    TokenUsage {
        input_tokens: compaction.compaction_input_tokens,
        output_tokens: compaction.compaction_output_tokens,
        total_tokens: compaction.compaction_total_tokens,
        cache_creation_input_tokens: compaction.compaction_cache_creation_input_tokens,
        cache_read_input_tokens: compaction.compaction_cache_read_input_tokens,
    }
}

pub(super) async fn latest_compact_prompt_cursor(
    session_artifact_store: &dyn talos_store::session::SessionArtifactStore,
    kernel_config: &AgentKernelConfig,
    session_id: &str,
) -> TalosResult<Option<talos_prompt::CompactPromptCursor>> {
    let summaries = session_artifact_store
        .load_summaries(
            std::path::Path::new(&kernel_config.workspace_dir),
            session_id,
            1,
        )
        .await?;
    Ok(summaries
        .last()
        .map(|summary| talos_prompt::CompactPromptCursor {
            covers_until_seq: summary.covers_until_seq,
        }))
}

fn saturating_u32(value: usize) -> u32 {
    value.min(u32::MAX as usize) as u32
}
